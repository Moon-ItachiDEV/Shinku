//! Prépare `shinku-helper.exe`, l'assistant élevé : runtime Visual C++ lié
//! statiquement, ressource de version (« Assistant de Shinku », icône de
//! Shinku), manifeste (`requireAdministrator`), et, sur MSVC,
//! `/DEPENDENTLOADFLAG:0x800` (ne charger de bibliothèque que depuis `System32`).
//!
//! Contrairement à l'agent, **la construction échoue** si la ressource ne
//! s'écrit pas : un assistant sans son manifeste ne demanderait pas
//! l'élévation, et sans sa ressource Windows ne saurait pas le nommer dans sa
//! fenêtre d'accord.

/// Le manifeste de l'assistant. Les explications sont ici, pas dans le fichier :
/// `tauri-winres` le recopie ligne par ligne dans un fichier de ressources, ce
/// qui refuse la déclaration `<?xml ?>`, les accents et les apostrophes (voir
/// `check_manifest`).
///
/// - `requireAdministrator` : Windows demande l'accord avant de lancer (il est
///   ignoré quand le contrôle de compte est coupé : l'assistant vérifie lui-même
///   son jeton) ;
/// - `uiAccess="false"` ;
/// - `supportedOS` : Windows 10 et 11 ;
/// - `PerMonitorV2` : la boîte est nette sur chaque écran.
const MANIFEST: &str = include_str!("windows-manifest.xml");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=windows-manifest.xml");
    static_vcruntime::metabuild();

    if let Err(problem) = check_manifest(MANIFEST) {
        panic!("windows-manifest.xml : {problem}");
    }

    // Shinku ne vise que Windows : ailleurs, il n'y a pas de ressource à écrire.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
    let mut resource = tauri_winres::WindowsResource::new();
    resource
        .set("FileDescription", "Assistant de Shinku")
        .set("ProductName", "Shinku")
        .set("InternalName", "shinku-helper")
        .set("OriginalFilename", "shinku-helper.exe")
        .set_icon("../../apps/shinku/src-tauri/icons/icon.ico")
        .set_manifest(MANIFEST);
    println!("cargo:rerun-if-changed=../../apps/shinku/src-tauri/icons/icon.ico");
    if let Err(error) = resource.compile() {
        panic!("la ressource Windows de l'assistant ne s'écrit pas : {error}");
    }
}

/// Refuse un manifeste que son incorporation abîmerait (voir `MANIFEST`).
fn check_manifest(manifest: &str) -> Result<(), &'static str> {
    if manifest.contains("<?xml") {
        return Err("pas de déclaration <?xml … ?> : Windows refuserait de lancer l'assistant");
    }
    if !manifest.is_ascii() {
        return Err("que des caractères ASCII");
    }
    if manifest.contains('\'') {
        return Err("pas d'apostrophe : les attributs s'écrivent entre guillemets");
    }
    if !manifest.contains("requireAdministrator") {
        return Err("l'assistant demande l'élévation : requireAdministrator");
    }
    Ok(())
}
