//! La **décision** de la garde de la corbeille, sans Windows : elle prend les
//! réglages déjà lus, la taille de ce qui part et la taille du volume, et rend
//! le verdict. Lire les réglages est l'affaire de `windows/shell.rs` ; décider
//! se juge ici, sur toutes les machines.
//!
//! Pourquoi une garde : quand la corbeille ne peut pas recevoir un élément,
//! Windows le supprime pour de bon, et seulement *après* coup on le voit
//! (`Sent::Destroyed`). Cette garde est la seule barrière qui agit **avant**.
//! Elle se trompe donc du côté sûr : une marge, et un refus quand elle ne sait
//! pas.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use super::RecycleError;

/// Les réglages de la corbeille, tels qu'ils se lisent. Toutes les valeurs sont
/// optionnelles : absente ou illisible, c'est `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BinSettings {
    /// `NukeOnDelete` global du compte (`…\Explorer\BitBucket`).
    pub nuke_global: Option<u32>,
    /// `NukeOnDelete` de ce volume (`…\BitBucket\Volume\{GUID}`).
    pub nuke_volume: Option<u32>,
    /// La stratégie `NoRecycleFiles` (`…\Policies\Explorer`, compte puis machine).
    pub policy_no_recycle: Option<u32>,
    /// `MaxCapacity` de ce volume, **en Mio**.
    pub max_capacity_mib: Option<u32>,
}

/// Part de `MaxCapacity` qu'un élément ne doit pas dépasser (pour cent).
const CAPACITY_MARGIN: u128 = 90;
/// Sans `MaxCapacity`, part du volume qu'un élément ne doit pas dépasser (pour cent).
const VOLUME_SHARE: u128 = 5;

/// Le verdict : `Ok` si l'envoi peut être demandé au shell, `Refused` sinon.
///
/// - un `NukeOnDelete` (global ou du volume) ou un `NoRecycleFiles` non nul : refus ;
/// - capacité connue : refus au-delà de 90 % de `MaxCapacity` ;
/// - capacité inconnue : refus au-delà de 5 % de la taille du volume, et refus
///   si la taille du volume est inconnue aussi.
///
/// Les calculs se font en `u128` : rien ne déborde.
pub(crate) fn judge(
    settings: &BinSettings,
    size: u64,
    volume_total: Option<u64>,
) -> Result<(), RecycleError> {
    // Toute valeur non nulle compte : on ne parie pas sur la façon dont Windows
    // lit un 2.
    let on = |value: Option<u32>| value.is_some_and(|value| value != 0);
    if on(settings.nuke_global) || on(settings.nuke_volume) || on(settings.policy_no_recycle) {
        return Err(RecycleError::Refused);
    }
    let size = u128::from(size);
    let fits = match (settings.max_capacity_mib, volume_total) {
        (Some(mebibytes), _) => size * 100 <= u128::from(mebibytes) * 1024 * 1024 * CAPACITY_MARGIN,
        (None, Some(total)) => size * 100 <= u128::from(total) * VOLUME_SHARE,
        (None, None) => false,
    };
    if fits { Ok(()) } else { Err(RecycleError::Refused) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn capacity(mebibytes: u32) -> BinSettings {
        BinSettings { max_capacity_mib: Some(mebibytes), ..BinSettings::default() }
    }

    fn refused(verdict: Result<(), RecycleError>) -> bool {
        matches!(verdict, Err(RecycleError::Refused))
    }

    #[test]
    fn each_nuke_setting_refuses_by_itself() {
        let base = capacity(1000);
        for settings in [
            BinSettings { nuke_global: Some(1), ..base },
            BinSettings { nuke_volume: Some(1), ..base },
            BinSettings { policy_no_recycle: Some(1), ..base },
            // Toute valeur non nulle refuse, pas seulement 1.
            BinSettings { nuke_global: Some(2), ..base },
            BinSettings { nuke_volume: Some(u32::MAX), ..base },
            BinSettings { policy_no_recycle: Some(2), ..base },
        ] {
            assert!(refused(judge(&settings, 1, Some(1000 * MIB))), "{settings:?}");
        }
        // À 0 ou absents : rien n'est refusé.
        let off = BinSettings {
            nuke_global: Some(0),
            nuke_volume: Some(0),
            policy_no_recycle: Some(0),
            ..base
        };
        assert!(judge(&off, 1, Some(1000 * MIB)).is_ok());
        assert!(judge(&base, 1, None).is_ok());
    }

    #[test]
    fn the_capacity_is_in_mebibytes_with_a_margin_of_ten_percent() {
        // 100 Mio : 90 Mio passent, un octet de plus non.
        assert!(judge(&capacity(100), 90 * MIB, None).is_ok());
        assert!(refused(judge(&capacity(100), 90 * MIB + 1, None)));
        // Pas des Kio : 1 Mio dans 8 Mio passe.
        assert!(judge(&capacity(8), MIB, None).is_ok());
        // Pas la capacité entière : 95 % sont refusés.
        assert!(refused(judge(&capacity(100), 95 * MIB, None)));
        // Une capacité à 0 ne reçoit rien, sauf un élément vide.
        assert!(refused(judge(&capacity(0), 1, Some(u64::MAX))));
        assert!(judge(&capacity(0), 0, None).is_ok());
    }

    #[test]
    fn a_known_capacity_wins_over_the_size_of_the_volume() {
        // Plus grand que 5 % du volume, mais dans la capacité : accepté.
        assert!(judge(&capacity(1000), 500 * MIB, Some(1000 * MIB)).is_ok());
    }

    #[test]
    fn without_a_capacity_the_limit_is_five_percent_of_the_volume() {
        let none = BinSettings::default();
        assert!(judge(&none, 5 * MIB, Some(100 * MIB)).is_ok());
        assert!(refused(judge(&none, 5 * MIB + 1, Some(100 * MIB))));
        // La taille du volume est inconnue aussi : refus.
        assert!(refused(judge(&none, 1, None)));
        assert!(refused(judge(&none, 0, None)));
    }

    #[test]
    fn nothing_overflows() {
        let none = BinSettings::default();
        assert!(refused(judge(&none, u64::MAX, Some(u64::MAX / 100))));
        assert!(judge(&none, u64::MAX / 20, Some(u64::MAX)).is_ok());
        assert!(refused(judge(&capacity(u32::MAX), u64::MAX, None)));
        assert!(judge(&capacity(u32::MAX), 1 << 50, None).is_ok());
    }
}
