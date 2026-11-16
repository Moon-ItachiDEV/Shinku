//! Une machine fabriquée de toutes pièces pour les essais de la « Confidentialité ».
//!
//! Les fichiers, eux, sont de vrais fichiers d'un dossier temporaire : c'est le
//! cœur qui les liste, les ouvre et les supprime, comme sur le vrai disque. Ce
//! que ce faux hôte joue, c'est le reste : les processus, le verrou d'un
//! profil, le registre (avec ses clés liens et ses valeurs qui reviennent),
//! les raccourcis récents et le presse-papiers.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::privacy::host::{
    ClipboardClear, CurrentClear, HistoryClear, PrivacyHost, RegistryError, RegistryValues,
};
use crate::privacy::rules::{TypedKey, ValueName};

/// Ce que `SHAddToRecentDocs(NULL)` fait des raccourcis, dans l'essai.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentClear {
    /// Il les efface tous.
    All,
    /// Il n'en efface aucun.
    Nothing,
    /// Il en laisse exactement ce nombre (plus ou moins qu'avant).
    Leaves(u32),
    /// L'appel échoue.
    Fails,
}

#[derive(Debug, Default)]
struct KeyState {
    link: bool,
    unreadable: bool,
    incomplete: bool,
    values: Vec<ValueName>,
    /// Des valeurs qui reviennent après la première suppression (l'Explorateur
    /// qui réécrit ce qu'il avait en mémoire).
    respawn: Vec<ValueName>,
    stuck: Vec<ValueName>,
}

#[derive(Debug)]
struct Fake {
    local_app_data: Option<PathBuf>,
    firefox: bool,
    processes: Option<Vec<String>>,
    held: HashSet<PathBuf>,
    recent: u32,
    recent_clear: RecentClear,
    recent_unreadable: bool,
    keys: HashMap<TypedKey, KeyState>,
    deleted: Vec<(TypedKey, ValueName)>,
    clipboard_filled: bool,
    history_enabled: Option<bool>,
    clipboard_unreadable: bool,
    clipboard_result: ClipboardClear,
    clipboard_calls: Vec<isize>,
    recent_calls: u32,
    gate: Option<(Sender<()>, Receiver<()>)>,
}

/// La machine des essais.
#[derive(Debug)]
pub struct FakePrivacyHost {
    state: Mutex<Fake>,
}

impl FakePrivacyHost {
    /// Une machine sans navigateur : `local_app_data` est ce dossier (forme interne).
    pub fn new(local_app_data: Option<PathBuf>) -> Self {
        Self {
            state: Mutex::new(Fake {
                local_app_data,
                firefox: false,
                processes: Some(Vec::new()),
                held: HashSet::new(),
                recent: 0,
                recent_clear: RecentClear::All,
                recent_unreadable: false,
                keys: HashMap::new(),
                deleted: Vec::new(),
                clipboard_filled: false,
                history_enabled: Some(false),
                clipboard_unreadable: false,
                clipboard_result: ClipboardClear {
                    history: HistoryClear::Cleared,
                    current: CurrentClear::Emptied,
                },
                clipboard_calls: Vec::new(),
                recent_calls: 0,
                gate: None,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, Fake> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn key(fake: &mut Fake, key: TypedKey) -> &mut KeyState {
        fake.keys.entry(key).or_default()
    }

    /// Le prochain effacement du presse-papiers **s'arrête** en plein geste, jusqu'à ce
    /// que le test lâche la barrière : de quoi tenir un effacement « en cours ». Rend
    /// ce que le test attend pour savoir que l'effacement est arrivé là, et ce qu'il envoie pour le laisser finir.
    pub fn block_next_clipboard(&self) -> (Receiver<()>, Sender<()>) {
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        self.state().gate = Some((entered_tx, release_rx));
        (entered_rx, release_tx)
    }

    /// Un dossier de profils Firefox existe.
    pub fn set_firefox(&self, present: bool) {
        self.state().firefox = present;
    }

    /// Les programmes en cours (`None` : la machine ne sait pas le dire).
    pub fn set_processes(&self, processes: Option<&[&str]>) {
        self.state().processes =
            processes.map(|list| list.iter().map(|s| (*s).to_owned()).collect());
    }

    /// Le verrou de ce dossier `User Data` est tenu (ou lâché).
    pub fn hold_lockfile(&self, user_data: &Path, held: bool) {
        let mut state = self.state();
        if held {
            state.held.insert(user_data.to_path_buf());
        } else {
            state.held.remove(user_data);
        }
    }

    /// Le nombre de raccourcis récents, et ce que l'effacement en fait.
    pub fn set_recent(&self, count: u32, clear: RecentClear) {
        let mut state = self.state();
        state.recent = count;
        state.recent_clear = clear;
    }

    /// Le dossier « Récents » ne se lit pas.
    pub fn make_recent_unreadable(&self) {
        self.state().recent_unreadable = true;
    }

    /// Combien de fois `clear_recent` a été appelé.
    pub fn recent_calls(&self) -> u32 {
        self.state().recent_calls
    }

    /// Pose ces valeurs dans cette clé (qui existe dès lors).
    pub fn set_values(&self, key: TypedKey, names: &[&str]) {
        let mut state = self.state();
        Self::key(&mut state, key).values = names.iter().map(|n| ValueName::new(n)).collect();
    }

    /// Cette clé est un lien de registre.
    pub fn make_link(&self, key: TypedKey) {
        let mut state = self.state();
        let entry = Self::key(&mut state, key);
        entry.link = true;
    }

    /// Cette clé ne se lit pas.
    pub fn make_key_unreadable(&self, key: TypedKey) {
        let mut state = self.state();
        Self::key(&mut state, key).unreadable = true;
    }

    /// Cette clé dépasse une borne : sa liste est incomplète.
    pub fn make_key_incomplete(&self, key: TypedKey) {
        let mut state = self.state();
        Self::key(&mut state, key).incomplete = true;
    }

    /// Ces valeurs reviennent après la première suppression de cette clé.
    pub fn respawn_after_delete(&self, key: TypedKey, names: &[&str]) {
        let mut state = self.state();
        Self::key(&mut state, key).respawn = names.iter().map(|n| ValueName::new(n)).collect();
    }

    /// Cette valeur ne se supprime pas (Windows refuse).
    pub fn refuse_delete(&self, key: TypedKey, name: &str) {
        let mut state = self.state();
        Self::key(&mut state, key).stuck.push(ValueName::new(name));
    }

    /// Les suppressions faites, dans l'ordre, par nom (en texte).
    pub fn deleted(&self) -> Vec<(TypedKey, String)> {
        self.state()
            .deleted
            .iter()
            .map(|(key, name)| (*key, String::from_utf16_lossy(&name.0)))
            .collect()
    }

    /// Les noms qui restent dans cette clé, en texte.
    pub fn remaining(&self, key: TypedKey) -> Vec<String> {
        self.state()
            .keys
            .get(&key)
            .map(|entry| entry.values.iter().map(|n| String::from_utf16_lossy(&n.0)).collect())
            .unwrap_or_default()
    }

    /// Le presse-papiers est plein (ou vide), et ce que « le vider » rend.
    pub fn set_clipboard(&self, filled: bool, result: ClipboardClear) {
        let mut state = self.state();
        state.clipboard_filled = filled;
        state.clipboard_result = result;
    }

    /// L'historique du presse-papiers est activé (`Some(true)`), désactivé, ou illisible (`None`).
    pub fn set_clipboard_history(&self, enabled: Option<bool>) {
        self.state().history_enabled = enabled;
    }

    /// Le contenu actuel du presse-papiers ne se lit pas (`false` : il se lit à nouveau).
    pub fn make_clipboard_unreadable(&self, unreadable: bool) {
        self.state().clipboard_unreadable = unreadable;
    }

    /// Les fenêtres propriétaires données à chaque effacement du presse-papiers.
    pub fn clipboard_calls(&self) -> Vec<isize> {
        self.state().clipboard_calls.clone()
    }

    /// Le presse-papiers est-il encore plein ?
    pub fn clipboard_is_filled(&self) -> bool {
        self.state().clipboard_filled
    }
}

impl PrivacyHost for FakePrivacyHost {
    fn local_app_data(&self) -> Option<PathBuf> {
        self.state().local_app_data.clone()
    }

    fn firefox_present(&self) -> bool {
        self.state().firefox
    }

    fn running_processes(&self) -> Option<Vec<String>> {
        self.state().processes.clone()
    }

    fn lockfile_held(&self, user_data: &Path) -> bool {
        self.state().held.contains(user_data)
    }

    fn recent_count(&self) -> io::Result<u32> {
        let state = self.state();
        if state.recent_unreadable {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(state.recent)
    }

    fn clear_recent(&self) -> io::Result<()> {
        let mut state = self.state();
        state.recent_calls += 1;
        match state.recent_clear {
            RecentClear::All => {
                state.recent = 0;
                Ok(())
            }
            RecentClear::Nothing => Ok(()),
            RecentClear::Leaves(left) => {
                state.recent = left;
                Ok(())
            }
            RecentClear::Fails => Err(io::Error::other("refusé")),
        }
    }

    fn registry_values(&self, key: TypedKey) -> Result<Option<RegistryValues>, RegistryError> {
        let state = self.state();
        let Some(entry) = state.keys.get(&key) else { return Ok(None) };
        if entry.link {
            return Err(RegistryError::Link);
        }
        if entry.unreadable {
            return Err(RegistryError::Io(io::Error::from(io::ErrorKind::PermissionDenied)));
        }
        Ok(Some(RegistryValues { names: entry.values.clone(), complete: !entry.incomplete }))
    }

    fn registry_delete(&self, key: TypedKey, name: &ValueName) -> Result<(), RegistryError> {
        let mut state = self.state();
        let entry = state.keys.entry(key).or_default();
        if entry.link {
            return Err(RegistryError::Link);
        }
        if entry.stuck.contains(name) {
            return Err(RegistryError::Io(io::Error::from(io::ErrorKind::PermissionDenied)));
        }
        entry.values.retain(|value| value != name);
        let back = std::mem::take(&mut entry.respawn);
        entry.values.extend(back);
        state.deleted.push((key, name.clone()));
        Ok(())
    }

    fn clipboard_filled(&self) -> io::Result<bool> {
        let state = self.state();
        if state.clipboard_unreadable {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(state.clipboard_filled)
    }

    fn clipboard_history_enabled(&self) -> io::Result<bool> {
        self.state().history_enabled.ok_or_else(|| io::Error::from(io::ErrorKind::PermissionDenied))
    }

    fn clear_clipboard(&self, owner: isize) -> ClipboardClear {
        let gate = self.state().gate.take();
        if let Some((entered, release)) = gate {
            let _ = entered.send(());
            let _ = release.recv();
        }
        let mut state = self.state();
        state.clipboard_calls.push(owner);
        let result = state.clipboard_result;
        if result.current == CurrentClear::Emptied {
            state.clipboard_filled = false;
        }
        result
    }
}
