//! Où l'assistant élevé a le droit de se trouver, et quels objets sont « sûrs ».
//!
//! **Tout est pur** : la lecture des descripteurs de sécurité est dans
//! `platform/windows/`, la décision est ici, essayée sur toutes les machines.
//!
//! Ce sont des garde-fous contre **nos propres erreurs** (élever un fichier
//! qu'un programme sans droits peut remplacer), pas une barrière contre un
//! programme du compte : voir l'en-tête de [`crate::helper`].
//!
//! Un objet (dossier ou fichier) est **sûr** si :
//!
//! - son propriétaire est Administrateurs, SYSTEM ou TrustedInstaller ;
//! - il a une liste de droits (pas de liste nulle) ;
//! - aucune entrée **d'autorisation** ne donne un droit d'écriture à un autre
//!   SID que ces trois-là.
//!
//! Règles de lecture : une entrée en **héritage seul** est ignorée pour l'objet ;
//! `CREATOR OWNER` est ignoré ; `OWNER RIGHTS` est un SID comme un autre ; les
//! entrées de refus sont ignorées ; tout **type d'entrée inconnu** (conditionnelle,
//! d'objet…) rend l'objet pas sûr.

/// Le groupe Administrateurs.
pub const ADMINISTRATORS: &str = "S-1-5-32-544";
/// Le compte SYSTEM.
pub const SYSTEM: &str = "S-1-5-18";
/// TrustedInstaller.
pub const TRUSTED_INSTALLER: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
/// `CREATOR OWNER` : ignoré (c'est un modèle d'héritage, pas une personne).
pub const CREATOR_OWNER: &str = "S-1-3-0";

/// Les droits d'écriture : `FILE_WRITE_DATA`, `FILE_APPEND_DATA`,
/// `FILE_WRITE_EA`, `FILE_DELETE_CHILD`, `FILE_WRITE_ATTRIBUTES`, `DELETE`,
/// `WRITE_DAC`, `WRITE_OWNER`, `GENERIC_WRITE`, `GENERIC_ALL`.
pub const WRITE_RIGHTS: u32 = 0x0002
    | 0x0004
    | 0x0010
    | 0x0040
    | 0x0100
    | 0x0001_0000
    | 0x0004_0000
    | 0x0008_0000
    | 0x4000_0000
    | 0x1000_0000;

/// Le nom du fichier de l'assistant.
pub const HELPER_FILE: &str = "shinku-helper.exe";

/// Le genre d'une entrée de la liste de droits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AceKind {
    /// Une autorisation simple.
    Allow,
    /// Un refus simple.
    Deny,
    /// Tout le reste (conditionnelle, d'objet, d'audit…) : inconnu.
    Other,
}

/// Une entrée de la liste de droits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ace {
    /// Son genre.
    pub kind: AceKind,
    /// Le SID concerné, en texte (`S-1-5-…`).
    pub sid: String,
    /// Le masque de droits.
    pub mask: u32,
    /// Vrai si l'entrée ne s'applique qu'aux enfants (héritage seul).
    pub inherit_only: bool,
}

/// Ce que dit le descripteur de sécurité d'un objet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Security {
    /// Le SID du propriétaire (`None` : illisible).
    pub owner: Option<String>,
    /// La liste de droits (`None` : liste nulle, ou illisible).
    pub acl: Option<Vec<Ace>>,
}

fn is_trusted(sid: &str) -> bool {
    [ADMINISTRATORS, SYSTEM, TRUSTED_INSTALLER]
        .iter()
        .any(|trusted| trusted.eq_ignore_ascii_case(sid))
}

/// L'objet est-il sûr ? Voir l'en-tête du module.
pub fn is_safe(security: &Security) -> bool {
    let Some(owner) = security.owner.as_deref() else { return false };
    if !is_trusted(owner) {
        return false;
    }
    let Some(acl) = &security.acl else { return false };
    acl.iter().all(|ace| match ace.kind {
        AceKind::Other => false,
        AceKind::Deny => true,
        AceKind::Allow => {
            ace.inherit_only
                || ace.sid.eq_ignore_ascii_case(CREATOR_OWNER)
                || is_trusted(&ace.sid)
                || ace.mask & WRITE_RIGHTS == 0
        }
    })
}

/// Les composants d'un chemin **final** (`\\?\C:\Program Files\X`), sans le
/// préfixe `\\?\` et sans barre finale. Pas de pliage ici.
fn components(path: &str) -> Vec<&str> {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    path.split(['\\', '/']).filter(|part| !part.is_empty()).collect()
}

fn same_part(left: &str, right: &str) -> bool {
    // Casse pliée en ASCII seulement : un nom non ASCII doit être identique.
    left.eq_ignore_ascii_case(right)
}

/// `child` est-il l'enfant **direct** de `parent` ? Chemins finaux, comparés
/// **par composants** : `C:\Program Files Evil\X` n'est pas dans
/// `C:\Program Files`.
pub fn is_direct_child(child: &str, parent: &str) -> bool {
    let (child, parent) = (components(child), components(parent));
    !parent.is_empty()
        && child.len() == parent.len() + 1
        && child.iter().zip(&parent).all(|(left, right)| same_part(left, right))
}

/// Le dernier composant d'un chemin.
pub fn file_name(path: &str) -> Option<&str> {
    components(path).last().copied()
}

/// Le chemin sans son dernier composant, sous la même forme de comparaison.
fn parent_of(path: &str) -> Option<String> {
    let parts = components(path);
    (parts.len() >= 2).then(|| parts[..parts.len() - 1].join("\\"))
}

/// Un objet ouvert : son chemin final, ses droits, et s'il est un point d'analyse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    /// Le chemin final (`GetFinalPathNameByHandleW`).
    pub path: String,
    /// Ses droits.
    pub security: Security,
    /// Vrai si c'est un point d'analyse (lien, jonction).
    pub reparse: bool,
}

/// L'assistant est-il bien placé ? Le fichier s'appelle `shinku-helper.exe`,
/// n'est pas un point d'analyse, et est sûr ; son dossier est l'enfant direct
/// d'un dossier `Program Files` ; ce dossier et `Program Files` lui-même sont
/// sûrs. Les chemins sont finaux, comparés par composants.
pub fn helper_is_well_placed(helper: &Placed, folder: &Placed, program_files: &[Placed]) -> bool {
    file_name(&helper.path).is_some_and(|name| same_part(name, HELPER_FILE))
        && !helper.reparse
        && is_safe(&helper.security)
        && parent_of(&helper.path)
            .is_some_and(|parent| components(&parent) == components(&folder.path))
        && !folder.reparse
        && is_safe(&folder.security)
        && program_files.iter().any(|files| {
            !files.reparse && is_direct_child(&folder.path, &files.path) && is_safe(&files.security)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const USERS: &str = "S-1-5-32-545";
    const AUTHENTICATED: &str = "S-1-5-11";
    const READ: u32 = 0x0012_0089;

    fn ace(sid: &str, mask: u32) -> Ace {
        Ace { kind: AceKind::Allow, sid: sid.to_owned(), mask, inherit_only: false }
    }

    fn secure() -> Security {
        Security {
            owner: Some(SYSTEM.to_owned()),
            acl: Some(vec![
                ace(SYSTEM, 0x1F01FF),
                ace(ADMINISTRATORS, 0x1F01FF),
                ace(TRUSTED_INSTALLER, 0x1F01FF),
                ace(USERS, READ),
            ]),
        }
    }

    fn with_ace(extra: Ace) -> Security {
        let mut security = secure();
        security.acl.as_mut().unwrap().push(extra);
        security
    }

    #[test]
    fn a_normal_program_files_folder_is_safe() {
        assert!(is_safe(&secure()));
        for owner in [ADMINISTRATORS, SYSTEM, TRUSTED_INSTALLER] {
            assert!(is_safe(&Security { owner: Some(owner.to_owned()), ..secure() }), "{owner}");
        }
    }

    #[test]
    fn the_owner_must_be_one_of_the_three() {
        for owner in [
            Some(USERS.to_owned()),
            Some("S-1-5-21-1-2-3-1001".to_owned()),
            Some(CREATOR_OWNER.to_owned()),
            None,
            Some(String::new()),
        ] {
            assert!(!is_safe(&Security { owner: owner.clone(), ..secure() }), "{owner:?}");
        }
    }

    #[test]
    fn a_null_or_unreadable_list_is_not_safe() {
        assert!(!is_safe(&Security { acl: None, ..secure() }));
        // Une liste vide ne donne aucun droit : sûre.
        assert!(is_safe(&Security { acl: Some(Vec::new()), ..secure() }));
    }

    #[test]
    fn each_write_right_given_to_anyone_else_makes_it_unsafe() {
        for right in
            [0x2, 0x4, 0x10, 0x40, 0x100, 0x10000, 0x40000, 0x80000, 0x4000_0000, 0x1000_0000]
        {
            assert!(!is_safe(&with_ace(ace(USERS, right))), "{right:#x}");
            assert!(!is_safe(&with_ace(ace(AUTHENTICATED, READ | right))), "{right:#x}");
        }
        // Lire et exécuter ne suffit pas à écrire.
        assert!(is_safe(&with_ace(ace(USERS, 0x1200A9))));
        // `WRITE_RIGHTS` est exactement ces dix droits.
        assert_eq!(WRITE_RIGHTS.count_ones(), 10);
    }

    #[test]
    fn the_three_trusted_sids_may_write() {
        for sid in [ADMINISTRATORS, SYSTEM, TRUSTED_INSTALLER] {
            assert!(is_safe(&with_ace(ace(sid, 0x1F01FF))), "{sid}");
        }
    }

    #[test]
    fn an_inherit_only_entry_is_ignored_for_the_object() {
        let mut inherited = ace(USERS, 0x1F01FF);
        inherited.inherit_only = true;
        assert!(is_safe(&with_ace(inherited)));
    }

    #[test]
    fn creator_owner_is_ignored_but_owner_rights_is_a_sid_like_another() {
        assert!(is_safe(&with_ace(ace(CREATOR_OWNER, 0x1F01FF))));
        assert!(!is_safe(&with_ace(ace("S-1-3-4", 0x1F01FF))));
        assert!(is_safe(&with_ace(ace("S-1-3-4", READ))));
    }

    #[test]
    fn a_deny_entry_is_ignored_and_an_unknown_kind_is_not_safe() {
        let deny = Ace { kind: AceKind::Deny, ..ace(USERS, 0x1F01FF) };
        assert!(is_safe(&with_ace(deny)));
        let other = Ace { kind: AceKind::Other, ..ace(SYSTEM, READ) };
        assert!(!is_safe(&with_ace(other)));
    }

    #[test]
    fn a_sid_is_compared_without_case() {
        assert!(is_safe(&Security { owner: Some(TRUSTED_INSTALLER.to_lowercase()), ..secure() }));
        assert!(is_safe(&with_ace(ace(&SYSTEM.to_lowercase(), 0x1F01FF))));
    }

    #[test]
    fn a_direct_child_is_compared_by_components() {
        let parent = r"\\?\C:\Program Files";
        assert!(is_direct_child(r"\\?\C:\Program Files\Shinku", parent));
        assert!(is_direct_child(r"\\?\c:\PROGRAM FILES\shinku", parent));
        assert!(is_direct_child(r"C:\Program Files\Shinku\", parent));
        for child in [
            r"\\?\C:\Program Files Evil\Shinku",
            r"\\?\C:\Program Files",
            r"\\?\C:\Program Files\Shinku\Plus",
            r"\\?\C:\Program\Files\Shinku",
            r"\\?\D:\Program Files\Shinku",
            r"\\?\C:\Users\Program Files\Shinku",
            r"\\?\C:\Program Files\\",
            "",
        ] {
            assert!(!is_direct_child(child, parent), "{child}");
        }
        assert!(!is_direct_child(r"\\?\C:\X", ""));
        // Casse pliée en ASCII seulement : `É` et `é` diffèrent.
        assert!(!is_direct_child(r"\\?\C:\Programmes\É", r"\\?\C:\Programmes\é"));
        assert!(is_direct_child(r"\\?\C:\Programmes\É\x", r"\\?\C:\Programmes\É"));
        assert!(!is_direct_child(r"\\?\C:\Programmes\é\x", r"\\?\C:\Programmes\É"));
    }

    fn placed(path: &str, security: Security) -> Placed {
        Placed { path: path.to_owned(), security, reparse: false }
    }

    fn setup() -> (Placed, Placed, Vec<Placed>) {
        (
            placed(r"\\?\C:\Program Files\Shinku\shinku-helper.exe", secure()),
            placed(r"\\?\C:\Program Files\Shinku", secure()),
            vec![
                placed(r"\\?\C:\Program Files", secure()),
                placed(r"\\?\C:\Program Files (x86)", secure()),
            ],
        )
    }

    #[test]
    fn a_normal_installation_is_well_placed() {
        let (helper, folder, files) = setup();
        assert!(helper_is_well_placed(&helper, &folder, &files));
        // Le dossier `Program Files (x86)` convient aussi.
        let (mut helper, mut folder, files) = setup();
        helper.path = r"\\?\C:\Program Files (x86)\Shinku\shinku-helper.exe".into();
        folder.path = r"\\?\C:\Program Files (x86)\Shinku".into();
        assert!(helper_is_well_placed(&helper, &folder, &files));
    }

    #[test]
    fn each_rule_of_the_location_is_needed() {
        let unsafe_security = with_ace(ace(USERS, 0x2));
        let (helper, folder, files) = setup();
        // Le nom du fichier.
        let mut other = helper.clone();
        other.path = r"\\?\C:\Program Files\Shinku\autre.exe".into();
        assert!(!helper_is_well_placed(&other, &folder, &files));
        // Un point d'analyse.
        let mut link = helper.clone();
        link.reparse = true;
        assert!(!helper_is_well_placed(&link, &folder, &files));
        // Le fichier, le dossier et Program Files doivent être sûrs.
        assert!(!helper_is_well_placed(
            &Placed { security: unsafe_security.clone(), ..helper.clone() },
            &folder,
            &files
        ));
        assert!(!helper_is_well_placed(
            &helper,
            &Placed { security: unsafe_security.clone(), ..folder.clone() },
            &files
        ));
        let bad_files = vec![Placed { security: unsafe_security, ..files[0].clone() }];
        assert!(!helper_is_well_placed(&helper, &folder, &bad_files));
        // Le dossier n'est pas celui du fichier.
        let mut elsewhere = folder.clone();
        elsewhere.path = r"\\?\C:\Program Files\Autre".into();
        assert!(!helper_is_well_placed(&helper, &elsewhere, &files));
        // Le dossier n'est pas l'enfant direct de Program Files.
        let deep_helper = placed(r"\\?\C:\Program Files\Shinku\bin\shinku-helper.exe", secure());
        let deep_folder = placed(r"\\?\C:\Program Files\Shinku\bin", secure());
        assert!(!helper_is_well_placed(&deep_helper, &deep_folder, &files));
        let evil_helper = placed(r"\\?\C:\Program Files Evil\Shinku\shinku-helper.exe", secure());
        let evil_folder = placed(r"\\?\C:\Program Files Evil\Shinku", secure());
        assert!(!helper_is_well_placed(&evil_helper, &evil_folder, &files));
        // Un point d'analyse parmi les dossiers.
        assert!(!helper_is_well_placed(
            &helper,
            &Placed { reparse: true, ..folder.clone() },
            &files
        ));
        let linked = vec![Placed { reparse: true, ..files[0].clone() }];
        assert!(!helper_is_well_placed(&helper, &folder, &linked));
        // Aucun Program Files connu.
        assert!(!helper_is_well_placed(&helper, &folder, &[]));
    }

    #[test]
    fn the_helper_name_is_compared_without_ascii_case_only() {
        let (mut helper, folder, files) = setup();
        helper.path = r"\\?\C:\Program Files\Shinku\SHINKU-HELPER.EXE".into();
        assert!(helper_is_well_placed(&helper, &folder, &files));
        helper.path = r"\\?\C:\Program Files\Shinku\shinku-helper.exe.old".into();
        assert!(!helper_is_well_placed(&helper, &folder, &files));
        assert_eq!(file_name(r"\\?\C:\a\b.exe"), Some("b.exe"));
    }
}
