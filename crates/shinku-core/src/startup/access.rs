//! Qui peut régler les entrées du PC, et comment : une décision **pure** à
//! partir de ce que l'hôte lit.
//!
//! | Situation | [`PcAccess`] |
//! | --- | --- |
//! | assistant absent ou emplacement non sûr | `unavailable` |
//! | jeton limité d'une paire (`TokenElevationTypeLimited`) | `consent` |
//! | Administrateurs activé dans le jeton (quel que soit le type) | `direct` (Windows ne demandera rien) |
//! | sinon, `EnableLUA` = 0 ou `ConsentPromptBehaviorUser` = 0 | `unavailable` |
//! | sinon (y compris un jeton « complet » filtré, sans Administrateurs) | `password` |
//!
//! L'ordre est celui du tableau.

use serde::Serialize;

/// Ce que Shinku peut faire pour les entrées du PC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PcAccess {
    /// Windows demandera un accord (un clic) à chaque changement.
    Consent,
    /// Windows demandera le mot de passe d'un administrateur.
    Password,
    /// Windows ne demandera rien : seule la boîte de l'assistant questionne.
    Direct,
    /// Shinku ne sait pas régler les entrées du PC ici.
    Unavailable,
}

/// Le type d'élévation du jeton de Shinku (`TokenElevationType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElevationType {
    /// Jeton simple : ni paire, ni élévation.
    Default,
    /// Le jeton complet d'une paire.
    Full,
    /// Le jeton limité d'une paire.
    Limited,
}

/// Ce que l'hôte a lu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessFacts {
    /// L'assistant est présent et son emplacement est sûr.
    pub helper_safe: bool,
    /// Le type d'élévation du jeton.
    pub elevation: ElevationType,
    /// Le groupe Administrateurs est activé dans le jeton.
    pub admin_enabled: bool,
    /// `EnableLUA` (`None` : absente).
    pub enable_lua: Option<u32>,
    /// `ConsentPromptBehaviorUser` (`None` : absente).
    pub consent_user: Option<u32>,
}

/// La décision.
pub fn decide(facts: &AccessFacts) -> PcAccess {
    if !facts.helper_safe {
        return PcAccess::Unavailable;
    }
    if facts.elevation == ElevationType::Limited {
        return PcAccess::Consent;
    }
    if facts.admin_enabled {
        return PcAccess::Direct;
    }
    if facts.enable_lua == Some(0) || facts.consent_user == Some(0) {
        return PcAccess::Unavailable;
    }
    PcAccess::Password
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(elevation: ElevationType, admin: bool) -> AccessFacts {
        AccessFacts {
            helper_safe: true,
            elevation,
            admin_enabled: admin,
            enable_lua: Some(1),
            consent_user: Some(3),
        }
    }

    #[test]
    fn an_absent_or_unsafe_helper_is_unavailable_whatever_the_token() {
        for elevation in [ElevationType::Default, ElevationType::Full, ElevationType::Limited] {
            for admin in [false, true] {
                let mut facts = facts(elevation, admin);
                facts.helper_safe = false;
                assert_eq!(decide(&facts), PcAccess::Unavailable);
            }
        }
    }

    #[test]
    fn a_limited_token_of_a_pair_asks_for_consent() {
        assert_eq!(decide(&facts(ElevationType::Limited, false)), PcAccess::Consent);
        // Même avec EnableLUA à 0 dans une lecture bizarre, le jeton limité parle.
        let mut odd = facts(ElevationType::Limited, false);
        odd.enable_lua = Some(0);
        assert_eq!(decide(&odd), PcAccess::Consent);
    }

    #[test]
    fn an_enabled_administrators_group_is_direct_and_a_full_token_without_it_is_not() {
        assert_eq!(decide(&facts(ElevationType::Full, true)), PcAccess::Direct);
        assert_eq!(decide(&facts(ElevationType::Default, true)), PcAccess::Direct);
        // Un jeton « complet » filtré (Administrateurs en refus seul) : Windows
        // demandera un mot de passe, pas « rien ».
        assert_eq!(decide(&facts(ElevationType::Full, false)), PcAccess::Password);
        let mut no_uac = facts(ElevationType::Full, false);
        no_uac.enable_lua = Some(0);
        assert_eq!(decide(&no_uac), PcAccess::Unavailable);
        // Administrateurs activé passe avant EnableLUA.
        let mut admin = facts(ElevationType::Default, true);
        admin.enable_lua = Some(0);
        assert_eq!(decide(&admin), PcAccess::Direct);
    }

    #[test]
    fn a_plain_token_without_rights_asks_for_a_password_unless_windows_never_asks() {
        assert_eq!(decide(&facts(ElevationType::Default, false)), PcAccess::Password);
        let mut absent = facts(ElevationType::Default, false);
        absent.enable_lua = None;
        absent.consent_user = None;
        assert_eq!(decide(&absent), PcAccess::Password);
        let mut no_uac = facts(ElevationType::Default, false);
        no_uac.enable_lua = Some(0);
        assert_eq!(decide(&no_uac), PcAccess::Unavailable);
        let mut no_prompt = facts(ElevationType::Default, false);
        no_prompt.consent_user = Some(0);
        assert_eq!(decide(&no_prompt), PcAccess::Unavailable);
        // Une autre valeur (1, 3) demande bien un mot de passe.
        for value in [1, 3] {
            let mut other = facts(ElevationType::Default, false);
            other.consent_user = Some(value);
            assert_eq!(decide(&other), PcAccess::Password);
        }
    }

    #[test]
    fn the_names_the_page_expects() {
        let json = |access| serde_json::to_value(access).unwrap();
        assert_eq!(json(PcAccess::Consent), "consent");
        assert_eq!(json(PcAccess::Password), "password");
        assert_eq!(json(PcAccess::Direct), "direct");
        assert_eq!(json(PcAccess::Unavailable), "unavailable");
    }
}
