//! Le Journal : le tri, les bornes, les dates hostiles, les clés.

use serde_json::Value;

use super::*;

const NOW: i64 = 1_763_300_000_000;
const DAY: i64 = 24 * 3600 * 1000;

fn run(started: i64, finished: Option<i64>, state: RunState) -> Run {
    Run {
        started_ms: started,
        finished_ms: finished,
        state,
        freed_bytes: 100,
        recoverable_bytes: 50,
        skipped_files: 2,
    }
}

fn cut(at: i64, name: &str, back: Option<i64>) -> CutLine {
    use crate::startup::{EntryId, Place};
    CutLine {
        id: EntryId::new(Place::UserRun, name),
        name: name.to_owned(),
        at_ms: at,
        file_time: 133_760_000_000_000_000,
        back_ms: back,
    }
}

fn game(name: &str, started: i64, ended: i64) -> LastGame {
    LastGame {
        name: name.to_owned(),
        started,
        ended,
        power_restored: Some(true),
        apps_restored: 2,
        complete: true,
    }
}

fn note(at: i64, name: &str) -> UninstallNote {
    UninstallNote::new(at, name)
}

fn lines(view: &HistoryView) -> &[HistoryLine] {
    &view.lines
}

fn date_of(line: &HistoryLine) -> Option<i64> {
    line.sort_date()
}

#[test]
fn nothing_gives_an_empty_journal() {
    let view = assemble(NOW, &[], &[], None, &[]);
    assert!(view.lines.is_empty());
    assert!(!view.more);
}

#[test]
fn every_kind_of_line_is_carried_with_its_fields() {
    let runs = [run(NOW - 5000, Some(NOW - 4000), RunState::Completed)];
    let cuts = [cut(NOW - 3000, "Discord", Some(NOW - 1000))];
    let last = game("Jeu", NOW - 9000, NOW - 8000);
    let notes = [UninstallNote { leftovers: 3, leftover_bytes: 340, ..note(NOW - 2000, "Essai") }];
    let view = assemble(NOW, &runs, &cuts, Some(&last), &notes);
    assert_eq!(
        view.lines,
        [
            HistoryLine::Uninstall {
                at_ms: Some(NOW - 2000),
                name: "Essai".to_owned(),
                leftovers: 3,
                leftover_bytes: 340
            },
            HistoryLine::StartupCut {
                at_ms: Some(NOW - 3000),
                name: "Discord".to_owned(),
                back_ms: Some(NOW - 1000)
            },
            HistoryLine::Clean {
                started_ms: Some(NOW - 5000),
                finished_ms: Some(NOW - 4000),
                state: RunState::Completed,
                freed_bytes: 100,
                recoverable_bytes: 50,
                skipped_files: 2
            },
            HistoryLine::Game {
                started_ms: Some(NOW - 9000),
                ended_ms: Some(NOW - 8000),
                name: "Jeu".to_owned(),
                power_restored: Some(true),
                apps_restored: 2,
                complete: true
            },
        ]
    );
}

#[test]
fn lines_are_sorted_from_the_most_recent_with_the_undated_at_the_end() {
    let runs = [
        run(NOW - 100, None, RunState::Running),
        run(NOW - 9000, Some(NOW - 8000), RunState::Cancelled),
        run(0, None, RunState::Interrupted),
    ];
    let cuts = [cut(NOW - 500, "B", None), cut(-5, "Sans date", None)];
    let view = assemble(NOW, &runs, &cuts, None, &[note(NOW - 300, "U")]);
    let dates: Vec<Option<i64>> = lines(&view).iter().map(date_of).collect();
    assert_eq!(
        dates,
        [Some(NOW - 100), Some(NOW - 300), Some(NOW - 500), Some(NOW - 8000), None, None]
    );
}

#[test]
fn a_clean_is_sorted_by_its_end_then_its_start() {
    // Début ancien mais fin récente : la fin range la ligne.
    let runs = [
        run(NOW - 90_000, Some(NOW - 1000), RunState::Completed),
        run(NOW - 5000, None, RunState::Running),
    ];
    let view = assemble(NOW, &runs, &[], None, &[]);
    assert_eq!(date_of(&view.lines[0]), Some(NOW - 1000));
    assert_eq!(date_of(&view.lines[1]), Some(NOW - 5000));
}

#[test]
fn a_game_is_sorted_by_its_end_then_its_start() {
    let last = game("Jeu", NOW - 7000, NOW - 2000);
    let view = assemble(NOW, &[], &[cut(NOW - 3000, "A", None)], Some(&last), &[]);
    assert!(matches!(view.lines[0], HistoryLine::Game { .. }), "la fin (-2000) passe avant -3000");
    // Une fin sans sens : la partie se range par son début.
    let last = game("Jeu", NOW - 7000, 0);
    let view = assemble(NOW, &[], &[cut(NOW - 8000, "A", None)], Some(&last), &[]);
    assert_eq!(date_of(&view.lines[0]), Some(NOW - 7000));
}

#[test]
fn equal_dates_are_ordered_by_kind_then_name() {
    let runs = [run(NOW - 10, Some(NOW - 10), RunState::Completed)];
    let cuts = [cut(NOW - 10, "Zèbre", None), cut(NOW - 10, "Abeille", None)];
    let notes = [note(NOW - 10, "Beta"), note(NOW - 10, "Alpha")];
    let last = game("Jeu", NOW - 20, NOW - 10);
    let view = assemble(NOW, &runs, &cuts, Some(&last), &notes);
    let kinds: Vec<String> = lines(&view)
        .iter()
        .map(|line| {
            let (rank, name) = line.tie_break();
            format!("{rank}:{name}")
        })
        .collect();
    assert_eq!(kinds, ["0:", "1:Abeille", "1:Zèbre", "2:Jeu", "3:Alpha", "3:Beta"]);
    // Dans l'autre ordre d'entrée, le même résultat.
    let reversed_cuts = [cuts[1].clone(), cuts[0].clone()];
    let reversed_notes = [notes[1].clone(), notes[0].clone()];
    assert_eq!(view, assemble(NOW, &runs, &reversed_cuts, Some(&last), &reversed_notes));
}

#[test]
fn at_most_two_hundred_lines_and_more_says_there_were_others() {
    let notes: Vec<UninstallNote> =
        (0..250).map(|index| note(NOW - 1000 - index, &format!("App {index}"))).collect();
    let view = assemble(NOW, &[], &[], None, &notes);
    assert_eq!(view.lines.len(), LINES_KEPT);
    assert!(view.more);
    // Les plus récentes restent.
    assert_eq!(date_of(&view.lines[0]), Some(NOW - 1000));
    assert_eq!(date_of(&view.lines[199]), Some(NOW - 1199));
    // Pile deux cents : rien n'est caché.
    let view = assemble(NOW, &[], &[], None, &notes[..200]);
    assert_eq!(view.lines.len(), 200);
    assert!(!view.more);
}

#[test]
fn running_and_interrupted_runs_are_shown() {
    let runs = [run(NOW - 10, None, RunState::Running), run(NOW - 20, None, RunState::Interrupted)];
    let view = assemble(NOW, &runs, &[], None, &[]);
    let states: Vec<RunState> = lines(&view)
        .iter()
        .filter_map(|line| match line {
            HistoryLine::Clean { state, .. } => Some(*state),
            _ => None,
        })
        .collect();
    assert_eq!(states, [RunState::Running, RunState::Interrupted]);
}

#[test]
fn hostile_dates_become_none() {
    let hostile = [0, -1, i64::MIN, i64::MAX, NOW + DAY + 1, NOW + 365 * DAY];
    for date in hostile {
        let cuts = [cut(date, "Coupée", Some(date))];
        let notes = [note(date, "Vue")];
        let last = game("Jeu", date, date);
        let runs = [run(date, Some(date), RunState::Completed)];
        let view = assemble(NOW, &runs, &cuts, Some(&last), &notes);
        assert_eq!(view.lines.len(), 4);
        for line in &view.lines {
            assert_eq!(date_of(line), None, "{date} : {line:?}");
            match line {
                HistoryLine::StartupCut { back_ms, .. } => assert_eq!(*back_ms, None),
                HistoryLine::Game { started_ms, ended_ms, .. } => {
                    assert_eq!((*started_ms, *ended_ms), (None, None));
                }
                HistoryLine::Clean { started_ms, finished_ms, .. } => {
                    assert_eq!((*started_ms, *finished_ms), (None, None));
                }
                HistoryLine::Uninstall { at_ms, .. } => assert_eq!(*at_ms, None),
            }
        }
    }
}

#[test]
fn the_limits_of_a_sane_date_are_one_and_tomorrow() {
    assert_eq!(sane(1, NOW), Some(1));
    assert_eq!(sane(0, NOW), None);
    assert_eq!(sane(NOW + DAY, NOW), Some(NOW + DAY));
    assert_eq!(sane(NOW + DAY + 1, NOW), None);
    // Une heure d'à présent absurde ne fait pas déborder le calcul.
    assert_eq!(sane(5, i64::MAX), Some(5));
}

#[test]
fn a_future_date_does_not_stay_on_top() {
    let notes = [note(NOW + 30 * DAY, "Futur"), note(NOW - 10, "Présent")];
    let view = assemble(NOW, &[], &[], None, &notes);
    assert!(matches!(&view.lines[0], HistoryLine::Uninstall { name, .. } if name == "Présent"));
    assert!(matches!(&view.lines[1], HistoryLine::Uninstall { at_ms: None, .. }));
}

/// Toutes les clés d'un JSON, à tous les niveaux.
fn keys(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                found.push(key.clone());
                keys(inner, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| keys(item, found)),
        _ => {}
    }
}

#[test]
fn the_json_has_no_key_for_a_path_or_an_id() {
    // Un nom qui contient « id » et « path » : l'essai porte sur les clés.
    let runs = [run(NOW - 5, Some(NOW - 4), RunState::Completed)];
    let cuts = [cut(NOW - 3, "Nvidia Path", Some(NOW - 1))];
    let last = game("Valid", NOW - 9, NOW - 8);
    let notes = [note(NOW - 2, r"C:\Windows\id")];
    let view = assemble(NOW, &runs, &cuts, Some(&last), &notes);
    let json = serde_json::to_value(&view).unwrap();
    let mut found = Vec::new();
    keys(&json, &mut found);
    assert!(!found.is_empty());
    for key in &found {
        let lower = key.to_lowercase();
        assert!(lower != "id" && !lower.contains("path") && !lower.ends_with("id"), "clé {key}");
    }
    // Les noms, eux, passent tels quels.
    let text = json.to_string();
    assert!(text.contains("Nvidia Path"));
}

#[test]
fn the_json_uses_camel_case_and_a_kind_tag() {
    let view =
        assemble(NOW, &[run(NOW - 5, None, RunState::Running)], &[], None, &[note(NOW - 6, "A")]);
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["more"], false);
    assert_eq!(json["lines"][0]["kind"], "clean");
    assert_eq!(json["lines"][0]["startedMs"], NOW - 5);
    assert_eq!(json["lines"][0]["state"], "running");
    assert_eq!(json["lines"][0]["finishedMs"], Value::Null);
    assert_eq!(json["lines"][1]["kind"], "uninstall");
    assert_eq!(json["lines"][1]["leftoverBytes"], 0);
}

#[test]
fn no_number_of_bytes_or_files_goes_past_the_largest_safe_integer() {
    let mut huge = run(NOW - 1000, Some(NOW), RunState::Completed);
    huge.freed_bytes = u64::MAX;
    huge.recoverable_bytes = u64::MAX;
    huge.skipped_files = u64::MAX;
    let mut gone = note(NOW - 2000, "Essai");
    gone.leftover_bytes = u64::MAX;
    gone.leftovers = u32::MAX;
    let view = assemble(NOW, &[huge], &[], None, &[gone]);
    let safe = (1_u64 << 53) - 1;
    assert_eq!(SAFE_INTEGER, safe);
    assert!(matches!(
        &view.lines[0],
        HistoryLine::Clean { freed_bytes, recoverable_bytes, skipped_files, .. }
            if (*freed_bytes, *recoverable_bytes, *skipped_files) == (safe, safe, safe)
    ));
    assert!(matches!(
        &view.lines[1],
        HistoryLine::Uninstall { leftover_bytes, leftovers, .. }
            if *leftover_bytes == safe && *leftovers == u32::MAX
    ));
    // Une valeur sûre passe telle quelle.
    let small = assemble(NOW, &[run(NOW - 1, Some(NOW), RunState::Completed)], &[], None, &[]);
    assert!(matches!(&small.lines[0], HistoryLine::Clean { freed_bytes: 100, .. }));
}
