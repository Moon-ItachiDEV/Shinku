//! Prépare `shinku-agent.exe` : runtime Visual C++ lié statiquement (rien à
//! installer sur le PC qui le reçoit) et ressource de version, pour que
//! Windows l'appelle « Shinku » dans ses réglages de démarrage.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    static_vcruntime::metabuild();

    // Shinku ne vise que Windows : ailleurs, il n'y a pas de ressource à écrire.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    let mut resource = tauri_winres::WindowsResource::new();
    resource
        .set("FileDescription", "Shinku")
        .set("ProductName", "Shinku")
        .set("InternalName", "shinku-agent")
        .set("OriginalFilename", "shinku-agent.exe")
        .set_icon("../../apps/shinku/src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed=../../apps/shinku/src-tauri/icons/icon.ico");
    if let Err(error) = resource.compile() {
        // Une ressource manquante n'empêche pas l'agent de fonctionner : on le dit, sans bloquer.
        println!("cargo:warning=ressource Windows non écrite : {error}");
    }
}
