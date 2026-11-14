//! Cœur de Shinku.
//!
//! Ce module contient tout ce qui décide : les règles, le moteur d'analyse et
//! le modèle de données. Il ne dessine rien et, à cette étape, il n'écrit rien
//! sur le disque de l'utilisateur hormis ses propres réglages : l'analyse est
//! en lecture seule.
//!
//! Les trois exécutables (`shinku`, `shinku-helper`, `shinku-agent`) s'appuient
//! sur lui.

pub mod fs;
pub mod guard;
pub mod hash;
pub mod helper;
pub mod model;
pub mod platform;
pub mod rules;
pub mod scan;
pub mod scope;
pub mod settings;
pub mod walk;
