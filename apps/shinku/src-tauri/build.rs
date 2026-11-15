//! Prépare la compilation de la fenêtre de Shinku : permissions des commandes
//! et ressources Windows (icône, manifeste, numéro de version).

/// Les commandes que la fenêtre peut appeler, une par fonction de
/// `src/commands.rs`. Tauri en tire une permission `allow-…` par commande, et
/// `capabilities/default.json` dit lesquelles la fenêtre reçoit. Une commande
/// absente d'une de ces listes est refusée à l'exécution : le test
/// `tests/wiring.test.ts` de l'interface vérifie qu'elles disent la même chose.
const COMMANDS: &[&str] = &[
    "overview",
    "acknowledge_settings_issue",
    "start_scan",
    "cancel_scan",
    "session_view",
    "clean_preview",
    "start_clean",
    "cancel_clean",
    "junk_details",
    "large_files_page",
    "duplicates_page",
    "set_theme",
    "add_excluded_folder",
    "remove_excluded_folder",
];

/// Le manifeste de `shinku.exe` : ce que le programme déclare à Windows.
///
/// - Contrôles communs en version 6 : les messages de Windows affichés par
///   Shinku ont le style de Windows 11.
/// - `asInvoker` : les droits de celui qui lance le programme, jamais plus.
///   La fenêtre ne demande pas l'élévation ; seul `shinku-helper.exe` le
///   fera, à la demande.
/// - `supportedOS` : programme écrit pour Windows 10 et 11, sans mode de
///   compatibilité.
/// - `dpiAware` et `dpiAwareness` : net sur chaque écran, y compris pour les
///   messages affichés avant la fenêtre.
/// - `longPathAware` : chemins de plus de 260 caractères, quand Windows les
///   autorise.
///
/// Ces explications sont ici et pas dans le fichier : voir [`check_manifest`].
const MANIFEST: &str = include_str!("windows-manifest.xml");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=windows-manifest.xml");

    // Vérifié sur toutes les plateformes : l'erreur se voit dès la première
    // compilation, pas seulement au moment de fabriquer l'installeur.
    if let Err(problem) = check_manifest(MANIFEST) {
        panic!("windows-manifest.xml : {problem}");
    }

    // Shinku ne vise que Windows. Ailleurs, ce paquet n'est qu'un programme
    // qui le dit : il n'y a rien à préparer.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let windows = tauri_build::WindowsAttributes::new().app_manifest(MANIFEST);
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(windows)
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("la préparation de l'application Tauri a échoué : {error:#}");
    }
}

/// Refuse un manifeste que son incorporation à l'exécutable abîmerait.
///
/// `tauri-build` ne l'incorpore pas tel quel : il le recopie ligne par ligne
/// dans un fichier de ressources, chaque ligne entre guillemets et précédée
/// d'une espace. Trois écritures, correctes en XML, n'y survivent pas :
///
/// - la déclaration `<?xml … ?>` n'est plus au tout début du document, ce qui
///   en fait un XML invalide. Windows refuse alors de lancer le programme
///   (« sa configuration côte-à-côte est incorrecte »), alors que Wine, moins
///   strict, le lance : la première version installée sur un vrai PC ne
///   démarrait pas pour cette raison ;
/// - une lettre accentuée dépend de la page de codes du compilateur de
///   ressources ;
/// - une apostrophe ressort précédée d'une barre oblique inversée.
///
/// Le texte d'un élément doit aussi tenir sur la ligne de ses balises : coupé
/// en lignes, il gagnerait des espaces.
fn check_manifest(manifest: &str) -> Result<(), &'static str> {
    if manifest.contains("<?xml") {
        return Err("pas de déclaration <?xml … ?> : Windows refuserait de lancer shinku.exe");
    }
    if !manifest.is_ascii() {
        return Err("que des caractères ASCII : les explications vont dans build.rs");
    }
    if manifest.contains('\'') {
        return Err("pas d'apostrophe : les attributs s'écrivent entre guillemets");
    }
    Ok(())
}
