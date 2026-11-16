//! Le module « Confidentialité » : effacer des **traces de l'activité de la
//! personne sur ce PC, dans son compte**.
//!
//! Shinku ne montre jamais le contenu d'une trace (ni adresse, ni nom de
//! fichier) : il dit qu'elle existe, son poids ou son nombre d'éléments. Rien
//! n'est coché d'avance, rien n'est élevé, rien n'est écrit au journal.
//!
//! Le catalogue est une **liste fermée**, compilée dans [`rules`] : l'historique
//! d'un profil de Chrome, Edge, Brave ou Vivaldi (des fichiers de noms exacts
//! dans un dossier de forme exacte), puis, pour Windows, les fichiers récents,
//! ce qui a été tapé (trois clés de registre, une énumération compilée) et le
//! presse-papiers. Jamais `Local State`, les mots de passe, la saisie
//! automatique, les favoris, les cookies, les données des sites, les extensions.
//!
//! - [`rules`] : le catalogue, pur ;
//! - [`names`] : le nom affiché d'un profil, lu dans `Local State` (jamais écrit) ;
//! - [`id`] : l'identifiant d'une trace, strict et opaque ;
//! - [`view`] : ce que la page reçoit et ce qu'on lui refuse ;
//! - [`erase`] : effacer (« tout ou rien » pour les fichiers, relecture pour le
//!   registre) ;
//! - [`host`] et [`fake`] : la machine vue par le module, et son faux ;
//! - [`desk`] : le guichet. L'exclusivité avec le grand vide et la mise à la
//!   corbeille des restes se prend dans le guichet principal.

pub mod desk;
pub mod erase;
pub mod fake;
pub mod host;
pub mod id;
pub mod names;
pub mod rules;
pub mod view;

#[cfg(test)]
mod tests;
