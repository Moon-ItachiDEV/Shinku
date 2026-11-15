//! Lie statiquement le runtime Visual C++ : l'assistant ne demande pas
//! `vcruntime140.dll` à l'ordinateur qui le reçoit. Sans effet ailleurs que
//! sur la chaîne d'outils MSVC.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    static_vcruntime::metabuild();
}
