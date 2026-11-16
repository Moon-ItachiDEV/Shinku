//! Suivre une désinstallation : du lancement du désinstalleur jusqu'aux restes.
//!
//! Une seule à la fois. L'état vit **en mémoire** dans le guichet, jamais sur le
//! disque. Les phases :
//!
//! ```text
//! Idle ─▶ Launching ─▶ Waiting ─▶ Settling ─▶ Searching ─▶ Removed
//!   ▲          │          │  ▲        │                        │
//!   └──────────┴──────────┘  └────────┘ (l'inscription revient) │
//!   └───────────────────────────────────────────────────────────┘ (stop)
//! ```
//!
//! - `Launching` : le lancement est en cours. `ShellExecuteExW` ne rend la main
//!   qu'après la réponse à la fenêtre de Windows, qui peut rester ouverte des
//!   minutes : la phase est posée **avant** de lancer, pour qu'un second geste
//!   soit refusé pendant ce temps et que la page puisse se redessiner ;
//! - `Waiting` : lancé, et l'inscription est encore là. **Le registre est la
//!   seule vérité** : hors Windows Installer, le processus lancé n'est pas suivi
//!   (beaucoup de désinstalleurs se recopient ailleurs et le premier processus
//!   sort tout de suite ; un désinstalleur élevé ne se laisse pas observer).
//!   Pour Windows Installer, dont le processus dure jusqu'à la fin, un
//!   processus sorti alors que l'inscription est encore là deux lectures de
//!   suite veut dire « la personne a annulé » ;
//! - `Settling` : l'inscription a disparu. Inno Setup et NSIS la retirent
//!   **avant** les fichiers : chercher trop tôt montrerait un dossier que le
//!   désinstalleur est en train de vider. On sort de là quand 5 secondes sont
//!   passées **et** que deux lectures de suite, à 2 secondes d'écart au moins,
//!   voient le même contenu direct dans l'emplacement de l'app (60 secondes au
//!   plus) ;
//! - `Searching` : les restes sont cherchés, hors du verrou (c'est un travail de
//!   disque) ; la page voit encore « presque fini » ;
//! - `Removed` : les restes sont gardés tant que la personne n'a pas fini.
//!
//! Chaque changement de phase porte un numéro de génération : le résultat d'un
//! travail de disque (recherche, mise à la corbeille) qui arrive après un
//! « Revenir à la liste » est jeté.

use std::time::Duration;

use crate::apps::AppId;
use crate::apps::host::{AppsHost, Launch, Watch};
use crate::apps::leftovers::{Found, Leftover, RemoveOutcome, RemoveResult, Snapshot};
use crate::apps::view::{FollowView, LeftoverRow, UninstallRefusal};

/// Au bout de ce temps sans que l'inscription disparaisse, on rend la main.
const WAITING_LIMIT: Duration = Duration::from_secs(30 * 60);
/// Temps minimum entre la disparition de l'inscription et la recherche.
const SETTLE_MIN: Duration = Duration::from_secs(5);
/// Temps maximum d'attente d'un désinstalleur qui vide encore son dossier.
const SETTLE_MAX: Duration = Duration::from_secs(60);
/// Écart minimum entre deux lectures du contenu de l'emplacement.
const SETTLE_GAP: Duration = Duration::from_secs(2);

/// L'app dont on suit la désinstallation.
#[derive(Debug)]
pub struct Tracked {
    /// Son identifiant.
    pub id: AppId,
    /// Son nom.
    pub name: String,
    /// Ce qu'on savait d'elle avant de lancer.
    pub snapshot: Snapshot,
    /// Vrai si c'est Windows Installer (son processus se suit).
    pub installer: bool,
    /// Depuis quand le désinstalleur est lancé (horloge de l'hôte).
    pub since: Duration,
}

/// Le contenu direct d'un dossier, pour voir s'il se vide encore.
#[derive(Debug)]
enum Content {
    /// Le dossier n'existe pas.
    Absent,
    /// Ses noms, pliés et triés.
    Names(Vec<String>),
    /// Illisible : jamais égal à rien.
    Unknown,
}

impl PartialEq for Content {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Absent, Self::Absent) => true,
            (Self::Names(left), Self::Names(right)) => left == right,
            _ => false,
        }
    }
}

#[derive(Debug)]
enum Phase {
    Idle,
    Launching(Tracked),
    Waiting { app: Tracked, watch: Option<Box<dyn Watch>>, ended_reads: u8 },
    Settling { app: Tracked, gone_at: Duration, last: Option<(Duration, Content)> },
    Searching(Tracked),
    Removed { app: Tracked, leftovers: Vec<Leftover>, family: bool, complete: bool },
}

/// Ce que l'appelant doit faire après un `poll`, hors du verrou.
#[derive(Debug)]
pub enum Step {
    /// Rien.
    Nothing,
    /// Chercher les restes, puis rendre le résultat à [`Follow::apply_search`].
    Search {
        /// La génération à rendre avec le résultat.
        generation: u64,
        /// Ce qu'on savait de l'app.
        snapshot: Snapshot,
        /// Le numéro de la prochaine clé de reste.
        next_key: u32,
    },
}

/// L'état de la désinstallation en cours.
#[derive(Debug)]
pub struct Follow {
    phase: Phase,
    generation: u64,
    next_key: u32,
    kept: bool,
}

impl Default for Follow {
    fn default() -> Self {
        Self::new()
    }
}

impl Follow {
    /// Rien en cours.
    pub fn new() -> Self {
        Self { phase: Phase::Idle, generation: 0, next_key: 0, kept: false }
    }

    /// Vrai si rien n'est en cours.
    pub fn is_idle(&self) -> bool {
        matches!(self.phase, Phase::Idle)
    }

    fn enter(&mut self, phase: Phase) {
        self.generation += 1;
        self.phase = phase;
    }

    /// Passe en `Launching` **avant** de lancer. `None` si une désinstallation
    /// est déjà en cours.
    pub fn begin(&mut self, app: Tracked) -> Option<u64> {
        if !self.is_idle() {
            return None;
        }
        self.enter(Phase::Launching(app));
        Some(self.generation)
    }

    /// Le désinstalleur a répondu. `Started` passe en `Waiting` ; tout le reste
    /// revient à `Idle` avec la raison.
    pub fn launched(
        &mut self,
        generation: u64,
        launch: Launch,
        now: Duration,
    ) -> Result<(), UninstallRefusal> {
        if self.generation != generation {
            return Err(UninstallRefusal::Failed);
        }
        let Phase::Launching(mut app) = std::mem::replace(&mut self.phase, Phase::Idle) else {
            return Err(UninstallRefusal::Failed);
        };
        match launch {
            Launch::Started(watch) => {
                app.since = now;
                self.enter(Phase::Waiting { app, watch, ended_reads: 0 });
                Ok(())
            }
            Launch::Cancelled => {
                self.enter(Phase::Idle);
                Err(UninstallRefusal::Cancelled)
            }
            Launch::Missing => {
                self.enter(Phase::Idle);
                Err(UninstallRefusal::Broken)
            }
            Launch::Failed(_) => {
                self.enter(Phase::Idle);
                Err(UninstallRefusal::Failed)
            }
        }
    }

    /// Un pas de suivi : relit le registre, avance les phases. À appeler toutes
    /// les deux secondes environ.
    pub fn poll(&mut self, host: &dyn AppsHost) -> Step {
        let now = host.uptime();
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Waiting { app, watch, mut ended_reads } => {
                if now.saturating_sub(app.since) >= WAITING_LIMIT {
                    self.enter(Phase::Idle);
                    return Step::Nothing;
                }
                match host.registration(app.id.source(), app.id.key()) {
                    Ok(None) => self.enter(Phase::Settling { app, gone_at: now, last: None }),
                    Ok(Some(_)) => {
                        let exited = app.installer && watch.as_ref().is_some_and(|w| w.ended());
                        ended_reads = if exited { ended_reads + 1 } else { 0 };
                        if ended_reads >= 2 {
                            // L'installeur de Windows est sorti, l'app est restée.
                            self.kept = true;
                            self.enter(Phase::Idle);
                        } else {
                            self.phase = Phase::Waiting { app, watch, ended_reads };
                        }
                    }
                    // Illisible : on ne conclut rien.
                    Err(_) => self.phase = Phase::Waiting { app, watch, ended_reads: 0 },
                }
                Step::Nothing
            }
            Phase::Settling { app, gone_at, mut last } => {
                // L'inscription est revenue (une mise à niveau, une réinstallation).
                if matches!(host.registration(app.id.source(), app.id.key()), Ok(Some(_))) {
                    self.enter(Phase::Waiting { app, watch: None, ended_reads: 0 });
                    return Step::Nothing;
                }
                let waited = now.saturating_sub(gone_at);
                let done = if waited >= SETTLE_MAX {
                    true
                } else if let Some(location) = app.snapshot.location.as_deref() {
                    // La lecture d'un dossier (4096 noms au plus) se fait ici, sous le
                    // verrou du suivi : c'est court, et bornée à un seul dossier.
                    let due =
                        last.as_ref().is_none_or(|(at, _)| now.saturating_sub(*at) >= SETTLE_GAP);
                    if due {
                        let current = content_of(host, location);
                        let stable = last.as_ref().is_some_and(|(_, before)| *before == current);
                        last = Some((now, current));
                        stable && waited >= SETTLE_MIN
                    } else {
                        false
                    }
                } else {
                    waited >= SETTLE_MIN
                };
                if done {
                    let snapshot = app.snapshot.clone();
                    self.enter(Phase::Searching(app));
                    Step::Search { generation: self.generation, snapshot, next_key: self.next_key }
                } else {
                    self.phase = Phase::Settling { app, gone_at, last };
                    Step::Nothing
                }
            }
            other => {
                self.phase = other;
                Step::Nothing
            }
        }
    }

    /// Le résultat d'une recherche de restes (première, ou « Chercher à nouveau »).
    /// Jeté si la phase a changé entre-temps.
    pub fn apply_search(&mut self, generation: u64, found: Found, next_key: u32) {
        if self.generation != generation
            || !matches!(self.phase, Phase::Searching(_) | Phase::Removed { .. })
        {
            return;
        }
        let (Phase::Searching(app) | Phase::Removed { app, .. }) =
            std::mem::replace(&mut self.phase, Phase::Idle)
        else {
            return;
        };
        self.next_key = next_key;
        self.phase = Phase::Removed {
            app,
            leftovers: found.leftovers,
            family: found.family,
            complete: found.complete,
        };
    }

    /// De quoi chercher les restes à nouveau (phase `Removed` seulement).
    pub fn rescan_input(&self) -> Option<(u64, Snapshot, u32)> {
        match &self.phase {
            Phase::Removed { app, .. } => {
                Some((self.generation, app.snapshot.clone(), self.next_key))
            }
            _ => None,
        }
    }

    /// De quoi retirer des restes (phase `Removed` seulement).
    pub fn removal_input(&self) -> Option<(Snapshot, Vec<Leftover>)> {
        match &self.phase {
            Phase::Removed { app, leftovers, .. } => {
                Some((app.snapshot.clone(), leftovers.clone()))
            }
            _ => None,
        }
    }

    /// Ce qui est parti (ou n'existait plus) quitte la liste ; le reste y reste,
    /// pour que la personne voie ce qui reste. Pas de numéro de génération ici :
    /// les clés des restes ne sont **jamais réutilisées** (le compteur survit à
    /// « Revenir à la liste »), un résultat d'avant ne peut donc retirer aucun
    /// reste d'une autre app.
    pub fn apply_removal(&mut self, results: &[RemoveResult]) {
        if let Phase::Removed { leftovers, .. } = &mut self.phase {
            leftovers.retain(|leftover| {
                !results.iter().any(|result| {
                    result.key == leftover.key
                        && matches!(
                            result.outcome,
                            RemoveOutcome::Removed | RemoveOutcome::AlreadyGone
                        )
                })
            });
        }
    }

    /// Revient à la liste. Ne tue rien, ne touche à rien. Sans effet pendant le
    /// lancement : Windows attend encore la réponse de la personne.
    pub fn stop(&mut self) {
        if !matches!(self.phase, Phase::Launching(_) | Phase::Idle) {
            self.enter(Phase::Idle);
        }
    }

    /// Dit une seule fois : l'installeur de Windows est sorti, l'app est restée.
    pub fn take_kept(&mut self) -> bool {
        std::mem::take(&mut self.kept)
    }

    /// Ce que la page voit de la phase.
    pub fn view(&self, now: Duration) -> FollowView {
        match &self.phase {
            Phase::Idle => FollowView::Idle,
            Phase::Launching(app) => {
                FollowView::Launching { id: app.id.clone(), name: app.name.clone() }
            }
            Phase::Waiting { app, .. } => FollowView::Waiting {
                id: app.id.clone(),
                name: app.name.clone(),
                seconds: now.saturating_sub(app.since).as_secs(),
            },
            Phase::Settling { app, .. } | Phase::Searching(app) => {
                FollowView::Settling { id: app.id.clone(), name: app.name.clone() }
            }
            Phase::Removed { app, leftovers, family, complete } => FollowView::Removed {
                id: app.id.clone(),
                name: app.name.clone(),
                leftovers: leftovers.iter().map(LeftoverRow::from).collect(),
                family: *family,
                complete: *complete,
            },
        }
    }
}

/// Le contenu direct d'un dossier.
fn content_of(host: &dyn AppsHost, location: &str) -> Content {
    if !host.is_local_fixed(location) {
        return Content::Unknown;
    }
    match host.existence(location) {
        crate::apps::host::Existence::Absent => Content::Absent,
        crate::apps::host::Existence::Folder => match host.children(location) {
            Ok(children) => {
                let mut names: Vec<String> =
                    children.iter().map(|child| child.name.to_lowercase()).collect();
                names.sort();
                Content::Names(names)
            }
            Err(_) => Content::Unknown,
        },
        _ => Content::Unknown,
    }
}
