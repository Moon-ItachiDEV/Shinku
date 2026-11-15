//! La valeur d'approbation : les douze octets que Windows lit pour savoir si
//! une entrée de démarrage est coupée.
//!
//! Ce format n'est pas documenté par Microsoft. C'est celui que le
//! Gestionnaire des tâches écrit, observé par des tiers, et les sources se
//! contredisent sur tout ce qui n'est ni `02` ni `03`. La lecture est donc
//! **stricte** : seules deux formes sont comprises, tout le reste est
//! « illisible » et Shinku n'y écrit jamais.
//!
//! La valeur est un `REG_BINARY` de douze octets : un état sur quatre octets,
//! puis un `FILETIME` de huit octets (petit-boutiste).
//!
//! - valeur absente : l'entrée est active ;
//! - `02 00 00 00` : active ;
//! - `03 00 00 00` : coupée, avec la date de la coupure ;
//! - tout le reste (`06`, `07`, une autre longueur, un autre type, une lecture
//!   refusée) : illisible.
//!
//! Shinku n'écrit que deux formes : [`cut_bytes`] et [`restore_bytes`].

use std::io;

use crate::startup::State;
use crate::startup::host::RawApproval;

/// Longueur de la valeur.
pub const LEN: usize = 12;
/// Le `FILETIME` du 1er janvier 1970.
const FILETIME_OF_UNIX_EPOCH: u64 = 116_444_736_000_000_000;
/// Le `FILETIME` du 1er janvier 2012 : avant, une date n'est pas plausible
/// (Windows 8, qui a introduit cet onglet, date de 2012).
const FILETIME_2012: u64 = FILETIME_OF_UNIX_EPOCH + 1_325_376_000 * 10_000_000;
/// Début d'une valeur « active ».
const ENABLED: [u8; 4] = [2, 0, 0, 0];
/// Début d'une valeur « coupée ».
const DISABLED: [u8; 4] = [3, 0, 0, 0];

/// Ce que dit une valeur d'approbation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Approval {
    /// L'état de l'entrée.
    pub state: State,
    /// Le `FILETIME` lu dans une valeur « coupée », tel quel (0 sinon).
    pub file_time: u64,
    /// La date de la coupure, en millisecondes depuis 1970, si elle est
    /// plausible : entre 2012 et maintenant.
    pub disabled_ms: Option<i64>,
    /// Vrai si une valeur existe dans le registre (même illisible). Une entrée
    /// active **sans** valeur n'a jamais été réglée, ou l'app a effacé la
    /// valeur en se réinscrivant ; avec `02 00 00 00`, quelqu'un l'a réglée.
    pub present: bool,
}

impl Approval {
    const fn of(state: State) -> Self {
        Self { state, file_time: 0, disabled_ms: None, present: false }
    }

    /// Un réglage qu'on ne lit pas : on n'y touche pas, on ne le compte pas.
    pub const fn unreadable() -> Self {
        Self::of(State::Unknown)
    }
}

/// Lit ce que le registre a donné. `now` est le `FILETIME` de maintenant.
pub fn read(raw: &io::Result<Option<RawApproval>>, now: u64) -> Approval {
    match raw {
        Ok(None) => Approval::of(State::Enabled),
        Ok(Some(value)) => Approval { present: true, ..read_value(value, now) },
        Err(_) => Approval::of(State::Unknown),
    }
}

fn read_value(value: &RawApproval, now: u64) -> Approval {
    if !value.is_binary || value.bytes.len() != LEN {
        return Approval::of(State::Unknown);
    }
    let (head, tail) = value.bytes.split_at(4);
    if head == ENABLED {
        return Approval::of(State::Enabled);
    }
    if head == DISABLED {
        let mut time = [0u8; 8];
        time.copy_from_slice(tail);
        let file_time = u64::from_le_bytes(time);
        let plausible = (FILETIME_2012..=now).contains(&file_time);
        return Approval {
            state: State::Disabled,
            file_time,
            disabled_ms: plausible.then(|| millis_of(file_time)),
            present: true,
        };
    }
    Approval::of(State::Unknown)
}

/// Un `FILETIME` plausible, en millisecondes depuis 1970.
fn millis_of(file_time: u64) -> i64 {
    i64::try_from((file_time - FILETIME_OF_UNIX_EPOCH) / 10_000).unwrap_or(i64::MAX)
}

/// La valeur qui coupe : `03 00 00 00` et la date de maintenant.
pub fn cut_bytes(file_time: u64) -> [u8; LEN] {
    let mut bytes = [0u8; LEN];
    bytes[..4].copy_from_slice(&DISABLED);
    bytes[4..].copy_from_slice(&file_time.to_le_bytes());
    bytes
}

/// La valeur qui remet : `02 00 00 00` et huit zéros.
pub fn restore_bytes() -> [u8; LEN] {
    let mut bytes = [0u8; LEN];
    bytes[..4].copy_from_slice(&ENABLED);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le 15 novembre 2026, en `FILETIME`.
    const NOW: u64 = FILETIME_OF_UNIX_EPOCH + 1_794_700_000 * 10_000_000;

    fn binary(bytes: &[u8]) -> io::Result<Option<RawApproval>> {
        Ok(Some(RawApproval { is_binary: true, bytes: bytes.to_vec() }))
    }

    fn with_head(head: [u8; 4], time: u64) -> Vec<u8> {
        let mut bytes = head.to_vec();
        bytes.extend_from_slice(&time.to_le_bytes());
        bytes
    }

    #[test]
    fn an_absent_value_is_enabled() {
        assert_eq!(read(&Ok(None), NOW), Approval::of(State::Enabled));
    }

    #[test]
    fn the_enabled_form_is_enabled_whatever_the_date_says() {
        assert_eq!(read(&binary(&restore_bytes()), NOW).state, State::Enabled);
        let with_date = with_head([2, 0, 0, 0], NOW - 5);
        assert_eq!(
            read(&binary(&with_date), NOW),
            Approval { present: true, ..Approval::of(State::Enabled) }
        );
    }

    #[test]
    fn only_a_real_value_is_present() {
        assert!(!read(&Ok(None), NOW).present, "valeur absente");
        assert!(read(&binary(&restore_bytes()), NOW).present, "02 : quelqu'un l'a réglée");
        assert!(read(&binary(&cut_bytes(NOW)), NOW).present);
        assert!(read(&binary(&[6; 12]), NOW).present, "même illisible, elle existe");
        assert_eq!(Approval::unreadable().state, State::Unknown);
    }

    #[test]
    fn the_disabled_form_carries_its_date() {
        let when = FILETIME_OF_UNIX_EPOCH + 1_763_230_000 * 10_000_000;
        let read = read(&binary(&cut_bytes(when)), NOW);
        assert_eq!(read.state, State::Disabled);
        assert_eq!(read.file_time, when);
        assert_eq!(read.disabled_ms, Some(1_763_230_000_000));
    }

    #[test]
    fn an_implausible_date_is_kept_for_comparison_but_not_shown() {
        for time in [0, 1, FILETIME_2012 - 1, NOW + 1, u64::MAX] {
            let read = read(&binary(&cut_bytes(time)), NOW);
            assert_eq!(read.state, State::Disabled, "{time}");
            assert_eq!(read.file_time, time);
            assert_eq!(read.disabled_ms, None, "{time}");
        }
        // Les bornes elles-mêmes sont plausibles.
        assert!(read(&binary(&cut_bytes(FILETIME_2012)), NOW).disabled_ms.is_some());
        assert!(read(&binary(&cut_bytes(NOW)), NOW).disabled_ms.is_some());
    }

    #[test]
    fn every_other_first_word_is_unreadable() {
        // `06` et `07` se voient dans la nature : les sources se contredisent
        // sur leur sens, Shinku ne les interprète pas.
        for first in [0u8, 1, 4, 5, 6, 7, 8, 0x0a, 0x0b, 0xff] {
            let value = with_head([first, 0, 0, 0], 0);
            assert_eq!(read(&binary(&value), NOW).state, State::Unknown, "{first:#x}");
        }
        // Les quatre octets comptent tous.
        for head in [[2, 1, 0, 0], [3, 0, 1, 0], [3, 0, 0, 1], [2, 0, 0, 0x80], [0x82, 0, 0, 0]] {
            let value = with_head(head, 0);
            let state = read(&binary(&value), NOW).state;
            assert_eq!(state, State::Unknown, "{head:?}");
        }
    }

    #[test]
    fn any_other_length_is_unreadable() {
        for length in [0usize, 1, 4, 8, 11, 13, 16, 64] {
            for first in [2u8, 3] {
                let mut bytes = vec![0u8; length];
                if let Some(byte) = bytes.first_mut() {
                    *byte = first;
                }
                assert_eq!(read(&binary(&bytes), NOW).state, State::Unknown, "{length} {first}");
            }
        }
    }

    #[test]
    fn any_other_type_is_unreadable() {
        // La bonne longueur, la bonne forme, mais pas du binaire (REG_SZ, REG_DWORD…).
        let text = RawApproval { is_binary: false, bytes: cut_bytes(NOW).to_vec() };
        assert_eq!(read(&Ok(Some(text)), NOW).state, State::Unknown);
        let number = RawApproval { is_binary: false, bytes: vec![3, 0, 0, 0] };
        assert_eq!(read(&Ok(Some(number)), NOW).state, State::Unknown);
    }

    #[test]
    fn a_refused_read_is_unreadable() {
        let refused = Err(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(read(&refused, NOW), Approval::of(State::Unknown));
    }

    #[test]
    fn the_two_forms_written_by_shinku_read_back_as_wanted() {
        let cut = cut_bytes(NOW);
        assert_eq!(&cut[..4], [3, 0, 0, 0]);
        assert_eq!(u64::from_le_bytes(cut[4..].try_into().unwrap()), NOW);
        assert_eq!(read(&binary(&cut), NOW + 10).state, State::Disabled);
        let restored = restore_bytes();
        assert_eq!(restored, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(read(&binary(&restored), NOW).state, State::Enabled);
    }
}
