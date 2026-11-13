//! Cœur de Shinku.
//!
//! Ce module contient tout ce qui décide : les règles, le moteur d'analyse et
//! le modèle de données. Il ne dessine rien et, à cette étape, il n'écrit rien
//! sur le disque de l'utilisateur : l'analyse est en lecture seule.
//!
//! Les trois exécutables (`shinku`, `shinku-helper`, `shinku-agent`) s'appuient
//! sur lui.

pub mod fs;
pub mod model;
pub mod platform;
pub mod scope;
