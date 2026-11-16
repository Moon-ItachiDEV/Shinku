//! Les essais de la suite de l'assistant, sur une machine fabriquée.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use super::*;
use crate::startup::approval::{cut_bytes, restore_bytes};

type Hook = Box<dyn Fn(&Fake)>;
type Approvals = HashMap<(StartupSlot, String), io::Result<Option<RawApproval>>>;

/// Une machine fabriquée : elle note tout ce qu'on lui demande, dans l'ordre.
struct Fake {
    rights: Cell<bool>,
    safe: Cell<bool>,
    names: RefCell<HashMap<StartupSlot, Result<Names, NamesError>>>,
    approvals: RefCell<Approvals>,
    answer: Cell<bool>,
    write_fails: Cell<bool>,
    write_ignored: Cell<bool>,
    during_question: RefCell<Option<Hook>>,
    log: RefCell<Vec<String>>,
    now: u64,
}

const NOW: u64 = 133_800_000_000_000_000;

impl Fake {
    fn new() -> Self {
        let fake = Self {
            rights: Cell::new(true),
            safe: Cell::new(true),
            names: RefCell::new(HashMap::new()),
            approvals: RefCell::new(HashMap::new()),
            answer: Cell::new(true),
            write_fails: Cell::new(false),
            write_ignored: Cell::new(false),
            during_question: RefCell::new(None),
            log: RefCell::new(Vec::new()),
            now: NOW,
        };
        for slot in StartupSlot::ALL {
            fake.names.borrow_mut().insert(slot, Ok(Names { names: Vec::new(), complete: true }));
        }
        fake
    }

    fn with_entry(self, slot: StartupSlot, name: &str) -> Self {
        self.add(slot, name);
        self
    }

    fn add(&self, slot: StartupSlot, name: &str) {
        if let Some(Ok(names)) = self.names.borrow_mut().get_mut(&slot) {
            names.names.push(name.to_owned());
        }
    }

    fn set_bytes(&self, slot: StartupSlot, name: &str, bytes: &[u8]) {
        let value = RawApproval { is_binary: true, bytes: bytes.to_vec() };
        self.approvals.borrow_mut().insert((slot, name.to_owned()), Ok(Some(value)));
    }

    fn approval_now(&self, slot: StartupSlot, name: &str) -> Option<Vec<u8>> {
        match self.approvals.borrow().get(&(slot, name.to_owned())) {
            Some(Ok(Some(value))) => Some(value.bytes.clone()),
            _ => None,
        }
    }

    fn writes(&self) -> Vec<String> {
        self.log.borrow().iter().filter(|line| line.starts_with("write")).cloned().collect()
    }

    fn log(&self, line: &str) {
        self.log.borrow_mut().push(line.to_owned());
    }
}

impl MachineStartup for Fake {
    fn has_rights(&self) -> bool {
        self.log("rights");
        self.rights.get()
    }

    fn location_safe(&self) -> bool {
        self.log("location");
        self.safe.get()
    }

    fn names(&self, place: StartupSlot) -> Result<Names, NamesError> {
        self.log("names");
        self.names.borrow().get(&place).cloned().unwrap_or(Err(NamesError::Unreadable))
    }

    fn approval(&self, place: StartupSlot, name: &str) -> io::Result<Option<RawApproval>> {
        self.log("approval");
        match self.approvals.borrow().get(&(place, name.to_owned())) {
            None => Ok(None),
            Some(Ok(value)) => Ok(value.clone()),
            Some(Err(_)) => Err(io::Error::other("illisible")),
        }
    }

    fn set_approval(&self, place: StartupSlot, name: &str, bytes: [u8; 12]) -> io::Result<()> {
        self.log(&format!("write {name}"));
        if self.write_fails.get() {
            return Err(io::Error::other("refusé"));
        }
        if !self.write_ignored.get() {
            self.set_bytes(place, name, &bytes);
        }
        Ok(())
    }

    fn file_time_now(&self) -> u64 {
        self.now
    }

    fn ask(&self, text: &str) -> bool {
        self.log(&format!("ask {text}"));
        if let Some(hook) = self.during_question.borrow().as_ref() {
            hook(self);
        }
        self.answer.get()
    }
}

fn words(action: &str, place: &str, name: &str) -> Vec<OsString> {
    let request = StartupRequest::new(
        if action == "couper" { StartupAction::Cut } else { StartupAction::Restore },
        StartupSlot::ALL.into_iter().find(|slot| slot.word() == place).unwrap(),
        name,
    )
    .unwrap();
    request.to_words().into_iter().map(OsString::from).collect()
}

fn cut(fake: &Fake) -> u8 {
    run(fake, &words("couper", "run", "Essai"))
}

fn base() -> Fake {
    Fake::new().with_entry(StartupSlot::Run, "Essai")
}

#[test]
fn a_cut_is_written_after_the_question_and_read_back() {
    let fake = base();
    assert_eq!(cut(&fake), 0);
    assert_eq!(fake.approval_now(StartupSlot::Run, "Essai"), Some(cut_bytes(NOW).to_vec()));
    // La question vient avant l'écriture, et toute lecture d'avant aussi.
    let log = fake.log.borrow();
    let ask = log.iter().position(|line| line.starts_with("ask")).unwrap();
    let write = log.iter().position(|line| line.starts_with("write")).unwrap();
    assert!(ask < write);
    assert!(log[..ask].iter().all(|line| !line.starts_with("write")));
}

#[test]
fn a_restore_writes_the_active_value() {
    let fake = base();
    fake.set_bytes(StartupSlot::Run, "Essai", &cut_bytes(NOW));
    assert_eq!(run(&fake, &words("remettre", "run", "Essai")), 0);
    assert_eq!(fake.approval_now(StartupSlot::Run, "Essai"), Some(restore_bytes().to_vec()));
}

#[test]
fn a_refused_request_is_code_2_and_nothing_else_is_asked() {
    let fake = base();
    for arguments in
        [vec![], vec![OsString::from("demarrage1")], words("couper", "run", "Essai")[..3].to_vec()]
    {
        assert_eq!(run(&fake, &arguments), 2);
    }
    assert!(fake.log.borrow().is_empty(), "aucune question à la machine : {:?}", fake.log.borrow());
}

#[test]
fn no_rights_is_code_5_before_anything_is_read() {
    let fake = base();
    fake.rights.set(false);
    assert_eq!(cut(&fake), 5);
    assert_eq!(*fake.log.borrow(), ["rights"]);
}

#[test]
fn an_unsafe_location_is_code_6_before_the_places_are_read() {
    let fake = base();
    fake.safe.set(false);
    assert_eq!(cut(&fake), 6);
    assert_eq!(*fake.log.borrow(), ["rights", "location"]);
}

#[test]
fn an_unsafe_common_folder_is_code_6_and_an_unreadable_place_is_code_8() {
    let fake = base().with_entry(StartupSlot::Folder, "x.lnk");
    fake.names.borrow_mut().insert(StartupSlot::Folder, Err(NamesError::UnsafeFolder));
    assert_eq!(run(&fake, &words("couper", "dossier", "x.lnk")), 6);
    fake.names.borrow_mut().insert(StartupSlot::Folder, Err(NamesError::Unreadable));
    assert_eq!(run(&fake, &words("couper", "dossier", "x.lnk")), 8);
    assert!(fake.writes().is_empty());
}

#[test]
fn the_name_must_exist_exactly_once_with_the_exact_units() {
    let missing = Fake::new();
    assert_eq!(cut(&missing), 4);
    // La casse compte : `essai` n'est pas `Essai`.
    let other_case = Fake::new().with_entry(StartupSlot::Run, "essai");
    assert_eq!(cut(&other_case), 4);
    // Une autre place : pas la bonne.
    let other_place = Fake::new().with_entry(StartupSlot::Run32, "Essai");
    assert_eq!(cut(&other_place), 4);
    // Deux noms égaux (dossier sensible à la casse n'en donne pas, mais un hôte pourrait).
    let twice = base().with_entry(StartupSlot::Run, "Essai");
    assert_eq!(cut(&twice), 4);
    // Deux noms qui ne diffèrent que par la casse : celui de la demande reste seul.
    let both = base().with_entry(StartupSlot::Run, "ESSAI");
    assert_eq!(cut(&both), 0);
    assert_eq!(both.approval_now(StartupSlot::Run, "ESSAI"), None, "l'autre n'est pas touché");
    for fake in [&missing, &other_case, &other_place, &twice] {
        assert!(fake.writes().is_empty());
    }
}

#[test]
fn a_place_read_in_part_does_not_prevent_finding_the_name() {
    let fake = base();
    fake.names
        .borrow_mut()
        .insert(StartupSlot::Run, Ok(Names { names: vec!["Essai".to_owned()], complete: false }));
    assert_eq!(cut(&fake), 0);
}

#[test]
fn an_unreadable_approval_is_code_7_and_nothing_is_written() {
    let fake = base();
    fake.approvals
        .borrow_mut()
        .insert((StartupSlot::Run, "Essai".to_owned()), Err(io::Error::other("x")));
    assert_eq!(cut(&fake), 7);
    let strange = base();
    strange.set_bytes(StartupSlot::Run, "Essai", &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(cut(&strange), 7);
    assert!(fake.writes().is_empty() && strange.writes().is_empty());
}

#[test]
fn already_in_the_requested_state_is_code_1_without_a_question_or_a_write() {
    let active = base();
    assert_eq!(run(&active, &words("remettre", "run", "Essai")), 1);
    let cut_already = base();
    cut_already.set_bytes(StartupSlot::Run, "Essai", &cut_bytes(NOW));
    assert_eq!(cut(&cut_already), 1);
    for fake in [&active, &cut_already] {
        assert!(
            fake.log
                .borrow()
                .iter()
                .all(|line| !line.starts_with("ask") && !line.starts_with("write"))
        );
    }
}

#[test]
fn a_refusal_of_the_question_is_code_3_and_nothing_is_written() {
    let fake = base();
    fake.answer.set(false);
    assert_eq!(cut(&fake), 3);
    assert!(fake.writes().is_empty());
    assert_eq!(fake.approval_now(StartupSlot::Run, "Essai"), None);
}

#[test]
fn the_entry_deleted_or_doubled_during_the_question_is_code_4() {
    let gone = base();
    *gone.during_question.borrow_mut() = Some(Box::new(|fake| {
        fake.names
            .borrow_mut()
            .insert(StartupSlot::Run, Ok(Names { names: vec![], complete: true }))
            .map(|_| ())
            .unwrap_or(())
    }));
    assert_eq!(cut(&gone), 4);
    let doubled = base();
    *doubled.during_question.borrow_mut() =
        Some(Box::new(|fake| fake.add(StartupSlot::Run, "Essai")));
    assert_eq!(cut(&doubled), 4);
    assert!(gone.writes().is_empty() && doubled.writes().is_empty());
}

#[test]
fn the_approval_changed_during_the_question_is_checked_again() {
    // Devenue illisible : code 7.
    let unreadable = base();
    *unreadable.during_question.borrow_mut() = Some(Box::new(|fake| {
        fake.approvals
            .borrow_mut()
            .insert((StartupSlot::Run, "Essai".to_owned()), Err(io::Error::other("x")));
    }));
    assert_eq!(cut(&unreadable), 7);
    // Déjà coupée par quelqu'un d'autre pendant ce temps : code 1, rien d'écrit.
    let already = base();
    *already.during_question.borrow_mut() =
        Some(Box::new(|fake| fake.set_bytes(StartupSlot::Run, "Essai", &cut_bytes(NOW))));
    assert_eq!(cut(&already), 1);
    assert!(unreadable.writes().is_empty() && already.writes().is_empty());
}

#[test]
fn a_failed_or_ignored_write_is_code_8() {
    let refused = base();
    refused.write_fails.set(true);
    assert_eq!(cut(&refused), 8);
    let ignored = base();
    ignored.write_ignored.set(true);
    assert_eq!(cut(&ignored), 8);
    assert_eq!(ignored.writes().len(), 1);
}

#[test]
fn only_the_requested_entry_is_written_and_only_once() {
    let fake = base().with_entry(StartupSlot::Run, "Autre");
    assert_eq!(cut(&fake), 0);
    assert_eq!(fake.writes(), ["write Essai"]);
}

#[test]
fn the_question_names_the_entry_its_place_and_the_warning() {
    let text = question_text(StartupAction::Cut, StartupSlot::Run32, "Mon app");
    assert!(text.starts_with(
        "Couper « Mon app » au démarrage de Windows, pour tous les comptes de ce PC ?"
    ));
    assert!(text.contains("Où elle est inscrite : registre de Windows, Run (32 bits)."));
    assert!(text.contains("L'app n'est pas désinstallée."));
    assert!(text.ends_with("choisis Annuler."));
    assert!(!text.contains("caractères particuliers"));

    let text = question_text(StartupAction::Restore, StartupSlot::Folder, "x.lnk");
    assert!(text.starts_with("Remettre « x.lnk » au démarrage de Windows"));
    assert!(text.contains("Où elle est inscrite : dossier Démarrage commun."));
    assert!(!text.contains("désinstallée"));
    assert!(text.contains("Si tu n'as pas touché à cet interrupteur dans Shinku"));

    let text = question_text(StartupAction::Cut, StartupSlot::Run, "Sécu\u{202E}rité");
    assert!(text.contains("⟨U+202E⟩"), "{text}");
    assert!(text.contains("caractères particuliers"));
    let text = question_text(StartupAction::Cut, StartupSlot::Run, &"n".repeat(200));
    assert!(text.contains('…') && text.contains("raccourci"));
    assert!(question_text(StartupAction::Cut, StartupSlot::Run, "a").contains("Run (64 bits)"));
}

#[test]
fn the_text_of_the_question_is_the_one_the_page_shows() {
    let name = "Réglages\u{A0}audio";
    let text = question_text(StartupAction::Cut, StartupSlot::Run, name);
    assert!(text.contains(&crate::startup::shown::shown_name(name).text));
}

#[test]
fn the_deadline_is_won_by_the_first_to_change_the_state() {
    let answered_first = Deadline::new();
    assert!(answered_first.answer());
    assert!(!answered_first.expire(), "la minuterie arrive trop tard");
    assert!(!answered_first.expired());
    let expired_first = Deadline::new();
    assert!(expired_first.expire());
    assert!(!expired_first.answer(), "la réponse arrive trop tard");
    assert!(expired_first.expired());
    assert!(!expired_first.expire(), "une seule fois");
    assert_eq!(QUESTION_DEADLINE, Duration::from_secs(120));
}

#[test]
fn the_deadline_has_one_winner_among_many_threads() {
    for _ in 0..50 {
        let deadline = std::sync::Arc::new(Deadline::new());
        let winners = std::sync::Arc::new(AtomicU8::new(0));
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let (deadline, winners) = (deadline.clone(), winners.clone());
                std::thread::spawn(move || {
                    let won = if index % 2 == 0 { deadline.answer() } else { deadline.expire() };
                    if won {
                        winners.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(winners.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn the_exit_codes_are_the_documented_numbers() {
    let numbers: Vec<u8> = [
        Code::Done,
        Code::Already,
        Code::Refused,
        Code::Cancelled,
        Code::NotFound,
        Code::NoRights,
        Code::UnsafeLocation,
        Code::Unreadable,
        Code::Failed,
    ]
    .iter()
    .map(|code| *code as u8)
    .collect();
    assert_eq!(numbers, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
}
