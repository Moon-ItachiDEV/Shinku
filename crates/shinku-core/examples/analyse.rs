//! Lance une vraie analyse sur cette machine et affiche le résultat.
//!
//! Lecture seule : rien n'est supprimé, déplacé ni modifié. Utile pour voir
//! ce que le moteur trouve sur un vrai PC avant que l'interface n'existe.
//!
//! ```text
//! cargo run --release --example analyse
//! cargo run --release --example analyse -- --json > rapport.json
//! cargo run --release --example analyse -- --sans-doublons
//! ```

use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use shinku_core::fs::to_display_path;
use shinku_core::hash::MemoryCache;
use shinku_core::model::{ByteSize, ModuleId, Trigger};
use shinku_core::platform::system;
use shinku_core::scan::report::{CategoryState, Report};
use shinku_core::scan::{Phase, ScanOptions, start};
use shinku_core::settings::SettingsStore;

fn main() -> ExitCode {
    let mut json = false;
    let mut options = ScanOptions::new(Trigger::Button);
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--json" => json = true,
            "--sans-doublons" => options.modules.duplicates = false,
            other => {
                eprintln!("option inconnue : {other}");
                eprintln!("options : --json, --sans-doublons");
                return ExitCode::from(2);
            }
        }
    }

    let machine = Arc::new(system());
    // Les « dossiers jamais touchés » de l'utilisateur valent aussi ici.
    if let Some(store) = SettingsStore::for_machine(machine.as_ref()) {
        let loaded = store.load();
        if let Some(issue) = &loaded.issue {
            eprintln!("réglages : {issue:?}");
        }
        options = options.with_settings(&loaded.settings);
    }

    let scan = match start(machine, Arc::new(MemoryCache::new()), options) {
        Ok(scan) => scan,
        Err(error) => {
            eprintln!("l'analyse n'a pas pu démarrer : {error}");
            return ExitCode::FAILURE;
        }
    };
    while !scan.is_finished() {
        let progress = scan.control().progress();
        let step = match progress.phase {
            Phase::Preparing => "préparation".to_owned(),
            Phase::Walking => format!(
                "parcours : {} dossiers, {} fichiers, {} trouvés",
                progress.folders,
                progress.files,
                ByteSize(progress.found_bytes)
            ),
            Phase::DuplicatesQuick | Phase::DuplicatesFull => format!(
                "doublons : {} comparés sur {}",
                ByteSize(progress.work_done),
                ByteSize(progress.work_total)
            ),
            Phase::Done | Phase::Cancelled => String::new(),
        };
        eprint!("\r{step:<78}");
        let _ = std::io::stderr().flush();
        std::thread::sleep(Duration::from_millis(100));
    }
    eprint!("\r{:<78}\r", "");

    let Some(report) = scan.wait() else {
        eprintln!("analyse annulée");
        return ExitCode::FAILURE;
    };
    if json {
        match serde_json::to_string_pretty(report.as_ref()) {
            Ok(text) => println!("{text}"),
            Err(error) => {
                eprintln!("le rapport n'a pas pu être écrit : {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        print_report(&report);
    }
    ExitCode::SUCCESS
}

fn print_report(report: &Report) {
    println!("Shinku · analyse en lecture seule\n");
    for volume in &report.volumes {
        println!(
            "{}  {} libres sur {}{}",
            to_display_path(&volume.root).display(),
            ByteSize(volume.free),
            ByteSize(volume.total),
            if volume.system { "  (Windows)" } else { "" }
        );
    }

    let summary = report.summary();
    println!("\nLe grand vide peut libérer {}", ByteSize(summary.auto_bytes));
    for category in &summary.categories {
        let name = match category.module {
            ModuleId::RecycleBin => "Corbeille",
            ModuleId::JunkFiles => "Fichiers inutiles",
            ModuleId::Duplicates => "Doublons",
            ModuleId::LargeFiles => "Gros fichiers (à vérifier)",
            ModuleId::Startup => "Démarrage (à vérifier)",
            _ => continue,
        };
        match category.state {
            CategoryState::Ready => println!(
                "  {name:<28}{:>10}  {} éléments",
                ByteSize(category.bytes).to_string(),
                category.items
            ),
            CategoryState::Pending => println!("  {name:<28}{:>10}", "en cours"),
            CategoryState::Unavailable => println!("  {name:<28}{:>10}", "pas encore"),
        }
    }

    println!("\nFichiers inutiles");
    for category in &report.junk.categories {
        if category.files == 0 {
            continue;
        }
        println!("  {:<28}{:>10}", category.label, ByteSize(category.bytes).to_string());
        for line in &category.lines {
            println!(
                "    {:<26}{:>10}  {} fichiers",
                line.label,
                ByteSize(line.bytes).to_string(),
                line.files
            );
        }
    }
    if report.junk.spared_files > 0 {
        println!(
            "  épargnés car récents : {} fichiers, {}",
            report.junk.spared_files,
            ByteSize(report.junk.spared_bytes)
        );
    }

    println!(
        "\nDoublons : {} copies en trop, {} ({} fichiers comparés, {} écartés, {} liens physiques)",
        report.duplicates.copies,
        ByteSize(report.duplicates.bytes),
        report.duplicates.candidates,
        report.duplicates.skipped_files,
        report.duplicates.hard_links
    );
    for group in report.duplicates.groups.iter().take(10) {
        println!("  {} × {}", group.files.len(), ByteSize(group.size));
        for file in &group.files {
            println!(
                "    {} {}{}",
                if file.keep { "garde" } else { "copie" },
                to_display_path(&file.path).display(),
                if file.synced { "  (synchronisé)" } else { "" }
            );
        }
    }

    println!(
        "\nGros fichiers : {} de plus de {}, {} en tout",
        report.large_files.count,
        ByteSize(report.large_files.threshold),
        ByteSize(report.large_files.bytes)
    );
    for file in report.large_files.files.iter().take(15) {
        println!(
            "  {:>10}  {}{}",
            ByteSize(file.on_disk).to_string(),
            to_display_path(&file.path).display(),
            if file.synced { "  (synchronisé)" } else { "" }
        );
    }

    let skipped = &report.skipped;
    if skipped.denied_folders + skipped.unreadable_folders + skipped.replaced_folders > 0 {
        println!(
            "\nNon analysé : {} dossiers interdits au compte, {} illisibles, {} remplacés en cours de route",
            skipped.denied_folders, skipped.unreadable_folders, skipped.replaced_folders
        );
        for example in &skipped.denied_examples {
            println!("  {}", to_display_path(example).display());
        }
    }
    for refused in &skipped.refused_roots {
        println!(
            "Refusé ({}) : {} — {}",
            refused.purpose,
            to_display_path(&refused.path).display(),
            refused.reason
        );
    }

    println!(
        "\n{} dossiers et {} fichiers vus. Parcours : {:.1} s. Total : {:.1} s.",
        report.stats.folders,
        report.stats.files,
        report.stats.walk_ms as f64 / 1000.0,
        report.stats.total_ms as f64 / 1000.0
    );
}
