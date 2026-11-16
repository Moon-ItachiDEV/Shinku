//! Le guichet du module « Confidentialité » : ce que la fenêtre peut lire et demander.
//!
//! [`PrivacyDesk`] garde la machine et rien d'autre : **aucune liste n'est
//! gardée** entre deux appels, la machine peut avoir changé (un navigateur
//! qui s'ouvre, un profil recréé). La vue se refait à chaque demande, et
//! l'effacement refait la liste avant de toucher à quoi que ce soit.
//!
//! Le verrou d'exclusivité n'est pas ici : il se prend au niveau du guichet
//! principal ([`crate::backend::Backend`]), le même que celui de
//! [`crate::apps::desk::AppsDesk::try_exclusive`], pour qu'un effacement et un
//! grand vide (ou une mise à la corbeille de restes) se refusent l'un l'autre.
//! **Aucun journal n'est écrit**, rien n'est retenu sur disque.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::fs::{
    DirReader, EntryMeta, Identity, PathState, fold_name, identity, inspect_dir_chain,
};
use crate::privacy::erase::{self, Gate, LotFile, erasable, erase_lot, open_real};
use crate::privacy::host::PrivacyHost;
use crate::privacy::id::{TraceId, name_stamp};
use crate::privacy::names::read_display_names;
use crate::privacy::rules::{self, Browser, Slot, TypedKey};
use crate::privacy::view::{
    ErasedTraces, GroupKind, PrivacyGroup, PrivacyRefusal, PrivacyView, TraceItem, TraceKind,
    TraceOutcome, TraceResult, TraceState,
};

/// Nombre d'identifiants d'une demande, au plus : au-delà, tout est refusé.
pub const MAX_IDS: usize = 64;

/// Un profil trouvé dans `User Data`.
#[derive(Clone, Debug)]
struct Profile {
    slot: Slot,
    path: PathBuf,
    identity: Identity,
    /// Le nom affiché, nettoyé ; `None` si `Local State` n'en donne pas de lisible.
    name: Option<String>,
}

/// Ce que la liste de `User Data` donne.
#[derive(Debug)]
enum UserData {
    /// Pas de dossier : le navigateur n'est pas là.
    Absent,
    /// Le dossier existe mais Shinku n'y entre pas (un lien sur la chaîne, ou
    /// une erreur de lecture) : pas de bloc.
    Skipped,
    /// Le dossier et ses profils.
    Found { path: PathBuf, profiles: Vec<Profile> },
}

/// Ce que la liste d'un profil donne pour la trace `history`.
enum LotView {
    /// Le dossier ne s'est pas lu.
    Unreadable,
    /// Rien à effacer.
    Empty,
    /// Quelque chose du lot n'est pas un fichier ordinaire (lien, dossier, « en ligne uniquement »).
    Irregular,
    /// Les fichiers du lot, dans l'ordre de la règle.
    Files(Vec<(String, EntryMeta)>),
}

/// Le module « Confidentialité », vu du guichet.
#[derive(Debug)]
pub struct PrivacyDesk {
    host: Arc<dyn PrivacyHost>,
    /// « On ferme » : levé par [`Self::close`], jamais baissé. Aucune trace ne démarre ensuite.
    closing: AtomicBool,
    /// Tenu pendant la section critique d'un lot (de la revérification du
    /// navigateur au dernier effacement) : [`Self::close`] l'attend.
    critical: Mutex<()>,
}

/// Vrai si ce programme tourne. Une machine qui ne sait pas dire ce qui tourne
/// est traitée comme si tout était ouvert (le sens prudent).
fn is_running(processes: Option<&[String]>, exe: &str) -> bool {
    processes.is_none_or(|list| list.iter().any(|name| name.eq_ignore_ascii_case(exe)))
}

impl PrivacyDesk {
    /// Un guichet sur cette machine.
    pub fn new(host: Arc<dyn PrivacyHost>) -> Self {
        Self { host, closing: AtomicBool::new(false), critical: Mutex::new(()) }
    }

    /// Shinku se ferme : plus aucune trace ne démarre, et **le lot en cours, s'il y en a un,
    /// va jusqu'au bout** avant que cet appel ne rende la main. La section est
    /// courte (quelques appels système sur des fichiers déjà ouverts), donc la fermeture
    /// n'attend rien d'autre. Appelé par `Backend::shutdown`.
    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        drop(self.critical.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    /// La section critique d'un lot : refusée (le corps ne s'exécute pas) si Shinku se ferme.
    /// Le drapeau est lu **sous le verrou** : `close` lève le drapeau puis attend le verrou,
    /// donc un lot soit finit avant la fermeture, soit ne commence pas.
    fn critical_section(&self, body: &mut dyn FnMut() -> TraceOutcome) -> TraceOutcome {
        let _held = self.critical.lock().unwrap_or_else(PoisonError::into_inner);
        if self.is_closing() {
            return TraceOutcome::Failed;
        }
        body()
    }

    fn user_data_path(&self, browser: Browser) -> Option<PathBuf> {
        let mut path = self.host.local_app_data()?;
        for part in browser.user_data() {
            path.push(part);
        }
        Some(path)
    }

    /// Le navigateur est ouvert : un processus de son nom (toutes sessions), **ou**
    /// le verrou de `User Data` tenu. Un navigateur qui tourne sans fenêtre,
    /// en arrière-plan, se voit par l'un ou l'autre.
    fn browser_running(&self, browser: Browser, user_data: &std::path::Path) -> bool {
        is_running(self.host.running_processes().as_deref(), browser.exe())
            || self.host.lockfile_held(user_data)
    }

    /// Liste `User Data` : les profils, par leur nom compilé et exact, avec leur identité
    /// et leur nom affiché. La chaîne de dossiers est revérifiée à chaque appel.
    fn scan(&self, browser: Browser) -> UserData {
        let Some(path) = self.user_data_path(browser) else { return UserData::Absent };
        match inspect_dir_chain(&path) {
            Ok(PathState::RealDir) => {}
            Ok(PathState::Missing) => return UserData::Absent,
            Ok(PathState::NotPlain) | Err(_) => return UserData::Skipped,
        }
        let mut entries: Vec<(OsString, EntryMeta)> = Vec::new();
        let listed = DirReader::new().read(&path, None, &mut |name, meta| {
            entries.push((name.to_owned(), *meta));
        });
        if listed.is_err() {
            return UserData::Skipped;
        }
        let candidates: Vec<(Slot, String)> = entries
            .iter()
            .filter(|(_, meta)| meta.is_walkable_dir())
            .filter_map(|(name, _)| {
                let text = name.to_str()?;
                Some((Slot::from_folder(text)?, text.to_owned()))
            })
            .collect();
        let folders: Vec<String> = candidates.iter().map(|(_, folder)| folder.clone()).collect();
        // `Local State` : lu pour le nom des profils, jamais écrit ni effacé.
        let names = entries
            .iter()
            .find(|(name, _)| fold_name(name) == fold_name(OsStr::new("Local State")))
            .map(|(name, meta)| read_display_names(&path.join(name), meta, &folders))
            .unwrap_or_default();
        let mut profiles: Vec<Profile> = candidates
            .into_iter()
            .filter_map(|(slot, folder)| {
                let profile_path = path.join(&folder);
                // Sans identité (système de fichiers qui n'en donne pas), le profil
                // ne pourrait pas être revérifié à l'effacement : il n'est pas proposé.
                let identity = identity(&profile_path).ok().flatten()?;
                Some(Profile {
                    slot,
                    name: names.get(&folder).cloned(),
                    path: profile_path,
                    identity,
                })
            })
            .collect();
        profiles.sort_by_key(|profile| profile.slot);
        UserData::Found { path, profiles }
    }

    /// Les fichiers de la trace `history` d'un profil, d'après la liste de son dossier.
    fn read_lot(&self, browser: Browser, profile: &Profile) -> LotView {
        let lot = rules::history_lot(browser);
        let mut found: Vec<(usize, String, EntryMeta)> = Vec::new();
        let listed = DirReader::new().read(
            &profile.path,
            Some(profile.identity.file_id),
            &mut |name, meta| {
                if let Some(text) = name.to_str() {
                    // Comparé **exactement** au nom de la règle.
                    if let Some(at) = lot.iter().position(|wanted| wanted == text) {
                        found.push((at, text.to_owned(), *meta));
                    }
                }
            },
        );
        if listed.is_err() {
            return LotView::Unreadable;
        }
        if found.is_empty() {
            return LotView::Empty;
        }
        found.sort_by_key(|(at, _, _)| *at);
        if found.iter().any(|(_, _, meta)| !erasable(meta)) {
            return LotView::Irregular;
        }
        LotView::Files(found.into_iter().map(|(_, name, meta)| (name, meta)).collect())
    }

    fn history_item(&self, browser: Browser, profile: &Profile) -> TraceItem {
        let id = TraceId::History {
            browser,
            slot: profile.slot,
            identity: profile.identity,
            name: name_stamp(profile.name.as_deref()),
        };
        // Le poids : la somme des tailles lues dans la liste, sans rien ouvrir.
        let (state, bytes) = match self.read_lot(browser, profile) {
            LotView::Unreadable | LotView::Irregular => (TraceState::Unreadable, None),
            LotView::Empty => (TraceState::Empty, Some(0)),
            LotView::Files(files) => {
                (TraceState::Present, Some(files.iter().map(|(_, meta)| meta.size).sum()))
            }
        };
        TraceItem { id, kind: TraceKind::History, state, bytes, count: None }
    }

    fn windows_group(&self) -> PrivacyGroup {
        let recent = match self.host.recent_count() {
            Ok(0) => (TraceState::Empty, Some(0)),
            Ok(count) => (TraceState::Present, Some(count)),
            Err(_) => (TraceState::Unreadable, None),
        };
        let (mut values, mut complete, mut readable) = (0usize, true, true);
        for key in TypedKey::ALL {
            match self.host.registry_values(key) {
                Ok(None) => {}
                Ok(Some(found)) => {
                    // Les valeurs d'index (`MRUList`, `MRUListEx`) et le nom vide ne sont
                    // pas des saisies : ils ne comptent pas.
                    values += found
                        .names
                        .iter()
                        .filter(|name| !name.is_index_of(key) && !name.0.is_empty())
                        .count();
                    complete &= found.complete;
                }
                Err(_) => readable = false,
            }
        }
        let count = u32::try_from(values).unwrap_or(u32::MAX);
        let typed = if !readable {
            (TraceState::Unreadable, None)
        } else if values > 0 || !complete {
            (TraceState::Present, Some(count))
        } else {
            (TraceState::Empty, Some(0))
        };
        // Présent si le contenu actuel est plein OU si l'historique est activé (Shinku
        // ne lit pas l'historique : activé, il est proposé). Vide : les deux le sont.
        let clipboard = match (self.host.clipboard_filled(), self.host.clipboard_history_enabled())
        {
            (Ok(filled), Ok(history)) => {
                if filled || history {
                    TraceState::Present
                } else {
                    TraceState::Empty
                }
            }
            _ => TraceState::Unreadable,
        };
        let item = |id, kind, (state, count): (TraceState, Option<u32>)| TraceItem {
            id,
            kind,
            state,
            bytes: None,
            count,
        };
        PrivacyGroup {
            key: "windows".to_owned(),
            kind: GroupKind::Windows,
            app: None,
            profile: None,
            slot: None,
            running: false,
            items: vec![
                item(TraceId::Recent, TraceKind::Recent, recent),
                item(TraceId::Typed, TraceKind::Typed, typed),
                item(TraceId::Clipboard, TraceKind::Clipboard, (clipboard, None)),
            ],
        }
    }

    /// Tout ce que la page affiche. Refait à chaque demande ; ne touche à rien.
    pub fn view(&self) -> PrivacyView {
        let mut groups = Vec::new();
        let mut has = [false; 2];
        for browser in Browser::ALL {
            let UserData::Found { path, profiles } = self.scan(browser) else { continue };
            if profiles.is_empty() {
                continue;
            }
            match browser {
                Browser::Chrome => has[0] = true,
                Browser::Edge => has[1] = true,
                Browser::Brave | Browser::Vivaldi => {}
            }
            let running = self.browser_running(browser, &path);
            for profile in &profiles {
                groups.push(PrivacyGroup {
                    key: format!("{}:{}", browser.name(), profile.slot.number()),
                    kind: GroupKind::Browser,
                    app: Some(browser),
                    profile: profile.name.clone(),
                    slot: Some(profile.slot.number()),
                    running,
                    items: vec![self.history_item(browser, profile)],
                });
            }
        }
        groups.push(self.windows_group());
        PrivacyView {
            groups,
            firefox: self.host.firefox_present(),
            edge_imports_chrome: has[0] && has[1],
        }
    }

    /// Efface les traces de ces identifiants, une fois chacune, dans l'ordre. Plus de
    /// [`MAX_IDS`] identifiants : tout est refusé, rien n'est touché. Un identifiant
    /// que le cœur n'aurait pas fabriqué (une vue périmée) donne `changed` sans rien toucher : la
    /// page regarde de nouveau, les autres traces sont traitées.
    ///
    /// L'appelant tient l'exclusivité (voir le guichet principal). `owner` est la
    /// fenêtre de Shinku, pour le presse-papiers.
    pub fn erase(&self, ids: &[String], owner: isize) -> Result<ErasedTraces, PrivacyRefusal> {
        if ids.len() > MAX_IDS {
            return Err(PrivacyRefusal::TooMany);
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let mut results = Vec::new();
        for text in ids {
            if !seen.insert(text.as_str()) {
                continue;
            }
            let outcome = match TraceId::parse(text) {
                None => TraceOutcome::Changed,
                // Shinku se ferme : rien ne démarre plus (la fenêtre ne verra pas la réponse).
                Some(_) if self.is_closing() => TraceOutcome::Failed,
                Some(id) => self.erase_one(id, owner),
            };
            results.push(TraceResult { id: text.clone(), outcome });
        }
        Ok(ErasedTraces { view: self.view(), results })
    }

    fn erase_one(&self, id: TraceId, owner: isize) -> TraceOutcome {
        match id {
            TraceId::History { browser, slot, identity, name } => {
                self.erase_history(browser, slot, identity, name)
            }
            TraceId::Recent => erase::erase_recent(self.host.as_ref()),
            TraceId::Typed => erase::erase_typed(self.host.as_ref()),
            TraceId::Clipboard => erase::erase_clipboard(self.host.as_ref(), owner),
        }
    }

    /// Les quatre étapes de la conception, dans l'ordre. La liste est refaite ici,
    /// le dossier retrouvé par son nom compilé : même identité et même nom
    /// affiché que dans la vue, sinon `changed`.
    fn erase_history(
        &self,
        browser: Browser,
        slot: Slot,
        expected: Identity,
        stamp: u64,
    ) -> TraceOutcome {
        let Some(user_data) = self.user_data_path(browser) else { return TraceOutcome::Changed };
        // 1. La chaîne des dossiers du profil, revérifiée **à l'effacement**.
        match inspect_dir_chain(&user_data.join(slot.folder_name())) {
            Ok(PathState::RealDir) => {}
            Ok(PathState::Missing) => return TraceOutcome::Changed,
            Ok(PathState::NotPlain) | Err(_) => return TraceOutcome::Failed,
        }
        let UserData::Found { profiles, .. } = self.scan(browser) else {
            return TraceOutcome::Changed;
        };
        let Some(profile) = profiles.into_iter().find(|profile| profile.slot == slot) else {
            return TraceOutcome::Changed;
        };
        if profile.identity != expected || name_stamp(profile.name.as_deref()) != stamp {
            return TraceOutcome::Changed;
        }
        if self.browser_running(browser, &user_data) {
            return TraceOutcome::InUse;
        }
        let files = match self.read_lot(browser, &profile) {
            LotView::Empty => return TraceOutcome::Nothing,
            LotView::Unreadable | LotView::Irregular => return TraceOutcome::Failed,
            LotView::Files(files) => files,
        };
        let lot: Vec<LotFile> = files
            .into_iter()
            .map(|(name, meta)| LotFile {
                path: profile.path.join(&name),
                name,
                expected: meta.expected(),
            })
            .collect();
        // 2 à 4 : ouvrir tout le lot, revérifier le navigateur fermé et le dossier
        // inchangé poignées tenues, tout marquer, tout confirmer.
        let names: Vec<&str> = lot.iter().map(|file| file.name.as_str()).collect();
        let closed = || !self.browser_running(browser, &user_data);
        // Les fichiers de la règle présents sont **exactement** ceux qui sont ouverts.
        let same_lot = || match self.read_lot(browser, &profile) {
            LotView::Files(now) => {
                now.len() == names.len()
                    && now.iter().zip(&names).all(|((name, _), wanted)| name == wanted)
            }
            _ => false,
        };
        let critical = |body: &mut dyn FnMut() -> TraceOutcome| self.critical_section(body);
        erase_lot(
            &lot,
            &open_real,
            &Gate {
                closed: &closed,
                same_lot: &same_lot,
                critical: &critical,
                before_mark: &|_| Ok(()),
            },
        )
    }
}
