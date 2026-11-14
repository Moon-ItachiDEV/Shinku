# Notes de conception

Ce document dit comment le code de Shinku applique le document d'architecture,
où il s'en écarte et pourquoi, et ce qui reste à faire avant une version
publique. Le document d'architecture reste la référence pour le « quoi » ; ces
notes couvrent le « comment ».

## Trois programmes, une frontière

| Programme | Rôle | Droits |
| --- | --- | --- |
| `shinku.exe` | La fenêtre et le cœur : analyse tout ce qui appartient à l'utilisateur. | Ceux de l'utilisateur, jamais plus. |
| `shinku-helper.exe` | Les actions sur la machine. N'accepte que des verbes fixes. | Administrateur, à la demande. Squelette pour l'instant. |
| `shinku-agent.exe` | L'analyse planifiée, sans fenêtre. | Ceux de l'utilisateur. Squelette pour l'instant. |

Toute la logique vit dans `shinku-core`. Les trois programmes ne font que
l'appeler : ce qui décide se teste donc sans fenêtre, sans Windows et sans
droits particuliers.

### La fenêtre refuse de tourner en administrateur

`shinku.exe` affiche une page web et travaille sur le profil de l'utilisateur :
deux choses qui n'ont rien à faire avec des droits d'administrateur. Élevé, il
peut même tourner sous un autre compte (mot de passe d'un administrateur saisi
depuis un compte standard, ou protection administrateur de Windows 11) et
regarder alors le mauvais profil.

Au démarrage, `platform::runs_elevated()` lit le jeton du processus :

- jeton ordinaire : Shinku démarre ;
- moitié « administrateur » d'une paire de jetons (« Exécuter en tant
  qu'administrateur ») : Shinku affiche un message et s'arrête ;
- tous les droits sans paire de jetons : le jeton ne suffit pas à trancher.
  Shinku regarde alors les droits du bureau (l'Explorateur). S'il a les mêmes,
  tout le PC tourne ainsi (contrôle de compte désactivé, session du compte
  Administrateur intégré) et refuser reviendrait à interdire Shinku : il
  démarre. Si le bureau est ordinaire ou appartient à un autre compte, c'est
  une élévation : il s'arrête.

L'installeur relance Shinku avec les droits de l'utilisateur à la fin de
l'installation (fonction `RunAsUser` du modèle NSIS de Tauri) : le message
n'apparaît donc pas après une installation.

Amélioration possible : se relancer soi-même sans élévation au lieu d'afficher
un message.

## Entre la page et le cœur : le guichet

La page ne parle qu'à `shinku_core::backend::Backend`, au travers de onze
commandes Tauri (`apps/shinku/src-tauri/src/commands.rs`). Chaque commande est
une demande précise et bornée : lancer une analyse, lire une page de résultats,
changer un réglage.

- Aucune commande ne reçoit un chemin à lire ou à écrire. Le dossier à exclure
  est choisi par l'utilisateur dans une boîte de dialogue de Windows, ouverte
  par le programme ; la page n'en fournit pas le chemin. Le seul chemin que la
  page envoie sert à retirer un dossier de la liste des exclusions, et n'est
  comparé qu'à cette liste.
- Les pages de résultats sont plafonnées côté cœur (`PAGE_LIMIT`), quelle que
  soit la taille demandée.

### Ce que la page a le droit de faire

- **Permissions Tauri au plus juste.** `capabilities/default.json` donne à la
  fenêtre ses onze commandes et les cinq gestes de sa barre de titre. Rien
  d'autre : ni fichiers, ni réseau, ni lancement de programmes, ni événements.
  Les commandes de l'application passent elles aussi par les permissions
  (`AppManifest::commands` dans `build.rs`) : une commande absente de la liste
  est refusée.
- **Politique de sécurité de contenu stricte.** Scripts, styles, images et
  polices viennent des fichiers de l'application, et de nulle part ailleurs.
  Rien n'est écrit dans la page : pas d'attribut `style`, pas de script en
  ligne. Les styles calculés (une largeur de jauge) passent par
  `views/style.ts`, qui règle les propriétés de l'élément sans attribut.
  La seule destination de `connect-src` est le canal de Tauri.
- **Aucun contenu distant.** La fenêtre refuse toute navigation hors de
  l'interface (`stays_in_the_interface` dans `app.rs`) et toute ouverture de
  fenêtre. `Object.prototype` est gelé (`freezePrototype`).
- **Pas de raccourcis de navigateur.** Dans l'application livrée, F5, Ctrl+P,
  Ctrl+F, le menu contextuel et leurs semblables sont neutralisés
  (`shortcuts.ts`). Copier, coller et tout sélectionner restent disponibles.

La liste des commandes existe en quatre endroits (le code Rust, `build.rs`, les
permissions, `backend/tauri.ts`). Le test `tests/wiring.test.ts` vérifie
qu'ils s'accordent, sans avoir à lancer la fenêtre.

### Écart avec le document d'architecture : l'avancement est demandé, pas poussé

Le document d'architecture prévoit des canaux Tauri pour l'avancement. Le code
fait autrement : pendant une analyse, la page demande l'état de la session
toutes les 150 ms (`session_view`).

- L'état est une petite structure en mémoire : la lire ne coûte rien.
- Il n'y a pas de flux à resynchroniser. Une page rechargée, ou ouverte en
  cours d'analyse, affiche le bon état à la première réponse.
- Le cœur ne dépend pas de Tauri, et la démo dans le navigateur suit
  exactement le même chemin que l'application.

Si une mesure montrait un coût, passer aux canaux ne toucherait que
`commands.rs` et `backend/tauri.ts`.

### Les types de l'interface sont écrits à la main, et vérifiés

`backend/types.ts` décrit le JSON du cœur. Pour qu'il ne dérive pas, un fichier
de référence (`tests/contract/contract.json`) liste les formes du JSON et les
valeurs de chaque énumération. Le cœur vérifie qu'il produit ces formes
(`crates/shinku-core/tests/ui_contract.rs`), l'interface vérifie qu'elle
attend les mêmes (`tests/contract.test.ts`). Ajouter une variante côté Rust
sans la reporter casse la compilation du test, puis le test de l'interface.

Produire `types.ts` automatiquement reste possible plus tard.

## La fenêtre

- **Sans cadre de Windows.** L'interface dessine sa barre de titre ; Windows
  garde l'ombre, les coins arrondis et le redimensionnement par les bords.
  Contrepartie connue : le survol du bouton Agrandir n'ouvre pas les
  dispositions d'ancrage de Windows 11 (Win + flèches fonctionne).
- **Taille.** 1280 × 800 à l'ouverture, réduite d'office si l'écran est plus
  petit ; 960 × 600 au minimum. Un portable de 13 pouces réglé à 150 % n'offre
  que 1280 × 672 : l'interface a une mise en page resserrée pour les fenêtres
  basses (`@media (max-height: …)` dans `styles/app.css`).
- **Couleur de fond.** La fenêtre prend la couleur du mode d'affichage avant de
  s'afficher, et la garde quand le mode change : pas d'éclair blanc à
  l'ouverture ni sur les bords pendant un redimensionnement.
- **Une seule fenêtre.** Un second lancement ramène la première au premier
  plan (`tauri-plugin-single-instance`). Ses arguments sont ignorés.
- **Jamais de fermeture muette.** Si WebView2 manque, si la fenêtre ne peut pas
  s'ouvrir ou si une erreur imprévue arrête le programme, un message de Windows
  l'explique. Sans lui, la version finale (sans console) disparaîtrait de
  l'écran sans un mot. La présence de WebView2 est vérifiée avant de lancer
  Tauri, qui sinon l'annonce lui-même, en anglais.
- **Zoom désactivé** pour l'instant : la mise en page est réglée au pixel.
  L'échelle d'affichage de Windows est respectée.

## Où Shinku écrit

| Emplacement | Contenu |
| --- | --- |
| `%APPDATA%\Shinku\settings.json` | Les réglages. |
| `%LOCALAPPDATA%\fr.shinku.app\` | Le cache de WebView2 pour la fenêtre. |

Rien d'autre, et rien hors du PC. L'analyse ne modifie aucun fichier.

Un fichier de réglages illisible n'est jamais écrasé en silence : Shinku repart
des valeurs par défaut, le dit, et bloque l'analyse tant que l'utilisateur n'a
pas vu l'avertissement si la liste des dossiers exclus a pu être perdue.
L'ancien fichier est gardé sous le nom `settings.illisible.json`.

## L'analyse

- Une seule lecture du disque sert les fichiers inutiles, les gros fichiers et
  les candidats aux doublons. Les liens et les jonctions ne sont jamais suivis.
- La liste d'interdits (`guard.rs`) est compilée dans le cœur : aucune règle
  ne peut proposer Windows, Program Files ou la racine d'une bibliothèque.
- Les règles épargnent ce qui a moins de sept jours. Dans `%TEMP%`, chaque
  élément est jugé en bloc : le dossier d'un installeur en cours n'est pas
  vidé à moitié.
- Doublons : même taille, puis empreinte d'un extrait, puis empreinte BLAKE3
  du fichier entier. L'exemplaire le plus ancien de chaque groupe est gardé.
  La comparaison octet par octet aura lieu au moment de supprimer.
- Les dossiers synchronisés sont signalés pour OneDrive. Les autres services,
  que Windows recense dans la clé de registre `SyncRootManager`, ne le sont
  pas encore.
- Gros fichiers : à partir de 3 Go. Shinku ne coche jamais un gros fichier à
  la place de l'utilisateur.

## L'installeur

NSIS, installation par machine dans Program Files, en français, avec l'amorce
de WebView2 embarquée : c'est le choix du document d'architecture. Program
Files protège les exécutables d'une modification par un programme qui n'a que
les droits de l'utilisateur.

WebView2 fait partie de Windows 11 : l'amorce ne sert que s'il a été retiré du
PC, et c'est le seul cas où l'installeur télécharge quelque chose (chez
Microsoft).

Pour l'instant, l'installeur ne contient que `shinku.exe`. Le helper et l'agent
y entreront avec leurs premières fonctions.

### Le manifeste de `shinku.exe`

Le manifeste (`apps/shinku/src-tauri/windows-manifest.xml`) dit à Windows ce
que le programme attend de lui. `tauri-build` le recopie ligne par ligne dans
les ressources de l'exécutable, ce qui abîme trois écritures pourtant
correctes en XML : la déclaration `<?xml … ?>`, les lettres accentuées et les
apostrophes. Le fichier s'en passe, et `build.rs` refuse de compiler s'il en
contient.

La première version installée sur un vrai PC ne démarrait pas pour cette
raison (« configuration côte-à-côte incorrecte »). Tous les tests passaient,
et Wine, moins strict que Windows, lançait le programme.

### Essayé avant d'être publié

Depuis, l'intégration continue installe l'installeur qu'elle vient de
fabriquer et lance Shinku pour de bon
(`apps/shinku/scripts/essai-fenetre.mjs`). La fenêtre doit s'ouvrir, analyser
le disque de la machine, montrer chacun de ses écrans, enregistrer un réglage,
refuser de s'ouvrir en double et se fermer. Une erreur dans la console de la
page ou un message d'erreur à l'écran fait échouer l'essai. L'installeur
n'est publié que s'il passe.

L'essai lit la page et clique par le port de débogage de WebView2, demandé de
l'extérieur par une variable d'environnement : `shinku.exe` n'a aucune option
d'essai.

Les machines de GitHub font tout tourner en administrateur. Shinku y est donc
lancé avec un jeton filtré, comme celui que donne le contrôle de compte
d'utilisateur (`apps/shinku/scripts/lancer-en-utilisateur.ps1`) : ce sont les
droits qu'il a sur le PC de quelqu'un, et WebView2 n'écoute cette variable
d'environnement que dans un programme sans droits d'administrateur.

## Avant une version publique

- **Signature du code.** L'installeur et les exécutables ne sont pas signés.
  Voir la décision prise sur le certificat individuel.
- **Désinstallation.** La case « supprimer les données » du modèle de Tauri
  efface un dossier du profil avec les droits d'administrateur. Elle ne vise
  pas `%APPDATA%\Shinku`, et un effacement élevé dans un dossier que
  l'utilisateur peut modifier est exactement ce que le document d'architecture
  interdit au helper. À remplacer par un effacement fait par `shinku.exe`
  lui-même, sans élévation.
- **Port de débogage de WebView2.** Un programme qui tourne déjà sous le
  compte de l'utilisateur peut lancer Shinku avec ce port ouvert, comme le
  fait l'essai de la fenêtre, et piloter sa page. Il n'y gagne rien tant que
  Shinku ne supprime rien et ne demande aucune élévation. À réexaminer avec
  les premières suppressions et le helper.
- **Helper et agent.** Lier leur bibliothèque d'exécution C++ de façon
  statique, comme le fait déjà `shinku.exe`, pour ne dépendre d'aucun
  composant à installer.
- **ARM64.** Le code ne dépend pas de l'architecture, mais seul l'installeur
  x64 est fabriqué et testé.
- **Intégration continue.** Désigner les actions GitHub par l'empreinte de
  leur commit plutôt que par leur numéro de version.
- **Licences.** Choisir la licence de Shinku, et produire la liste complète
  des licences des dépendances Rust.
- **Icône.** L'icône actuelle reprend le signe de l'interface
  (`scripts/icone.mjs`). Le logo définitif reste à choisir dans le canevas.
- **Identifiant.** `fr.shinku.app` nomme le dossier du cache de WebView2 : le
  changer après une publication laisserait l'ancien dossier derrière lui.
- **Journal local** et **mise à jour** : prévus par l'architecture, pas encore
  écrits.
