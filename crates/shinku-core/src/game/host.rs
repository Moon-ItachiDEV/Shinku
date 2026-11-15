//! La machine, vue par le mode jeu.
//!
//! Comme [`crate::platform::Environment`] pour le moteur d'analyse, le trait
//! [`GameHost`] est tout ce que la machine à états sait de Windows : le
//! premier plan, les processus, le mode d'alimentation. La vraie
//! implémentation est dans `platform/windows/game.rs`. Les tests utilisent
//! [`FakeHost`], une machine fabriquée de toutes pièces qui note chaque geste.

use std::fmt;
use std::io;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::{Serialize, Serializer};

use crate::game::detect::file_name;
use crate::model::Timestamp;

/// Un processus. Un pid se recycle : c'est la date de création qui fait l'identité.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcessRef {
    /// Numéro du processus.
    pub pid: u32,
    /// Date de création, en `FILETIME` (100 ns depuis 1601).
    pub started: i64,
}

/// La fenêtre au premier plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Foreground {
    /// Son processus.
    pub process: ProcessRef,
    /// Le chemin complet de son exécutable (`C:\…`).
    pub exe: String,
}

/// Un processus de la session Windows courante.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningProcess {
    /// Le processus.
    pub process: ProcessRef,
    /// Son parent.
    pub parent: u32,
    /// Le nom de son exécutable, en minuscules.
    pub exe_name: String,
}

/// Ce que Shinku sait changer sur un processus, et lit avant de le changer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessState {
    /// Classe de priorité (`NORMAL_PRIORITY_CLASS` = 0x20…).
    pub priority: u32,
    /// Masque de contrôle du mode efficacité (0 ou 1) : 1 si le mode est
    /// imposé, 0 si Windows le gère.
    pub throttle_control: u32,
    /// Masque d'état du mode efficacité (0 ou 1) : actif ou non, quand il est imposé.
    pub throttle_state: u32,
}

/// Les classes de priorité de Windows que Shinku connaît. Jamais « temps réel ».
pub mod priority {
    /// `IDLE_PRIORITY_CLASS`.
    pub const IDLE: u32 = 0x40;
    /// `BELOW_NORMAL_PRIORITY_CLASS`.
    pub const BELOW_NORMAL: u32 = 0x4000;
    /// `NORMAL_PRIORITY_CLASS`.
    pub const NORMAL: u32 = 0x20;
    /// `ABOVE_NORMAL_PRIORITY_CLASS`.
    pub const ABOVE_NORMAL: u32 = 0x8000;
    /// `HIGH_PRIORITY_CLASS`.
    pub const HIGH: u32 = 0x80;

    /// Vrai si Shinku connaît cette classe.
    pub fn is_known(class: u32) -> bool {
        matches!(class, IDLE | BELOW_NORMAL | NORMAL | ABOVE_NORMAL | HIGH)
    }
}

/// Un mode d'alimentation de Windows : un GUID.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerPlan(u128);

impl PowerPlan {
    /// « Utilisation normale ».
    pub const BALANCED: Self = Self(0x381b4222_f694_41f0_9685_ff5bb260df2e);
    /// « Économie d'énergie ».
    pub const POWER_SAVER: Self = Self(0xa1841308_3541_4fab_bc81_f71556f20b4a);
    /// « Performances élevées ».
    pub const HIGH_PERFORMANCE: Self = Self(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);

    /// Le GUID sous forme d'entier de 128 bits.
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Le GUID sous forme d'entier de 128 bits.
    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

impl fmt::Display for PowerPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (v >> 96) as u32,
            (v >> 80) as u16,
            (v >> 64) as u16,
            (v >> 48) as u16,
            v & 0xffff_ffff_ffff
        )
    }
}

impl fmt::Debug for PowerPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PowerPlan({self})")
    }
}

/// Un texte qui n'est pas un GUID au format canonique.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotAGuid;

impl fmt::Display for NotAGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ce n'est pas un GUID au format 8-4-4-4-12")
    }
}

impl std::error::Error for NotAGuid {}

impl FromStr for PowerPlan {
    type Err = NotAGuid;

    /// Lit un GUID au format canonique, sans accolades : 8-4-4-4-12 chiffres
    /// hexadécimaux, majuscules ou minuscules.
    fn from_str(text: &str) -> Result<Self, NotAGuid> {
        let parts: Vec<&str> = text.split('-').collect();
        let lengths = [8, 4, 4, 4, 12];
        if parts.len() != lengths.len()
            || parts.iter().zip(lengths).any(|(part, len)| part.len() != len)
            || !text.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
        {
            return Err(NotAGuid);
        }
        u128::from_str_radix(&parts.concat(), 16).map(Self).map_err(|_| NotAGuid)
    }
}

impl Serialize for PowerPlan {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// La machine, vue par le mode jeu. Ne modifie que deux choses : le mode
/// d'alimentation, et la priorité et le mode efficacité d'un processus.
pub trait GameHost: Send + Sync + fmt::Debug {
    /// L'instant présent.
    fn now(&self) -> Timestamp;

    /// La fenêtre au premier plan, si on peut connaître son processus.
    fn foreground(&self) -> Option<Foreground>;

    /// Les processus de la session Windows courante. Ceux qu'on ne peut pas
    /// ouvrir n'y sont pas.
    fn processes(&self) -> Vec<RunningProcess>;

    /// Le chemin complet de l'exécutable de ce processus, si on peut l'ouvrir.
    /// `None` aussi si le pid appartient désormais à un autre processus.
    fn exe_path(&self, process: ProcessRef) -> Option<String>;

    /// Le nom à montrer pour cet exécutable : la description du fichier, sinon
    /// son nom sans `.exe`.
    fn display_name(&self, exe: &str) -> String;

    /// Vrai si le PC fonctionne sur batterie.
    fn on_battery(&self) -> bool;

    /// Le mode d'alimentation actif.
    fn power_plan(&self) -> Option<PowerPlan>;

    /// Vrai si ce mode existe sur ce PC.
    fn has_power_plan(&self, plan: PowerPlan) -> bool;

    /// Active ce mode d'alimentation.
    fn set_power_plan(&self, plan: PowerPlan) -> io::Result<()>;

    /// Le nom du processeur (« AMD Ryzen 9 7950X3D 16-Core Processor »).
    fn processor_name(&self) -> String;

    /// Lit l'état d'un processus, sans rien changer. `None` s'il n'existe plus
    /// (ou si son pid a été repris par un autre) ou s'il ne s'ouvre pas.
    fn inspect(&self, process: ProcessRef) -> Option<ProcessState>;

    /// Met un processus dans cet état. Ne change que ce qui diffère de son
    /// état actuel.
    fn apply(&self, process: ProcessRef, state: ProcessState) -> io::Result<()>;
}

/// Une machine qui ne voit rien : rien au premier plan, aucun processus,
/// aucun mode d'alimentation. C'est celle des plateformes sans mode jeu.
#[derive(Clone, Copy, Debug, Default)]
pub struct InertHost;

impl GameHost for InertHost {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn foreground(&self) -> Option<Foreground> {
        None
    }

    fn processes(&self) -> Vec<RunningProcess> {
        Vec::new()
    }

    fn exe_path(&self, _process: ProcessRef) -> Option<String> {
        None
    }

    fn display_name(&self, exe: &str) -> String {
        stem(exe)
    }

    fn on_battery(&self) -> bool {
        false
    }

    fn power_plan(&self) -> Option<PowerPlan> {
        None
    }

    fn has_power_plan(&self, _plan: PowerPlan) -> bool {
        false
    }

    fn set_power_plan(&self, _plan: PowerPlan) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pas de mode d'alimentation ici"))
    }

    fn processor_name(&self) -> String {
        String::new()
    }

    fn inspect(&self, _process: ProcessRef) -> Option<ProcessState> {
        None
    }

    fn apply(&self, _process: ProcessRef, _state: ProcessState) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "pas de processus ici"))
    }
}

/// Le nom d'un fichier sans son `.exe`, avec la casse du chemin.
pub fn stem(exe: &str) -> String {
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    let cut = name.len().checked_sub(4).filter(|&at| name.is_char_boundary(at));
    match cut {
        Some(at) if name[at..].eq_ignore_ascii_case(".exe") => name[..at].to_owned(),
        _ => name.to_owned(),
    }
}

/// Le `FILETIME` de 1970 : ce qu'il faut ajouter à un [`Timestamp`] pour avoir
/// la date de création d'un processus.
const FILETIME_OF_UNIX_EPOCH: i64 = 116_444_736_000_000_000;

/// Un geste de Shinku sur la machine fabriquée, dans l'ordre où il est fait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// Changement du mode d'alimentation.
    SetPowerPlan(PowerPlan),
    /// Changement d'un processus.
    Apply {
        /// Le processus.
        process: ProcessRef,
        /// L'état demandé.
        state: ProcessState,
    },
}

#[derive(Debug)]
struct FakeProcess {
    process: ProcessRef,
    parent: u32,
    path: String,
    state: ProcessState,
    readable: bool,
    writable: bool,
}

#[derive(Debug)]
struct Fake {
    now: Timestamp,
    created: i64,
    foreground: Option<ProcessRef>,
    processes: Vec<FakeProcess>,
    on_battery: bool,
    active: Option<PowerPlan>,
    plans: Vec<PowerPlan>,
    processor: String,
    power_refused: bool,
    gestures: Vec<Gesture>,
    opened: Vec<u32>,
    name_reads: u32,
    spy: Option<std::path::PathBuf>,
    spied: Vec<Option<String>>,
}

/// Une machine fabriquée de toutes pièces, pour les tests (et pour essayer le
/// guichet sans Windows). Ses copies partagent la même machine : le test garde
/// une copie, la machine à états en reçoit une autre.
///
/// Elle note chaque geste de Shinku ([`FakeHost::gestures`]), peut refuser le
/// changement d'alimentation, et peut « espionner » un fichier : à chaque
/// geste, elle en relit le contenu, pour prouver qu'il a été écrit avant d'agir.
#[derive(Clone, Debug)]
pub struct FakeHost {
    inner: Arc<Mutex<Fake>>,
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHost {
    /// Un PC de bureau ordinaire : sur secteur, en « Utilisation normale », avec
    /// les trois modes d'alimentation, sans aucune fenêtre ni processus.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Fake {
                now: Timestamp::from_unix(1_800_000_000, 0),
                created: 0,
                foreground: None,
                processes: Vec::new(),
                on_battery: false,
                active: Some(PowerPlan::BALANCED),
                plans: vec![
                    PowerPlan::BALANCED,
                    PowerPlan::POWER_SAVER,
                    PowerPlan::HIGH_PERFORMANCE,
                ],
                processor: "Intel(R) Core(TM) i7-13700K".to_owned(),
                power_refused: false,
                gestures: Vec::new(),
                opened: Vec::new(),
                name_reads: 0,
                spy: None,
                spied: Vec::new(),
            })),
        }
    }

    fn state(&self) -> MutexGuard<'_, Fake> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Fait avancer l'horloge.
    pub fn advance(&self, duration: Duration) {
        let mut fake = self.state();
        fake.now = Timestamp::from_ticks(
            fake.now.ticks() + i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX),
        );
    }

    /// Lance un processus de ce chemin. Les processus sont créés dans l'ordre
    /// de ces appels, jamais deux à la même date.
    pub fn spawn(&self, pid: u32, parent: u32, path: &str) -> ProcessRef {
        let mut fake = self.state();
        fake.created += 1;
        let process =
            ProcessRef { pid, started: fake.now.ticks() + FILETIME_OF_UNIX_EPOCH + fake.created };
        fake.processes.push(FakeProcess {
            process,
            parent,
            path: path.to_owned(),
            state: ProcessState {
                priority: priority::NORMAL,
                throttle_control: 0,
                throttle_state: 0,
            },
            readable: true,
            writable: true,
        });
        process
    }

    /// Termine un processus.
    pub fn kill(&self, process: ProcessRef) {
        let mut fake = self.state();
        fake.processes.retain(|known| known.process != process);
        if fake.foreground == Some(process) {
            fake.foreground = None;
        }
    }

    /// Met un processus au premier plan, ou personne.
    pub fn set_foreground(&self, process: Option<ProcessRef>) {
        self.state().foreground = process;
    }

    /// Fixe l'état d'un processus, comme le ferait un autre programme.
    pub fn set_state(&self, process: ProcessRef, state: ProcessState) {
        if let Some(known) = self.state().processes.iter_mut().find(|p| p.process == process) {
            known.state = state;
        }
    }

    /// L'état d'un processus qui vit.
    pub fn state_of(&self, process: ProcessRef) -> Option<ProcessState> {
        self.state().processes.iter().find(|p| p.process == process).map(|p| p.state)
    }

    /// Un processus qu'on ne peut ni lire ni modifier, comme celui d'un jeu
    /// protégé par un anti-triche : il n'est pas non plus listé.
    pub fn set_readable(&self, process: ProcessRef, readable: bool) {
        if let Some(known) = self.state().processes.iter_mut().find(|p| p.process == process) {
            known.readable = readable;
        }
    }

    /// Un processus qu'on peut lire mais pas modifier.
    pub fn set_writable(&self, process: ProcessRef, writable: bool) {
        if let Some(known) = self.state().processes.iter_mut().find(|p| p.process == process) {
            known.writable = writable;
        }
    }

    /// Fixe le fonctionnement sur batterie.
    pub fn set_on_battery(&self, on_battery: bool) {
        self.state().on_battery = on_battery;
    }

    /// Fixe le mode d'alimentation actif, comme le ferait la personne.
    pub fn set_active_plan(&self, plan: Option<PowerPlan>) {
        self.state().active = plan;
    }

    /// Le mode d'alimentation actif.
    pub fn active_plan(&self) -> Option<PowerPlan> {
        self.state().active
    }

    /// Retire un mode d'alimentation de ce PC.
    pub fn remove_plan(&self, plan: PowerPlan) {
        self.state().plans.retain(|known| *known != plan);
    }

    /// Fixe le nom du processeur.
    pub fn set_processor(&self, name: &str) {
        name.clone_into(&mut self.state().processor);
    }

    /// Windows refuse (ou accepte à nouveau) de changer le mode d'alimentation.
    pub fn refuse_power_changes(&self, refused: bool) {
        self.state().power_refused = refused;
    }

    /// Les gestes faits sur la machine, dans l'ordre.
    pub fn gestures(&self) -> Vec<Gesture> {
        self.state().gestures.clone()
    }

    /// Oublie les gestes notés jusqu'ici.
    pub fn clear_gestures(&self) {
        self.state().gestures.clear();
    }

    /// Les pid dont l'exécutable a été demandé (donc ouverts), dans l'ordre.
    pub fn opened(&self) -> Vec<u32> {
        self.state().opened.clone()
    }

    /// Nombre de fois où un nom à montrer a été demandé.
    pub fn name_reads(&self) -> u32 {
        self.state().name_reads
    }

    /// Relit ce fichier à chaque geste de Shinku.
    pub fn spy_on(&self, path: impl Into<std::path::PathBuf>) {
        self.state().spy = Some(path.into());
    }

    /// Le contenu du fichier espionné, vu juste avant chaque geste.
    pub fn spied(&self) -> Vec<Option<String>> {
        self.state().spied.clone()
    }
}

impl Fake {
    fn note(&mut self, gesture: Gesture) {
        if let Some(path) = &self.spy {
            self.spied.push(std::fs::read_to_string(path).ok());
        }
        self.gestures.push(gesture);
    }
}

impl GameHost for FakeHost {
    fn now(&self) -> Timestamp {
        self.state().now
    }

    fn foreground(&self) -> Option<Foreground> {
        let fake = self.state();
        let process = fake.foreground?;
        let known = fake.processes.iter().find(|p| p.process == process && p.readable)?;
        Some(Foreground { process, exe: known.path.clone() })
    }

    fn processes(&self) -> Vec<RunningProcess> {
        self.state()
            .processes
            .iter()
            .filter(|p| p.readable)
            .map(|p| RunningProcess {
                process: p.process,
                parent: p.parent,
                exe_name: file_name(&p.path),
            })
            .collect()
    }

    fn exe_path(&self, process: ProcessRef) -> Option<String> {
        let mut fake = self.state();
        fake.opened.push(process.pid);
        fake.processes.iter().find(|p| p.process == process && p.readable).map(|p| p.path.clone())
    }

    fn display_name(&self, exe: &str) -> String {
        self.state().name_reads += 1;
        stem(exe)
    }

    fn on_battery(&self) -> bool {
        self.state().on_battery
    }

    fn power_plan(&self) -> Option<PowerPlan> {
        self.state().active
    }

    fn has_power_plan(&self, plan: PowerPlan) -> bool {
        self.state().plans.contains(&plan)
    }

    fn set_power_plan(&self, plan: PowerPlan) -> io::Result<()> {
        let mut fake = self.state();
        if fake.power_refused || !fake.plans.contains(&plan) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refusé"));
        }
        fake.note(Gesture::SetPowerPlan(plan));
        fake.active = Some(plan);
        Ok(())
    }

    fn processor_name(&self) -> String {
        self.state().processor.clone()
    }

    fn inspect(&self, process: ProcessRef) -> Option<ProcessState> {
        self.state().processes.iter().find(|p| p.process == process && p.readable).map(|p| p.state)
    }

    fn apply(&self, process: ProcessRef, state: ProcessState) -> io::Result<()> {
        let mut fake = self.state();
        let Some(index) = fake.processes.iter().position(|p| p.process == process) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "ce processus n'existe plus"));
        };
        if !fake.processes[index].readable || !fake.processes[index].writable {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "accès refusé"));
        }
        fake.note(Gesture::Apply { process, state });
        fake.processes[index].state = state;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_power_plan_goes_through_text_and_back() {
        let text = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
        let plan: PowerPlan = text.parse().unwrap();
        assert_eq!(plan, PowerPlan::HIGH_PERFORMANCE);
        assert_eq!(plan.to_string(), text);
        assert_eq!(PowerPlan::BALANCED.to_string(), "381b4222-f694-41f0-9685-ff5bb260df2e");
        assert_eq!(PowerPlan::POWER_SAVER.to_string(), "a1841308-3541-4fab-bc81-f71556f20b4a");
        // Les majuscules sont lues, et rendues en minuscules.
        let upper: PowerPlan = text.to_uppercase().parse().unwrap();
        assert_eq!(upper.to_string(), text);
        assert_eq!(serde_json::to_value(plan).unwrap(), text);
    }

    #[test]
    fn only_canonical_guids_are_read() {
        for bad in [
            "",
            "8c5e7fda",
            "{8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c}",
            "8c5e7fdae8bf4a969a85a6e23a8c635c",
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635",
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635cc",
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635g",
            "+c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c\n",
        ] {
            assert_eq!(bad.parse::<PowerPlan>(), Err(NotAGuid), "{bad:?}");
        }
    }

    #[test]
    fn a_stem_drops_only_the_exe_extension() {
        assert_eq!(stem(r"C:\Jeux\Hollow Peaks.EXE"), "Hollow Peaks");
        assert_eq!(stem("a/b/hades.exe"), "hades");
        assert_eq!(stem("script.bat"), "script.bat");
        assert_eq!(stem(".exe"), "");
        assert_eq!(stem(""), "");
        assert_eq!(stem("é.exe"), "é");
    }

    #[test]
    fn the_fake_machine_keeps_the_order_of_what_it_is_asked() {
        let host = FakeHost::new();
        let parent = host.spawn(10, 1, r"C:\Jeux\parent.exe");
        host.advance(Duration::from_secs(1));
        let child = host.spawn(11, 10, r"C:\Jeux\enfant.exe");
        assert!(parent.started < child.started);
        host.set_foreground(Some(child));
        assert_eq!(host.foreground().unwrap().exe, r"C:\Jeux\enfant.exe");
        let names: Vec<_> = host.processes().into_iter().map(|p| p.exe_name).collect();
        assert_eq!(names, ["parent.exe", "enfant.exe"]);

        let low = ProcessState {
            priority: priority::BELOW_NORMAL,
            throttle_control: 1,
            throttle_state: 1,
        };
        host.apply(child, low).unwrap();
        assert_eq!(host.inspect(child), Some(low));
        host.set_writable(child, false);
        assert!(host.apply(child, low).is_err());
        host.set_readable(child, false);
        assert_eq!(host.inspect(child), None);
        assert_eq!(host.foreground(), None);
        host.kill(parent);
        assert_eq!(host.processes().len(), 0, "le processus illisible n'est pas listé");
        assert_eq!(
            host.gestures(),
            [Gesture::Apply { process: child, state: low }],
            "un geste refusé n'est pas noté"
        );

        host.set_on_battery(true);
        assert!(host.on_battery());
        host.set_power_plan(PowerPlan::HIGH_PERFORMANCE).unwrap();
        assert_eq!(host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
        host.refuse_power_changes(true);
        assert!(host.set_power_plan(PowerPlan::BALANCED).is_err());
        assert_eq!(host.active_plan(), Some(PowerPlan::HIGH_PERFORMANCE));
    }

    #[test]
    fn the_inert_machine_sees_nothing() {
        let host = InertHost;
        assert_eq!(host.foreground(), None);
        assert!(host.processes().is_empty());
        assert!(!host.has_power_plan(PowerPlan::HIGH_PERFORMANCE));
        assert!(host.set_power_plan(PowerPlan::BALANCED).is_err());
        assert_eq!(host.display_name(r"C:\Jeux\Hades.exe"), "Hades");
    }
}
