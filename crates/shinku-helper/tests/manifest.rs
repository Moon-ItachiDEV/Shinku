//! Le manifeste de l'assistant : ce que Windows lit avant de le lancer.

const MANIFEST: &str = include_str!("../windows-manifest.xml");

#[test]
fn the_manifest_asks_for_the_administrator_level_without_ui_access() {
    assert!(
        MANIFEST.contains(
            r#"<requestedExecutionLevel level="requireAdministrator" uiAccess="false" />"#
        )
    );
    assert!(!MANIFEST.contains("asInvoker") && !MANIFEST.contains("highestAvailable"));
}

#[test]
fn the_manifest_survives_the_resource_compiler() {
    // `tauri-winres` recopie le manifeste ligne par ligne : voir build.rs.
    assert!(MANIFEST.is_ascii());
    assert!(!MANIFEST.contains("<?xml"));
    assert!(!MANIFEST.contains('\''));
}

#[test]
fn the_manifest_names_windows_10_and_11_and_per_monitor_dpi() {
    assert!(MANIFEST.contains("{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"));
    assert!(MANIFEST.contains("PerMonitorV2"));
}

#[test]
fn the_helper_has_no_console_and_no_verbs() {
    let source = include_str!("../src/main.rs");
    // Les commentaires peuvent nommer ce qui n'existe pas.
    let main: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(main.contains("#![windows_subsystem = \"windows\"]"));
    assert!(
        !main.contains("--verbes") && !main.contains("--version") && !main.contains("println!")
    );
    // Un durcissement refusé fait sortir avec le code 8 (`Code::Failed`).
    assert!(main.contains("if !shinku_core::platform::harden_helper_process() {"));
    let after = &main[main.find("if !shinku_core::platform::harden_helper_process() {").unwrap()..];
    assert!(after.lines().nth(1).is_some_and(|line| line.contains("Code::Failed as u8")));
    // Le durcissement est la première chose que fait `main`.
    let body = &main[main.find("fn main()").unwrap()..];
    let first = body
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty() && !line.trim().starts_with("//"))
        .unwrap();
    assert!(first.contains("harden_helper_process"), "{first}");
}
