//! Le mode jeu : reconnaître un jeu, adapter le PC pendant la partie, tout
//! remettre ensuite.
//!
//! Shinku ne lance jamais un jeu. Un petit programme d'arrière-plan
//! (`shinku-agent.exe`) regarde la fenêtre au premier plan ; quand un jeu
//! qu'il reconnaît y reste, il applique les réglages de la personne (mode
//! d'alimentation, apps d'arrière-plan en veilleuse), puis les remet en place
//! quand la partie se termine.
//!
//! Toute la décision est ici et ne touche pas à Windows : elle parle à la
//! machine par le trait [`host::GameHost`], remplacé dans les tests par une
//! fausse machine ([`host::FakeHost`]).
//!
//! - [`detect`] : ce qui est un jeu, et ce qui n'en est jamais un ;
//! - [`apps`] : les apps qu'on peut mettre en veilleuse, et celles qu'on ne
//!   touche jamais ;
//! - [`store`] : `game.json`, le journal de la partie en cours, de quoi tout
//!   remettre en place même après une coupure de courant ;
//! - [`host`] : la machine vue par le mode jeu ;
//! - [`watch`] : la machine à états, un pas toutes les deux secondes ;
//! - [`agent`] : la boucle de l'agent, sans Windows ;
//! - [`control`] : ce que la fenêtre peut demander à l'agent ;
//! - [`view`] et [`desk`] : ce que la page « Mode jeu » reçoit et demande.

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub mod agent;
pub mod apps;
pub mod control;
pub mod desk;
pub mod detect;
pub mod host;
pub mod store;
pub mod view;
pub mod watch;

/// Durée d'un pas de l'agent.
pub const STEP: Duration = Duration::from_secs(2);

/// Ce que le mode jeu a fait de l'alimentation pendant une partie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerOutcome {
    /// « Performances élevées » est en place, posé par Shinku.
    Applied,
    /// Windows était déjà en « Performances élevées » : rien à faire.
    AlreadyFast,
    /// Sur batterie : Shinku n'y touche pas.
    OnBattery,
    /// Ce PC n'a pas le mode « Performances élevées ».
    Unavailable,
    /// Ce processeur est mieux servi par le mode normal de Windows.
    Unsuited,
    /// Le mode actif est celui du constructeur ou un mode personnalisé : Shinku n'y touche pas.
    Custom,
    /// Windows a refusé le changement.
    Refused,
    /// Le réglage est éteint.
    Off,
}

/// Les processeurs pour lesquels AMD demande de garder le mode normal de Windows.
const UNSUITED_PROCESSORS: [&str; 4] = ["7900X3D", "7950X3D", "9900X3D", "9950X3D"];

/// Vrai si ce processeur est mieux servi par le mode d'alimentation normal :
/// AMD le demande pour ses puces à double cache. Le nom est celui que Windows
/// donne (« AMD Ryzen 9 7950X3D 16-Core Processor »).
pub fn unsuited_processor(name: &str) -> bool {
    let name = name.to_uppercase();
    UNSUITED_PROCESSORS.iter().any(|model| name.contains(model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_cache_amd_processors_keep_the_normal_power_mode() {
        assert!(unsuited_processor("AMD Ryzen 9 7950X3D 16-Core Processor"));
        assert!(unsuited_processor("amd ryzen 9 7900x3d"));
        assert!(unsuited_processor("AMD Ryzen 9 9950X3D 16-Core Processor"));
        assert!(unsuited_processor("AMD Ryzen 9 9900X3D 12-Core Processor"));
        // Les autres puces à gros cache n'ont pas cette recommandation.
        assert!(!unsuited_processor("AMD Ryzen 7 7800X3D 8-Core Processor"));
        assert!(!unsuited_processor("Intel(R) Core(TM) i7-13700K"));
        assert!(!unsuited_processor(""));
    }

    #[test]
    fn power_outcomes_travel_in_camel_case() {
        let json = serde_json::to_value(PowerOutcome::AlreadyFast).unwrap();
        assert_eq!(json, "alreadyFast");
        let back: PowerOutcome = serde_json::from_value(json).unwrap();
        assert_eq!(back, PowerOutcome::AlreadyFast);
    }
}
