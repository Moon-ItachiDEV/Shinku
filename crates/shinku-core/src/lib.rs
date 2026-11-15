//! Cœur de Shinku.
//!
//! Ce module contient tout ce qui décide : les règles, le moteur d'analyse,
//! le grand vide et le modèle de données. Il ne dessine rien.
//!
//! L'analyse ([`scan`]) est en lecture seule. Seul le grand vide ([`clean`])
//! supprime, et seulement ce que l'analyse a montré. En dehors de lui, le
//! cœur n'écrit que ses propres fichiers : les réglages et le journal.
//!
//! Les trois exécutables (`shinku`, `shinku-helper`, `shinku-agent`) s'appuient
//! sur lui.

pub mod backend;
pub mod clean;
pub mod fs;
pub mod game;
pub mod guard;
pub mod hash;
pub mod helper;
pub mod model;
pub mod platform;
pub mod rules;
pub mod scan;
pub mod scope;
pub mod session;
pub mod settings;
#[cfg(test)]
mod testing;
pub mod walk;
