# Notes de conception

Ce document dit comment le code de Shinku applique le document d'architecture,
où il s'en écarte et pourquoi, et ce qui reste à faire avant une version
publique. Le document d'architecture reste la référence pour le « quoi » ; ces
notes couvrent le « comment ».

## Trois programmes, une frontière

| Programme | Rôle | Droits |
| --- | --- | --- |
| `shinku.exe` | La fenêtre et le cœur : analyse tout ce qui appartient à l'utilisateur. | Ceux de l'utilisateur, jamais plus. |
| `shinku-helper.exe` | Les actions sur la machine. N'accepte qu'une demande fixe de quatre mots, dont un nom (voir « Régler une entrée du PC »). | Administrateur, à la demande, un processus par changement (voir « Contre qui l'assistant protège »). |
| `shinku-agent.exe` | Le mode jeu, sans fenêtre (voir plus bas). Plus tard, l'analyse planifiée. | Ceux de l'utilisateur ; il refuse de tourner en administrateur. |

### Contre qui l'assistant protège

Un programme qui tourne sous le compte, sans droits, **contrôle entièrement
`shinku.exe`** : même utilisateur, même niveau d'intégrité. Il peut l'injecter,
piloter sa page, changer la demande avant le lancement. Rien de ce que
`shinku.exe` vérifie n'est donc une barrière contre lui : ce sont des garde-fous
contre **nos propres erreurs** (élever un fichier remplaçable, se tromper de
ligne).

La seule barrière est **l'accord de Windows**, et encore, selon le mode :

| Mode | Accord | Frontière ? |
| --- | --- | --- |
| A. Administrateur à jeton partagé (PC perso courant) | un clic sur « Oui » | Non, d'après Microsoft lui-même |
| B. Compte standard | mot de passe d'un administrateur | Oui |
| C. « Protection administrateur » de Windows 11 | la personne, authentifiée | Oui |
| D. Contrôle de compte coupé, ou Shinku déjà élevé | aucun | Sans objet : la boîte de l'assistant est la seule question |

Ce que quelqu'un gagne, au plus, en passant par l'assistant : **couper ou remettre
une entrée qui existe déjà** dans `HKLM` ou dans le dossier Démarrage commun,
désignée par son vrai nom, que l'assistant affiche. Il ne choisit pas le texte
affiché : ces noms ne sont écrits que par des administrateurs.

Conséquences assumées :

- **La boîte de l'assistant n'est pas une barrière.** Elle s'affiche sur le bureau
  ordinaire : un programme du compte ne peut pas y cliquer (Windows l'interdit
  vers une fenêtre élevée), mais il peut la recouvrir. Elle protège contre
  l'erreur honnête, contre le « oui » réflexe à une fenêtre de Windows qu'on n'a
  pas demandée, et contre une demande substituée dans un `shinku.exe` détourné
  **si la page montre le même nom**. Aucun lot futur ne doit s'appuyer dessus
  comme sur une protection.
- **Sans signature, la fenêtre de Windows ne distingue pas Shinku d'un
  imitateur** (bannière « éditeur inconnu »). La sortie publique est déjà
  conditionnée à la signature de `shinku.exe` ; **elle l'est aussi à celle de
  `shinku-helper.exe`**, et `shinku.exe` devra alors vérifier la signature de
  l'assistant avant de le lancer (voir « Avant une version publique »). D'ici
  là, seules les versions d'essai existent : pas de réglage « expérimental ».
- Le verbe de ce lot **a un paramètre** (un nom) et n'est **pas** « sans danger
  même si un programme malveillant l'envoie » : c'est pourquoi l'assistant
  relit tout lui-même et montre ce qu'il change. `shinku.exe`, lui, n'écrit
  jamais sous `HKLM`.

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

`shinku.exe --oublier` (voir « Effacer les réglages à la désinstallation »)
rend la même décision : il appelle `elevated_launch`, il ne la recopie pas. Il
la rend par un code de sortie (2), sans boîte de message.

## Entre la page et le cœur : le guichet

La page ne parle qu'à `shinku_core::backend::Backend`, au travers de trente-six
commandes Tauri (`apps/shinku/src-tauri/src/commands.rs`). Chaque commande est
une demande précise et bornée : lancer une analyse, lire une page de résultats,
changer un réglage, faire le vide.

- Aucune commande ne reçoit un chemin à lire ou à écrire. Le dossier à exclure
  est choisi par l'utilisateur dans une boîte de dialogue de Windows, ouverte
  par le programme ; la page n'en fournit pas le chemin. Les seuls chemins ou
  noms que la page renvoie sont ceux que le cœur lui a donnés (un dossier à
  retirer de la liste des exclusions, un jeu ou une app du mode jeu), et ils
  ne sont que comparés à ce que le cœur connaît déjà : un texte inconnu est
  refusé. Pour ajouter un jeu ou une app, la page choisit un numéro dans
  la dernière liste d'apps ouvertes que le cœur lui a rendue, et rend aussi le
  chemin qu'elle y a lu : le cœur vérifie que ce chemin est exactement celui de
  ce numéro dans *sa* dernière liste, sinon la liste a changé et la demande est
  refusée (`unknownApp`). Le chemin n'est jamais utilisé, seulement comparé. Le grand vide
  lui-même ne reçoit que trois cases cochées et la date de l'analyse
  affichée. Le module Démarrage ne reçoit qu'un **identifiant** que le cœur a
  donné (deux lettres de place, un tiret et seize chiffres hexadécimaux) et une case : ni chemin, ni nom de valeur
  de registre, ni commande ne viennent de la page. Le module Confidentialité ne reçoit, lui aussi, que des **identifiants** fabriqués par le cœur
  (voir « Confidentialité »).
- Les pages de résultats sont plafonnées côté cœur (`PAGE_LIMIT`), quelle que
  soit la taille demandée.

### Ce que la page a le droit de faire

- **Permissions Tauri au plus juste.** `capabilities/default.json` donne à la
  fenêtre ses trente-six commandes et les cinq gestes de sa barre de titre. Rien
  d'autre : ni fichiers, ni réseau, ni événements, et aucun programme que la page
  nomme : seuls sont lancés l'agent, sept pages des réglages de Windows, le
  fichier des licences (`LICENCES-TIERS.txt`, à côté de `shinku.exe`, ouvert avec
  le programme que Windows associe aux fichiers texte : voir « Le Journal »),
  et, après confirmation, le désinstalleur d'une app que le cœur relit dans le
  registre.
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

La liste des commandes existe en six endroits : `build.rs`, `generate_handler!`
(`app.rs`), `commands.rs`, les permissions (`capabilities/default.json`),
`backend/tauri.ts`, et les types de l'interface (`backend/types.ts`, tenus par le
contrat). Le test `tests/wiring.test.ts` vérifie qu'ils s'accordent, sans avoir
à lancer la fenêtre.

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
  **Un second lancement de la fenêtre n'écrit rien dans les fichiers de
  Shinku** (`shinku.exe --oublier` n'ouvre pas de fenêtre : c'est un autre
  chemin, qui efface, voir « Effacer les réglages à la désinstallation »). Le
  greffon tranche pendant `build()` (il sort alors avec
  `std::process::exit(0)`) ; or `run()` a déjà construit le guichet
  (`.manage(Backend::for_machine())`). Ce que `run()` fait avant la décision ne
  fait donc que lire : `for_machine` lit les réglages (`SettingsStore::load`) et
  branche les magasins (`GameStore`, `StartupStore`, `UninstallStore`,
  `JournalStore`) et les hôtes sans rien écrire, ni dans un fichier, ni dans le
  registre, ni en lançant un programme ; `report_crashes` ne fait que poser un
  crochet de panne. Tout ce qui écrit est dans le crochet `setup` de
  l'application, qui tourne après ceux des greffons (`tauri::Builder::build` appelle
  `initialize_plugins`, puis `setup` ne vient qu'à l'événement `Ready` de
  `run`) : la fermeture du grand vide coupé (`Backend::close_interrupted_clean`,
  voir « Le Journal »), puis la mise en accord de l'agent du mode jeu
  (`sync_agent`, qui écrit la valeur `Run` et lance `shinku-agent.exe`).
  `Backend::with_journal` ne fait que brancher le journal ; l'essai
  `the_interrupted_clean_is_closed_only_once_the_single_window_is_known`
  (`main.rs`) garde l'ordre dans la source. La preuve sur le vrai Windows est
  `checkSingleWindow` (`scripts/essai-fenetre.mjs`) : elle écrit dans
  `journal.json` un grand vide « en cours » daté de 2020, lance un second
  `shinku.exe`, exige le fichier identique octet pour octet, puis remet les
  octets d'origine (sautée sans `journal.json`, donc sans `--grand-vide`).
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
| `%APPDATA%\Shinku\journal.json` | Les grands vides : dates et totaux, aucun chemin. |
| `%LOCALAPPDATA%\Shinku\game.json` | Le journal du mode jeu : la partie en cours, le bilan de la dernière, les jeux reconnus. Écrit par l'agent seulement. |
| `%LOCALAPPDATA%\Shinku\startup.json` | Ce que le module Démarrage a coupé (au plus 200 lignes : identifiant, nom, dates, `FILETIME` écrit). Écrit par la fenêtre seulement. |
| `%LOCALAPPDATA%\Shinku\uninstalls.json` | Les désinstallations que la page Désinstallation a menées (au plus 100 notes : date, nom de l'app, nombre et poids des restes mis à la corbeille ; aucun chemin). Écrit par la fenêtre seulement. Voir « Le Journal ». |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, valeur `Shinku` | Lance l'agent avec Windows. La seule écriture que Shinku fasse dans une clé `Run`. Présente seulement quand le mode jeu automatique est allumé. |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\{Run, Run32, StartupFolder}` | Une valeur de douze octets par entrée de démarrage du compte que la personne coupe ou remet (voir « Démarrage »). Le même réglage que le Gestionnaire des tâches. Écrite par `shinku.exe`. |
| `HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\{Run, Run32, StartupFolder}` | La même valeur de douze octets pour une entrée du PC. Écrite **seulement par `shinku-helper.exe`**, élevé, après l'accord de Windows, quand il en demande un, et un « OK » dans la boîte de l'assistant (vue 64 bits). |
| `%LOCALAPPDATA%\fr.shinku.app\` | Le cache de WebView2 pour la fenêtre. |
| `%APPDATA%\fr.shinku.app\` | Rien que Shinku écrive : le modèle de Tauri le nomme aussi, et le désinstalleur le supprime (voir « Effacer les réglages à la désinstallation »). |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\{RunMRU, TypedPaths, WordWheelQuery}` | Les **valeurs** de ces trois clés sont **supprimées** quand la personne coche « Ce que tu as tapé dans Windows » (voir « Confidentialité »). Les clés restent. C'est la première fois que Shinku supprime dans le registre. |
| `%LOCALAPPDATA%\{Google\Chrome, Microsoft\Edge, BraveSoftware\Brave-Browser, Vivaldi}\User Data\{Default, Profile N}\` | Les fichiers de l'**historique** d'un profil (liste fermée, voir « Confidentialité ») sont supprimés, directement, quand la personne le coche. Rien d'autre du profil. |
| Fichiers récents de Windows, presse-papiers | Vidés quand la personne le coche : les récents par Windows (`SHAddToRecentDocs`), l'historique du presse-papiers par Windows (`Clipboard.ClearHistory`), et son contenu actuel par Shinku lui-même (`OpenClipboard`, `EmptyClipboard`). |

Rien d'autre, et rien hors du PC. `shinku.exe` n'écrit jamais sous `HKLM` : la
seule écriture de ce genre est la valeur d'approbation ci-dessus, faite par
l'assistant élevé. L'analyse ne modifie aucun fichier. Le
grand vide supprime ce que décrit la section qui lui est consacrée ; la
Confidentialité supprime des traces d'une liste fermée que la personne a
cochées (fichiers d'historique, valeurs de trois clés de `HKCU`) ; la
Désinstallation n'envoie à la corbeille que des restes cochés. Le mode jeu change des réglages en cours de partie (priorité de
processus, mode d'alimentation), jamais un fichier de la personne ; il remet
tout en place. Le module Démarrage ne change que des approbations de démarrage
(du compte, ou du PC par l'assistant), et ne supprime aucune entrée de démarrage.

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
  vidé à moitié. Ces dossiers ne sont comptés qu'à la fin du parcours :
  pendant l'analyse, le chiffre « à libérer » reste donc en dessous de la
  vérité, puis rattrape son retard d'un coup. À corriger avec le moteur.
- Doublons : même taille, puis empreinte d'un extrait, puis empreinte BLAKE3
  du fichier entier. L'exemplaire le plus ancien de chaque groupe est gardé,
  tant que la personne n'en choisit pas un autre.
  La comparaison octet par octet aura lieu au moment de supprimer.
- Les dossiers synchronisés sont signalés pour OneDrive. Les autres services,
  que Windows recense dans la clé de registre `SyncRootManager`, ne le sont
  pas encore.
- Gros fichiers : à partir de 3 Go. Shinku ne coche jamais un gros fichier à
  la place de l'utilisateur.

### Écart avec le plan technique : les doublons ne sont pas cochés d'office

Le plan technique rangeait les doublons dans le lot automatique, avec les
fichiers inutiles et la corbeille : cochés d'office, ils partaient avec
« Faire le vide ». La première analyse d'un vrai PC a proposé ainsi plus de
25 Go de doublons. Des photos ou des vidéos gardées en double pouvaient donc
partir sans que personne ne l'ait décidé. Le choix a été fait le
15 novembre 2026 : des fichiers personnels ne sont jamais cochés d'office.

- Le cœur range chaque catégorie du grand vide dans un lot (`Lot`) :
  automatique (fichiers inutiles, corbeille), au choix (doublons) ou « À
  vérifier » (gros fichiers, démarrage). Un test refuse qu'une catégorie de
  fichiers personnels, de niveau B, entre dans le lot automatique.
- Les doublons restent sur l'écran Résultats, avec leur total et une case
  décochée. Le chiffre « à libérer » de l'analyse et le total des résultats ne
  comptent que le lot automatique, puis ce que la personne coche elle-même.
- Une case cochée à la main ne vaut que pour l'analyse où elle l'a été : des
  doublons cochés tout à l'heure ne le sont plus après une nouvelle analyse.
  Une case décochée, elle, le reste. Une nouvelle analyse ne coche donc jamais
  rien de plus que les cases d'office. L'interface reconnaît une nouvelle
  analyse à son heure de début (`Summary.started`).
- La case des doublons reste fermée tant que leur recherche continue : on ne
  coche pas ce qu'on ne connaît pas encore.
- Dans chaque groupe, un exemplaire est toujours gardé : le plus ancien, ou
  celui que la personne choisit sur l'écran Doublons, ligne par ligne ou pour
  tout un dossier. Ce choix (`keep_duplicate`) désigne un exemplaire que le
  rapport connaît déjà, par l'empreinte et la taille de son groupe et par son
  rang : aucun chemin ne vient de la page. Le cœur le note dans son rapport,
  où un groupe garde toujours un exemplaire et un seul ; rien n'est supprimé
  à ce moment-là. Un choix ne vaut que pour l'analyse affichée : des résultats
  trop anciens pour nettoyer ne le prennent plus, et une nouvelle analyse
  repart du plus ancien, ce que l'écran dit. « Garder ce dossier » ne remplace
  pas un exemplaire déjà gardé dans ce dossier. L'écran ne propose que les
  dossiers des groupes déjà affichés, mais le choix vaut pour tous.

## Le grand vide

C'est la seule phase qui supprime (`crates/shinku-core/src/clean`). La page
envoie trois cases et la date de l'analyse qu'elle affiche ; la liste de ce
qui part vient du rapport gardé par le cœur, et du disque tel qu'il est au
moment d'écrire. L'ordre est fixe :

1. **La corbeille est vidée.** C'est le seul geste sans retour.
2. **Les fichiers inutiles sont supprimés directement.** L'étape refait le
   parcours de l'analyse, avec les mêmes règles et les mêmes âges, à la date
   de l'analyse : rien de plus récent qu'elle n'est touché. Chaque entrée est
   ouverte sans suivre de lien, revérifiée, puis supprimée par ce même accès.
   Un élément de `%TEMP%` jugé en bloc part en entier ou reste en entier.
3. **Les copies en double partent à la corbeille**, jamais ailleurs.
   L'exemplaire gardé de chaque groupe (le plus ancien, ou celui que la
   personne a choisi) reste ouvert
   tant que ses copies partent. Chaque copie lui est comparée octet par octet
   avant de partir.

Les garde-fous, chacun avec son test :

- des résultats de plus de dix minutes, ou d'une autre analyse que celle
  affichée, sont refusés ;
- la corbeille n'est jamais cochée d'office. La vider est sans retour, et
  elle garde ce que la personne a jeté comme les copies qu'un grand vide
  précédent y a envoyées « récupérables ». C'est un second écart avec le plan
  technique, qui la rangeait dans le lot automatique ;
- un fichier modifié depuis l'analyse reste. Un fichier qu'une app utilise
  reste aussi : rien n'est forcé, rien n'est remis au prochain démarrage. Un
  fichier inutile est ouvert sans aucun partage avant de partir : si un autre
  programme le tient, même en autorisant sa suppression, il reste ;
- un élément jugé en bloc de plus de dix mille entrées est laissé : sa
  suppression ne s'interrompt pas, et doit rester assez courte pour que fermer
  Shinku n'en laisse jamais un à moitié supprimé ;
- le cache d'une app ouverte n'est pas touché, et le bilan le dit ;
- les « dossiers jamais touchés » et la liste d'interdits sont relus au moment
  de supprimer. Une exclusion qui ne peut pas être appliquée arrête l'étape
  avant qu'elle commence ;
- quand la corbeille ne peut pas recevoir un fichier (désactivée, trop
  petite), Windows le supprimerait définitivement. Shinku écoute l'opération
  du shell (`platform/windows/shell.rs`) et l'arrête avant ;
- après chaque envoi, l'étape regarde ce que la copie est devenue. Une copie
  introuvable, ou un exemplaire gardé qui a changé, arrête l'étape : c'est
  l'arrêt de sécurité, que le bilan annonce avec le nom du fichier ;
- arrêter laisse finir le fichier en cours et rend un bilan partiel. Fermer
  la fenêtre fait de même avant que le programme ne s'arrête.

Limites connues, à traiter avant une version publique :

- une corbeille pleine fait de la place en abandonnant ses éléments les plus
  anciens. Le bilan le dit après coup (« récupérable » ne compte que ce
  qu'elle a gardé), mais Shinku ne lit pas encore sa capacité avant d'y
  envoyer des copies ;
- les fichiers inutiles sont jugés tels qu'ils sont au moment de supprimer.
  Un dossier ancien déplacé dans `%TEMP%` après l'analyse part donc avec le
  reste, sans avoir figuré dans les totaux confirmés.

Le bilan sépare ce qui est libéré pour de bon de ce qui attend dans la
corbeille, et n'annonce pas plus que ce que les disques ont gagné. Le journal
note le début, l'avancement et la fin de chaque grand vide : un grand vide
resté « en cours » au lancement suivant a été interrompu, et l'Accueil le dit
jusqu'à la prochaine analyse.

Côté page, « Faire le vide » ouvre d'abord une confirmation, qui liste
exactement la demande qui sera envoyée. Une case dont la catégorie n'a rien à
nettoyer ne compte pas. Le grand vide ne supprime que ce qu'il décrit ; la
Confidentialité supprime ses propres traces, cochées une à une.

## Le mode jeu

La première maquette avait un bouton « Préparer / lancer le jeu ». Il y a trop
de façons de lancer un jeu (Steam, Epic, un raccourci, un exécutable) pour que
Shinku en choisisse une : **Shinku ne lance rien**. La page Mode jeu est une
page de réglages directs. Un programme d'arrière-plan, l'agent, reconnaît le
jeu quand il passe au premier plan, applique les réglages, puis remet tout en
place.

Les règles du produit :

- rien n'est modifié sans accord : le mode jeu automatique est **éteint par
  défaut** ;
- tout se remet en place : fin de partie, fermeture de session, plantage,
  coupure de courant, désinstallation ;
- aucune élévation : ni `shinku.exe` ni l'agent ne tournent en administrateur ;
- honnêteté : pas de « mémoire libérée », pas d'images par seconde promises, et
  Shinku ne refait pas ce que Windows fait déjà (le Mode Jeu de Windows, les
  notifications coupées en plein écran : la page les signale et ouvre les
  réglages de Windows) ;
- la page ne fournit jamais un chemin ni un nom libre (voir plus haut).

### Reconnaître un jeu

Toute la décision est dans `shinku-core` (`game/`), testée sous Linux avec une
fausse machine (`FakeHost`). Un exécutable est un jeu, dans cet ordre :

1. s'il est dans les jeux ajoutés par la personne ;
2. sinon, s'il n'est pas dans « Ce n'est pas un jeu » ;
3. sinon, s'il ne tombe pas sous le **veto** : navigateurs, shell de Windows,
   lecteurs vidéo, bureautique, outils de développement, communication,
   lanceurs (Steam, Epic, Battle.net, Riot…) et Shinku lui-même, par nom ou par
   dossier de lanceur. Le veto ne joue pas contre un jeu que la personne a
   ajouté (c'est son choix écrit), mais l'ajout d'un programme sous veto est
   refusé ;
4. sinon, s'il se trouve dans une bibliothèque de jeux (`steamapps\common`,
   `XboxGames`, `Epic Games`, `GOG Games`, `Riot Games`, un dossier `Games` ou
   `Jeux`…).

Le jeu est identifié par le **chemin de son exécutable**, pas par un numéro de
processus. Il doit rester au premier plan sur deux pas consécutifs (2 s chacun)
avant que le mode s'active.

Ce qui est écarté, et pourquoi : la détection du plein écran et la lecture de la
liste de jeux de Windows. Trop de faux positifs (une vidéo en plein écran, un
diaporama) et une clé de registre non documentée, qui peut changer. Mieux vaut
un jeu à ajouter à la main qu'un navigateur traité comme un jeu.

### Ce que l'agent applique

- **Alimentation « Performances élevées »**, réévaluée à chaque pas : jamais sur
  batterie (remise tout de suite si on passe sur batterie en cours de partie) ;
  jamais si le PC n'a pas ce mode ; jamais sur un processeur AMD X3D
  (7900X3D, 7950X3D, 9900X3D, 9950X3D), pour lesquels AMD recommande le mode
  normal ; jamais si le mode actif est celui du constructeur ou un mode
  personnalisé. À la fin, le mode d'avant n'est remis **que si le mode actif est
  encore celui que Shinku a posé** : si la personne l'a changé entre-temps, son
  choix reste.
- **Apps en veilleuse** : priorité « sous la normale » et mode efficacité de
  Windows pour une liste d'apps (OneDrive, Dropbox, Google Drive cochés
  d'office ; les navigateurs proposés mais décochés, parce qu'on peut y regarder
  une vidéo ou y passer un appel ; plus les apps ajoutées). On ne fait que
  baisser : un processus déjà bas garde sa classe. L'app que la personne regarde
  n'est jamais ralentie. Les processus du système, de Shinku et du jeu sont
  protégés. **Les lanceurs de jeux** (Steam, Epic, GOG Galaxy, Battle.net,
  EA app, Ubisoft Connect, Rockstar…) ne sont jamais mis en veilleuse, même
  ajoutés à la main : c'est eux qui lancent les jeux, et un jeu né d'un
  processus en veilleuse en hériterait. Le bureau à distance (`mstsc.exe`) est
  protégé de même. Le nom d'une app ajoutée est la description de son fichier,
  lue quand elle tourne (sinon le nom du fichier sans `.exe`).

Ce qui n'est pas fait : fermer des apps, vider de la mémoire, changer des
priorités vers le haut, toucher au registre ou aux services.

### Le journal et la reprise

`game.json` est écrit par l'agent seulement, à chaque transition, jamais
périodiquement, d'un seul geste (fichier écrit à côté, puis mis en place). À
l'activation, **l'intention est écrite avant le geste** : la session complète,
avec le mode d'alimentation d'avant et, pour chaque processus mis en veilleuse,
son numéro, sa date de création et son état d'avant. Si l'écriture échoue
(disque plein), rien n'est modifié.

Au démarrage de l'agent (`recover`), une session trouvée au journal est reprise
si le jeu vit encore, sinon terminée normalement : c'est ce qui remet l'ordre
après un plantage ou une coupure de courant. Un processus n'est remis que s'il
vit encore (numéro **et** date de création : un numéro se recycle) ; sa priorité
n'est remise que si elle vaut encore ce que Shinku a posé.

**Dans le doute, on remet.** C'est la règle qui tranche toute incertitude de
cette section : relever par erreur un processus (le remettre en priorité
normale alors que la personne l'avait baissé elle-même) est un défaut bénin ;
laisser une app en priorité basse, sans que personne le sache, est un défaut
grave. L'heure murale, qui peut sauter ou reculer, ne décide donc de rien de
grave.

**Les processus nés d'une app en veilleuse.** Un processus créé par un
processus dont la classe de priorité a été baissée en hérite. La règle est un
invariant, sans date ni horloge : *seuls les processus du journal restent en
veilleuse ; tout autre processus trouvé exactement « sous la normale » par
héritage est relevé dès qu'on le voit.* L'identité d'un processus est le couple
(numéro, date de création), comparé par égalité seulement : aucune comparaison
d'ordre, aucune heure murale, aucune horloge monotone dans la filiation.

- Un **émetteur** est une entrée du journal dont l'état d'avant était
  strictement au-dessus de « sous la normale » (`priority::lowers`). Une app
  déjà basse avant Shinku n'émet rien : ses enfants ne sont jamais touchés.
- La **famille** (en mémoire seulement) : les processus hors journal, ni le jeu
  ni ancêtre du jeu, vus naître d'un émetteur ou d'un membre. Un processus est
  *nouveau* s'il est absent du dernier relevé lisible ; un **héritier** est un
  membre nouveau trouvé exactement « sous la normale ». Les processus déjà là
  avant la partie, ou déjà bas avant d'être vus, ne sont jamais touchés.
- À chaque pas d'une partie ouverte, journal vide compris (les points « à suivre »
  survivent à la dernière remise), avant tout retour anticipé
  (veilleuse éteinte, partie relâchée, remise en attente), la passe de famille
  cherche les membres jusqu'au point fixe : l'ensemble de départ réunit les
  numéros des émetteurs vivants, des membres vivants et des morts de ce pas (un
  parent mort au pas où son enfant apparaît reste un émetteur), plus ceux que
  la passe d'avant a laissés « à suivre » : tout ce que Shinku relève ou remet
  (entrée remise en cours de partie, jeu ou ancêtre relevé) et tout émetteur
  trouvé mort reste un point de départ de la prochaine passe lisible, car un
  enfant né entre le relevé et le geste a pu hériter. Le jeu et ses ancêtres ne
  deviennent pas membres : seuls leurs enfants nouveaux sont évalués. Un membre qui
  s'est remis « normale » lui-même laisse passer ses propres enfants, car
  ceux-ci ont pu hériter à travers lui. La famille est bornée à 2048 membres.
  Chaque membre est évalué une seule fois (priorité illisible : trois essais).
  Trouvé sous la normale, il est relevé en « normale » avec le mode efficacité
  par défaut, en un seul essai ; un héritier qui se rabaisse ensuite lui-même
  n'est pas poursuivi.
- Un héritier trouvé sous la normale est **toujours** relevé tout de suite,
  listé ou non. S'il est listé et coché, le relevé des apps (déclenché sans
  attendre) le met ensuite en veilleuse comme n'importe quelle app listée, en
  notant l'état qu'il lit : « normale », qu'il ait hérité ou non du mode
  efficacité. Un nouveau processus listé dont le parent n'est dans aucun
  processus du relevé (orphelin : l'intermédiaire a vécu moins d'un pas) est
  relevé d'abord de la même façon ; un nouveau processus listé dont le parent
  est un processus vivant étranger garde l'état lu comme état d'avant. Si le
  geste de mise en veilleuse échoue, l'entrée ne sort du journal que si l'état
  lu est celui qu'on avait noté.
- Le jeu et ses ancêtres ne sont jamais ralentis : le jeu et ses ancêtres
  absents du journal, trouvés bas, sont relevés (en « normale », mode efficacité
  par défaut, comme tout ce qui a hérité) ; un ancêtre du journal est remis à
  chaque pas, y compris après un changement de jeu. Les enfants du jeu relancé
  par une app en veilleuse sont relevés avec lui, si le jeu a pu être relevé
  (un jeu lancé en administrateur ne l'est pas : voir les limites).

**Les morts.** Une entrée du journal absente du relevé n'en sort que si Windows
prouve que le processus n'existe plus : `liveness` rend `Dead`, `Alive` ou
`Unknown` (accès refusé, relevé incomplet), et seul `Dead` retire l'entrée.
Sinon l'entrée garde son état d'avant et la remise la retentera. Un relevé
illisible (vide, ou sans le jeu alors que son processus n'est pas prouvé mort)
suspend seulement les morts, la famille, la mise en veilleuse des nouveaux et la
fin « plus aucun processus du jeu » ; les autres fins (mode automatique éteint,
« Ce n'est pas un jeu », dix minutes d'arrière-plan, nouveaux essais de remise)
passent ; mais une remise dont le relevé frais est illisible n'est pas finie :
le journal reste tel quel (rendre deux fois son état à une entrée est sans
danger), la remise est en attente et rejouée au délai habituel, quand le relevé
est de nouveau lisible. `liveness` dit aussi `Dead` quand le numéro appartient
maintenant à une autre session Windows (un processus ne change jamais de
session), mais seulement si les deux ouvertures du processus échouent et que les
deux sessions se lisent ; « terminé » se lit par une attente nulle sur le
processus, sans dépendre du code de sortie, seulement si `SYNCHRONIZE` est
accordé (sinon, repli sur le code de sortie).

**Une seule remise en place** sert à la fin de partie, à « Tout remettre », aux
nouveaux essais, à la reprise après plantage et à `--quitter` / `--retirer`,
avec ou sans mémoire. Au plus trois tours : (a) chaque entrée retrouve son état
d'avant par identité exacte (un processus mort sort, un processus illisible
reste en attente) ; (b) un relevé frais, puis la passe de famille en mode
remise, qui relève tout membre entrant trouvé sous la normale, listé ou non ;
(c) un tour de plus si quelque chose a été relevé. Le journal n'est écrit
qu'après (b) : un relevé frais est donc pris avant toute écriture, y compris
pour « Tout remettre ». Une remise en cours de partie (app décochée ou regardée,
app devenue le jeu) n'a pas de relevé frais : ses émetteurs restent « à suivre »
pour la passe suivante. Sans mémoire (reprise après plantage, ordres sans
agent), il n'y a pas de filtre « nouveau », les points de départ sont les émetteurs trouvés
vivants, et tout membre sous la normale est relevé une fois ; une entrée morte
ne donne rien, et une entrée du journal, même tout juste remise, n'est jamais
traitée comme un héritier (une app déjà basse avant la partie retrouve son état
d'avant, rien d'autre). Après un redémarrage du PC, aucune identité du journal ne
correspond : rien n'est touché, sans qu'on ait à détecter le redémarrage.

**Les dates.** `since` est le début du jeu affiché ; il ne sert qu'à
l'affichage. Les délais (deux pas pour confirmer un jeu, dix minutes
d'arrière-plan, trente secondes de relevé, soixante secondes avant de réessayer
une écriture, les deux minutes de l'agent lancé par Windows) se comptent avec
l'horloge monotone (`GetTickCount64`), jamais avec l'heure murale ; aucun n'entre
dans la filiation. Un ancien `game.json` reste lisible : les champs `opened`,
`openedUptime` et `roots` (avec `goneMs`, `goneUptime`) sont ignorés et ne sont
plus écrits.

**Limites connues.**

- Un programme non listé lancé par un intermédiaire qui a vécu moins de deux
  secondes, jamais vu, garde la priorité basse héritée. Pendant un relevé
  illisible de plusieurs pas, la même chose vaut pour tout intermédiaire né et
  mort pendant l'aveuglement.
- Au-delà de 2048 membres vivants dans la famille, les nouveaux héritiers ne sont
  plus suivis, donc plus relevés.
- Une remise en attente (un processus illisible ou un relevé illisible) bloque
  l'ouverture d'une nouvelle partie jusqu'à ce qu'elle finisse (au plus dix
  minutes), et le jeu n'est pas surveillé pendant ce temps.
- Un processus sorti avec le code 259 et dont une poignée reste ouverte ailleurs
  est vu vivant par `exe_path` et `apply` (jamais un abaissement à tort) ; `liveness`
  n'a pas ce défaut quand `SYNCHRONIZE` est accordé.
- Un pid repris par un étranger : l'émetteur mort reste un point de départ un
  relevé de plus, donc un enfant bas de lui-même du nouveau porteur peut être
  relevé pendant deux pas au lieu d'un (bénin : on relève, on n'abaisse jamais).
- Un jeu lancé « en tant qu'administrateur » par une app en veilleuse hérite de
  sa priorité basse, et l'agent, non élevé, ne peut pas le relever : il reste bas.
- Si le pid du jeu est repris par un processus illisible (un service), `liveness`
  rend `Unknown` et la fin de partie peut être retardée de dix minutes, avec un
  bilan « incomplet » ; le repli par la session n'a probablement pas d'effet
  (à vérifier sur un vrai Windows).
- Un plantage entre une remise en cours de partie et le pas suivant laisse
  passer l'enfant né dans cet intervalle (quelques millisecondes).
- Reprise après plantage : les enfants nés pendant que l'agent était arrêté, de
  parents déjà morts, ne sont pas retrouvés ; sans mémoire, un assistant qui
  s'était baissé lui-même peut être relevé.
- Un héritier qui se rabaisse lui-même après son relèvement n'est pas repris
  (un essai par identité).
- L'état d'avant d'un héritier listé est « normale », mode efficacité par défaut,
  que le mode efficacité soit hérité ou non (non vérifié sous Windows).
- Après un redémarrage du PC avec le même jeu ouvert, la partie reprise peut
  afficher une durée fausse (cosmétique).

**Les échecs.** Un processus dont la priorité a changé mais pas le mode
efficacité est remis tel qu'il était (`SetPriorityClass`), et le cœur relit son
état avant de le laisser de côté ; si l'état a bougé, l'entrée reste au journal.
Cette marche arrière ne vaut que pour un geste qui *baissait* la priorité : une
remise dont la deuxième étape échoue garde la priorité rendue et rend l'erreur,
pour que la suite retente le reste. Le journal n'est écrit que si son contenu
change. Un journal qui ne s'écrit pas est marqué sale et réécrit au début de
chaque pas : dès que l'écriture passe, le disque dit la vérité ; un agent qui
voudrait s'arrêter avec un journal en retard attend jusqu'à cinq pas pour
l'écrire. Un `settings.json` illisible n'est relu que toutes les 30 secondes (ou
dès que sa date change). Les tentatives de remise sont
cumulées sur toute la partie (relâchement compris).

Une session se termine quand plus aucun processus du jeu ne vit, quand le jeu
n'a pas été au premier plan depuis dix minutes (un lanceur resté ouvert ne garde
pas le PC en mode jeu), quand il n'est plus reconnu, ou quand la personne éteint
le mode automatique. Si quelque chose ne peut pas être remis (Windows refuse de
changer l'alimentation), la session reste en attente, la remise est retentée
toutes les 30 s, et la page le dit ; au bout de vingt essais, le bilan note que
tout n'a pas pu être remis.

`game.json` peut être écrit par n'importe quel programme du compte : sa lecture
est méfiante (classes de priorité connues seulement, jamais « temps réel »,
chaînes bornées, identifiants d'alimentation au format exact, fichier de plus
de 1 Mio traité comme illisible, vingt jeux vus au plus). Il en va de même de
`settings.json` : plus de 1 Mio, il est illisible, et un chemin de jeu de plus
de 1 024 signes est écarté.

**À la désinstallation.** Sans agent qui réponde à `--quitter` ou `--retirer`,
le programme qui envoie l'ordre fait lui-même la reprise : il lit le journal et
remet ce qui y est noté. Pour `--retirer`, si quelque chose résiste, quelques
essais rapprochés (cinq, à 200 ms) suivent, car après la désinstallation plus
rien ne le reprendrait. Pour `--retirer`, la valeur `Run` du compte qui envoie
l'ordre est effacée dans tous les cas, agent ou pas ; celle des autres comptes
du PC ne l'est pas (voir « Limites connues »).

### Limites connues

- **Un ordre `--quitter` ou `--retirer` lancé pendant qu'un agent démarre peut
  ne pas être entendu** : pendant la reprise et la lecture des réglages, le
  mutex de l'agent existe mais pas encore sa fenêtre. L'ordre n'arrive pas, la
  reprise sur place tourne alors en même temps que l'agent, et l'agent continue
  ensuite de tourner. Fenêtre de quelques centaines de millisecondes.
- **Une désinstallation élevée par un autre compte** que celui de la personne
  (un compte standard élevé avec les identifiants d'un administrateur), quand
  aucun agent de la personne ne répond à l'ordre, vise le profil de
  l'administrateur : `--retirer` ne trouve pas le `game.json` de la personne et
  ne remet rien (si son agent tourne, il reçoit l'ordre : voir « Avant une version
  publique »). La valeur `Run` qui resterait pointe sur un
  programme absent, donc inerte ; le journal resté est repris au prochain
  démarrage de l'agent si Shinku est réinstallé.
- **La valeur `Run` des autres comptes du PC reste** après la désinstallation :
  l'installation est par machine, mais `--retirer` et le modèle de Tauri
  n'effacent que la clé `HKCU` du compte qui désinstalle. Elle pointe sur un
  programme absent, donc ne lance rien ; Windows la montre seulement comme une
  entrée de démarrage cassée. Le journal que l'agent de ce compte aurait laissé
  est repris si Shinku est réinstallé.
- **Les agents des autres sessions Windows ouvertes** (changement rapide
  d'utilisateur) ne reçoivent pas l'ordre de l'installeur, qui parle à la fenêtre
  du bureau courant, mais ils verrouillent le fichier : l'installeur les arrête
  de force, s'il en reste un en marche après la sortie de celui du compte. Leur
  journal est repris au prochain démarrage de leur agent (`--demarrage` remet
  d'abord en place la partie restée au journal).
- **Le module de mise à jour de Tauri ne passe pas par le désinstalleur** : il
  installe par-dessus, et la valeur `Run` n'est pas touchée. Le crochet sait
  pourtant garder la valeur `Run` d'un désinstalleur lancé avec `/UPDATE`
  (`--quitter` au lieu de `--retirer`), mais Tauri ne le lance jamais ainsi : la
  page de réinstallation saute avant d'ajouter `/UPDATE`. Le choix « désinstaller
  avant d'installer » de cette page lance l'ancien désinstalleur sans `/UPDATE` : la valeur
  `Run` est effacée, et le mode jeu ne démarre plus avec Windows tant que la
  personne n'a pas rouvert Shinku (qui la réécrit si le mode automatique est
  allumé).
- **Les jeux hors bibliothèque** (Battle.net, Minecraft, Roblox…) ne sont pas
  reconnus tout seuls : la personne les ajoute depuis la liste des apps ouvertes.
  Un jeu ajouté est retenu par le chemin exact de son programme : si ce chemin
  change à chaque mise à jour (Roblox range le sien dans un dossier au nom de la
  version, comme les anciens jeux du Store sous `WindowsApps`), l'ajout cesse de
  valoir sans rien dire, et il faut l'ajouter de nouveau.
- **La bibliothèque déplacée** : seuls les dossiers habituels des boutiques sont
  reconnus. Un jeu installé dans un dossier que la personne a choisi (`D:\Epic\…`)
  s'ajoute à la main.
- **Deux écrans** : seule l'app au premier plan retrouve sa vitesse. Une vidéo
  regardée sur le second écran dans une app en veilleuse reste ralentie.
- **Les jeux UWP** passent par `ApplicationFrameHost.exe`, que le veto écarte :
  la fenêtre au premier plan n'est pas celle du jeu.
- **Un anti-triche** peut refuser d'ouvrir le processus du jeu : Shinku ne le
  voit pas au premier plan, ou ne peut pas savoir s'il vit encore.
- **Le nom du mutex** de l'agent (`Local\Shinku.Agent`) peut être pris à
  l'avance par un autre programme du même compte, qui empêcherait l'agent de
  démarrer. Il n'y gagne rien d'autre.
- **Aucun vrai jeu n'a encore été essayé** : l'essai de bout en bout utilise un
  faux jeu, et les effets de la veilleuse sur un jeu précis ne sont pas
  mesurés.

## Démarrage

« Démarrage : gérer les programmes qui se lancent avec Windows. » La page liste
les apps **inscrites** au démarrage et permet d'en couper une d'un interrupteur,
**au même endroit que le Gestionnaire des tâches** : l'app reste installée,
rien n'est supprimé, et elle se remet d'un clic, dans Shinku ou dans Windows.
Tout est dans `crates/shinku-core/src/startup/` (`mod.rs`, `command.rs`,
`approval.rs`, `host.rs`, `store.rs`, `view.rs`, `desk.rs`, `access.rs`,
`launcher.rs`, `shown.rs`), `helper/` (`request.rs`, `location.rs`, `startup.rs`) et,
pour Windows, `platform/windows/` (`startup.rs`, `launcher.rs`, `helper_host.rs`,
`security.rs`).

Les règles du produit :

- rien n'est modifié sans un geste de la personne, et tout changement se défait ;
- **pas de chiffre inventé** : aucun « impact », aucune durée de démarrage ;
- **aucune phrase ne promet plus que ce que Shinku lit**. Shinku ne voit ni les
  apps du Microsoft Store, ni les tâches planifiées, ni les services : on dit
  « apps inscrites au démarrage », jamais « rien ne se lance » ni « plus rien » ;
- Shinku ne supprime aucune entrée de démarrage : il ne fait que l'approuver ou la
  désapprouver, là où Windows le fait ;
- `shinku.exe` n'écrit rien sous `HKLM` ; le PC se règle par l'assistant (voir « Régler une entrée du PC »).

### Les six places

| Place | Lettres | Où sont les entrées | Clé d'approbation (valeur du même nom) | Portée |
| --- | --- | --- | --- | --- |
| `UserRun` | `ur` | valeurs de `HKCU\…\CurrentVersion\Run` | `HKCU\…\Explorer\StartupApproved\Run` | compte |
| `UserRun32` | `uw` | valeurs de `HKCU\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Run` (chemin littéral : `HKCU\Software` n'est pas redirigé) | `…\StartupApproved\Run32` | compte |
| `UserFolder` | `uf` | fichiers du dossier Démarrage du compte (`FOLDERID_Startup`) | `…\StartupApproved\StartupFolder` (nom = nom du fichier, extension comprise) | compte |
| `MachineRun` | `mr` | valeurs de `HKLM\…\Run`, vue 64 bits | `HKLM\…\StartupApproved\Run` | PC |
| `MachineRun32` | `mw` | la même clé, vue 32 bits (`KEY_WOW64_32KEY`) | `HKLM\…\StartupApproved\Run32` | PC |
| `MachineFolder` | `mf` | fichiers du dossier Démarrage commun (`FOLDERID_CommonStartup`) | `HKLM\…\StartupApproved\StartupFolder` | PC |

Les clés d'approbation s'ouvrent en vue 64 bits. Une place dont la clé n'existe
pas est une place vide, pas une erreur ; une place qu'on ne peut pas lire est
signalée à la page (« Windows n'a pas laissé lire toute cette liste »). Seules
les valeurs `REG_SZ` et `REG_EXPAND_SZ` de `Run` sont des entrées ; dans les
dossiers, tous les fichiers sont des entrées sauf `desktop.ini`, **cachés
compris** (ne pas cacher ce qui se cache), sans les sous-dossiers. Un **lien**
du dossier Démarrage est listé sous son nom (l'Explorateur le lance), réglable,
de cible inconnue, et **jamais suivi** : ni sa cible lue, ni ses métadonnées
demandées. Un lien vers un **dossier** (jonction, lien symbolique de dossier) n'est
pas une app : il est écarté comme un sous-dossier. La boucle du dossier est
bornée comme celle du registre : 4 096 éléments examinés, gardés ou non, puis la
place est dite incomplète.

**Les bornes se disent, elles ne se taisent pas.** Il y a des bornes : 300
entrées par place, 255 caractères pour un nom, 4 096 valeurs examinées dans une
clé `Run`, 1 Mio pour une valeur, 2 048 caractères pour une commande. Quand l'une
d'elles écarte quelque chose (la 301e entrée, un nom trop long, une valeur
démesurée, plus de 4 096 valeurs), la place est dite **incomplète** : la page
reçoit la portée dans « illisible » (« Windows n'a pas laissé lire toute cette
liste »), et le résumé de l'analyse reste « Non mesuré » au lieu d'un chiffre
qu'un programme du compte aurait pu fausser en ajoutant des leurres. Une
**commande trop longue** n'écarte pas l'entrée : elle est gardée, sans commande,
donc de cible inconnue (les commandes très longues sont celles des maliciels
« sans fichier »). Un **texte qui n'est pas de l'UTF-16 valide** (commande d'une
valeur `Run`, cible d'un raccourci, chemin d'un fichier du dossier) ne devient
jamais un chemin qu'on cherche sur le disque : la commande est vide, la cible
inconnue (une conversion approximative ferait dire « semble désinstallée » d'un
programme bien présent). Un nom qui n'est pas de l'UTF-16 valide, ou qui contient un
zéro, est gardé comme **approximatif** : montré, jamais réglé, et son
approbation n'est pas lue (la chercher sous un nom approximatif ne trouverait
rien et le ferait passer pour « actif »). Le cœur applique lui-même la borne de
300 entrées : un hôte qui en rendrait davantage est tronqué et dit incomplet.

### Le format d'approbation, et pourquoi la lecture est stricte

La valeur d'approbation est un `REG_BINARY` de **12 octets** : un état sur
quatre octets, puis un `FILETIME` de huit octets (petit-boutiste).
**Ce format n'est pas documenté par Microsoft.** C'est celui que le
Gestionnaire des tâches écrit, observé par des tiers, et les sources se
contredisent sur tout ce qui n'est pas `02` et `03` (on voit aussi `06` et `07`
dans la nature, sans explication sûre ; la règle « impair = coupé » qui circule
n'est confirmée nulle part). Shinku lit donc **strictement** (`approval.rs`) :

| Ce que Shinku lit | État |
| --- | --- |
| valeur absente | active |
| `REG_BINARY`, 12 octets, les quatre premiers `02 00 00 00` | active |
| `REG_BINARY`, 12 octets, les quatre premiers `03 00 00 00` | coupée ; la date est le `FILETIME` s'il tombe entre 2012 et maintenant, sinon pas de date |
| **tout le reste** : `06`, `07`, une autre longueur, un autre type, une lecture refusée | **illisible** |

Une entrée illisible est montrée **sans interrupteur** (« Shinku ne sait pas lire
son réglage »), avec un bouton qui ouvre Windows, et Shinku n'y écrit **jamais**.
Mieux vaut ne pas toucher à un réglage qu'on ne comprend pas que d'en écraser
un que Windows lisait autrement.

Shinku n'écrit que deux formes, et seulement pour une entrée **du compte** lue
active ou coupée :

- couper : `03 00 00 00` et le `FILETIME` de maintenant ;
- remettre : `02 00 00 00` et huit zéros.

Après chaque écriture, la valeur est **relue** : si elle ne se lit pas comme
l'état voulu, le changement est un échec (`failed`) et la page le dit.

### Ce qui n'est pas listé, et pourquoi

- **Les apps du Microsoft Store** (MSIX, `windows.startupTask`) : leur état est
  gardé ailleurs et aucune API publique ne permet de régler l'app d'un autre.
- **Les tâches planifiées** et **les services** : ce ne sont pas des entrées
  de démarrage au sens du Gestionnaire des tâches, et les régler est une autre
  décision, plus lourde.
- Les clés `RunOnce`, les `Winlogon\Shell` et autres recoins : le Gestionnaire
  des tâches ne les règle pas non plus.

La page le dit en toutes lettres, pour qu'une liste courte ne soit pas lue
comme « c'est tout ce qui démarre ».

### Lire une ligne de commande

`command.rs` répond à une question : de quel programme parle cette commande ?
Ce sont des fonctions pures, qui ne parlent à la machine que par `StartupHost`.

1. les variables `%…%` sont développées, les blancs de tête retirés ;
2. un chemin réseau (`\\…`, `//…`) est **inconnu avant tout accès au disque**, et
   un chemin dont le lecteur n'est pas **fixe** (`GetDriveTypeW`, `DRIVE_FIXED`
   seulement : ni lecteur réseau, ni amovible, ni absent) l'est aussi : Shinku ne
   cherche rien et ne lit aucune ressource de version hors d'un lecteur fixe, si
   bien que lire une entrée ne déclenche ni connexion ni attente du redirecteur
   réseau (limite : une jonction ou un lien placé *dans* un chemin d'un lecteur
   fixe, qui mènerait ailleurs, n'est pas détecté ; le fichier final lui-même
   n'est jamais suivi s'il est un lien) ;
3. une commande qui commence par `"` désigne le programme jusqu'au guillemet
   fermant (non fermé : jusqu'à la fin) ;
4. sinon, comme `CreateProcess` : les préfixes coupés aux espaces, du plus court
   au plus long, avec et sans `.exe` ajouté ; le premier qui est un fichier
   gagne, sinon le programme est le premier mot. Au plus 64 préfixes sont
   essayés : si d'autres ont été laissés de côté et qu'aucun n'est un fichier, la
   cible est **inconnue**, jamais manquante ;
5. un programme sans dossier est cherché dans `System32` puis dans le dossier de
   Windows (`GetSystemWindowsDirectoryW`, jamais l'environnement) ; introuvable,
   il est **inconnu**, jamais « introuvable » (on ne connaît pas le `PATH`) ;
6. un chemin absolu n'est **manquant** (« semble désinstallée ») que si la machine
   répond « introuvable » avec certitude. La machine répond en réalité par
   trois mots : *fichier*, *absent* (seulement « introuvable »), *incertain*
   (accès refusé, nom invalide, lien, lecteur qui ne répond pas). Un **dossier**
   (un raccourci vers un dossier est courant) et tout *incertain* donnent
   **inconnu** : jamais une affirmation fausse. Un dossier qui n'est qu'un
   préfixe de la commande (`C:\Program Files` dans `C:\Program Files (x86)\…`)
   n'arrête pas la recherche, comme pour `CreateProcess`. Un chemin relatif
   n'est jamais cherché (il dépendrait du dossier courant) ;
7. un programme **hôte** (`rundll32`, `cmd`, `powershell`, `java`, `python`,
   `node`…) n'est que le porteur : on n'en tire ni nom ni éditeur, la page dit
   « Lancée par rundll32.exe ».

Un raccourci se lit avec `IShellLink` en `SLGP_RAWPATH`, **sans le résoudre ni
le réparer** ; s'il n'a pas de cible lisible, l'entrée est le fichier lui-même.
Une cible qui remplirait le tampon (32 768 caractères) n'est jamais gardée
tronquée : la cible reste inconnue. Un dossier Démarrage **redirigé hors d'un
lecteur fixe** (partage réseau d'un profil d'entreprise) n'est pas lu : la place
est dite illisible.

### Noms, éditeurs, identifiants

- Le nom affiché est la description des ressources de version du fichier, sauf
  si elle est vide ou **générique** (`Update`, `Launcher`, `Setup`, `Helper`,
  `Electron`, `App`, `Service`, `Agent`, `Tray`, `Startup`…) ; sinon le nom
  inscrit, sans `.lnk`. Le nom inscrit devient un « alias » s'il diffère.
- L'éditeur est **ce que le fichier déclare** : ce n'est pas une identité
  vérifiée, et la page le dit.
- Tout texte montré est nettoyé : caractères de contrôle et de format retirés
  (dont U+202E, qui renverse le sens d'un texte), blancs réduits, 80 caractères.
- L'**identifiant** d'une entrée est fait des deux lettres de sa place (`ur`,
  `uw`, `uf`, `mr`, `mw` ou `mf`), d'un tiret, puis du FNV-1a 64 bits des unités UTF-16 du nom brut (deux octets par
  unité, poids faible d'abord), **sans changer la casse**, en 16 chiffres
  hexadécimaux minuscules. La page ne connaît que lui. Deux entrées d'une
  même énumération qui tombent sur le même identifiant deviennent toutes deux
  non réglables.
- L'entrée de Shinku lui-même (sa valeur `Run`, pour le mode jeu automatique) se
  reconnaît à son nom **et** à la commande exacte que le mode jeu écrit pour
  cette installation (`platform::agent::run_command`). Elle ne se règle pas ici :
  la page renvoie au Mode jeu.

### Les apps qui se réinscrivent, et `startup.json`

Windows ne dit pas **qui** a coupé une entrée. `startup.json` (dans
`%LOCALAPPDATA%\Shinku\`, à côté de `game.json`) garde, pour chaque entrée que
Shinku a coupée, son identifiant, son nom, la date, et le `FILETIME` écrit.
À chaque vue :

- l'entrée est coupée **avec le même `FILETIME`** : « coupée par Shinku » ;
- coupée avec un autre : elle a été recoupée ailleurs, la ligne est retirée ;
- active **sans valeur d'approbation** : elle s'est remise sans Shinku. Certaines
  apps, celles bâties sur Electron notamment, **effacent l'approbation quand
  elles se réinscrivent**. Shinku note la première observation, dit « s'est
  remise toute seule » pendant 14 jours, puis retire la ligne : la page invite à
  régler aussi dans les options de l'app. La date de première observation est
  bornée par maintenant (un fichier trafiqué ne garde pas la marque pour
  toujours) ;
- active **avec** une valeur `02 00 00 00` : quelqu'un l'a rallumée dans Windows
  (le Gestionnaire des tâches écrit cette forme). Shinku ne peut pas dire
  qu'elle s'est remise « toute seule » : la coupure notée est **oubliée, sans
  marque** ;
- l'entrée n'existe plus : la ligne est retirée ;
- une place illisible **ou lue en partie**, un réglage illisible ou un
  identifiant ambigu : la ligne est **gardée telle quelle** (on ne conclut rien
  de ce qu'on ne voit pas).

La lecture du fichier est méfiante : absent, illisible ou de plus de 1 Mo, il
vaut un fichier vide (jamais d'erreur montrée) ; une ligne mal formée est
ignorée ; 200 lignes au plus (les plus anciennes sortent) ; il n'est réécrit que
si quelque chose a changé, par remplacement (jamais un fichier à moitié écrit).
Rien de ce qu'il dit ne sert à écrire dans le registre.

### Le guichet

Deux commandes : `startup_view` (sans argument) et `set_startup_entry`
(`id`, `enabled`), qui rendent chacune la vue entière. Un seul changement à la
fois : un second appel simultané est refusé (`busy`). Les refus ont une raison
(`unknownEntry`, `notAdjustable`, `busy`, `failed`). La page d'ouverture des
réglages de Windows (`open_windows_settings`) gagne la page `startupApps`
(`ms-settings:startupapps`).

Le résumé de l'analyse compte les entrées **actives** hors celle de Shinku,
toutes portées : « {n} apps inscrites au démarrage de Windows ». Il est lu une
fois, à la fin du parcours des disques, par l'hôte du guichet ; si une place
est illisible **ou lue en partie** (une borne atteinte), ou si la machine ne sait
pas compter (l'hôte inerte des plateformes sans démarrage de Windows), la
catégorie reste « Non mesuré » plutôt que d'afficher un chiffre faux. Une
entrée au nom approximatif n'est pas comptée active.

Après un échec (`failed`), Shinku ne remet pas l'ancien état : la page recharge
la vue (`startup_view`), qui relit la machine et montre l'état réel, quel qu'il
soit. L'erreur est rendue seule, jamais accompagnée d'une vue qui pourrait
mentir.

### Limites connues

- **Ni les apps du Store, ni les tâches planifiées, ni les services** ne sont
  listés (voir plus haut).
- **Les entrées du PC sont en lecture seule** quand l'accès est `unavailable`
  (voir « Régler une entrée du PC ») : la page ouvre alors la page de Windows.
- **Pas d'icônes** : aucune icône n'est affichée dans ce lot, la page montre
  l'initiale du nom. Shinku lit pourtant les **ressources de version** du fichier
  (description et éditeur, `GetFileVersionInfoW`, qui charge le fichier), et
  seulement sur un lecteur fixe. Un fichier « en ligne seulement » d'un service de
  stockage (attributs `OFFLINE`, `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`) n'est
  jamais lu : il existe, mais sans description, car le lire le ferait télécharger.
  Lire la liste du démarrage ne télécharge rien.
- **« Absent » ne vient que des erreurs Windows 2 et 3** (fichier ou chemin
  introuvable, lues par le code brut) : un lecteur absent, un partage éteint ou
  un nom invalide sont des incertitudes.
- **Le format d'approbation n'est pas documenté** : une version future de
  Windows peut changer ce que `02` et `03` veulent dire. La lecture stricte
  limite les dégâts à un affichage « illisible », jamais à une écriture.
- **Les bornes** (300 entrées par place, noms de 255 caractères, valeurs de
  1 Mio) écartent ce qui les dépasse, **en le disant** : la place est incomplète,
  la page le signale, le résumé reste « Non mesuré ». Dans une place incomplète,
  ce qui est montré reste réglable.
- **Le mode jeu lit sa propre approbation comme le module** :
  `platform::windows::agent::autostart` passe par la même lecture stricte
  (`startup::approval`). Seul `03 00 00 00` + date est « coupé dans Windows » ;
  `06`, `07` ou toute autre forme illisible ne le fait pas dire.
- **Un chemin d'un lecteur fixe qui passe par une jonction ou un lien** menant à
  un partage n'est pas détecté (seul le fichier final n'est jamais suivi).
- **La course entre la relecture et l'écriture** (une valeur `Run` supprimée
  pendant les quelques microsecondes qui séparent les deux) peut laisser une
  approbation orpheline, sans effet : le Gestionnaire des tâches en laisse
  aussi.
- **Une coupure ne ferme pas l'app.** Si elle est ouverte, elle le reste : elle
  ne se lance plus toute seule à la prochaine ouverture de session.

### Ce que les essais ne prouvent pas : la recette à la main

Les tests du cœur (machine fabriquée), ceux de l'hôte Windows (sur un vrai
registre, sous une clé d'essai de `HKCU`, y compris sous Wine) et l'essai sur un
vrai Windows prévu dans l'intégration continue prouvent que Shinku lit et écrit
les bonnes valeurs aux bons endroits. Ils **ne prouvent pas que Windows respecte
ces valeurs à l'ouverture de la session** : aucune session ne s'ouvre sur la
machine d'essai.

Cette preuve est une **recette à la main sur un vrai Windows 11**, et une
**condition de sortie publique** : pour une entrée de chaque place du compte
(`Run`, `Run32`, dossier Démarrage), couper dans Shinku puis lire « Désactivé »
dans le Gestionnaire des tâches, fermer et rouvrir la session, constater que
l'app ne s'est pas lancée ; et l'inverse : couper dans Windows, lire « Coupée »
dans Shinku.

L'essai de la fenêtre sur le vrai Windows de l'intégration continue
(`essai-demarrage.ps1` et `checkStartup`, voir « Essayé avant d'être publié »)
**prouve** que, sur un vrai registre et un vrai dossier Démarrage, la page lit
les entrées de chaque place du compte et celle du PC, que couper ou remettre
dans la page écrit la valeur d'approbation attendue (douze octets, `03000000…`
pour coupée, `020000000000000000000000` pour remise) aux bons endroits, relue par
`reg query /reg:64`, et que la zone de retour et « Annuler » disent la vérité.
Il **ne prouve pas** que Windows respecte ces valeurs à l'ouverture de la
session : aucune session ne s'ouvre sur la machine d'essai. Cette preuve reste
la recette à la main ci-dessus.

### Régler une entrée du PC : l'assistant élevé

Régler une entrée de `HKLM` demande un administrateur. `shinku-helper.exe` est
lancé **une fois par changement** (`ShellExecuteExW`, verbe `runas`), avec
quatre mots : `demarrage1`, `couper` ou `remettre`, la place (`run`, `run32`,
`dossier`), le nom en UTF-16 hexadécimal minuscule. La page n'envoie que
l'identifiant ; le cœur retrouve la place et le nom. Tout est dans
`crates/shinku-core/src/helper/` (`request.rs`, `location.rs`, `startup.rs`) et
`platform/windows/` (`helper_host.rs`, `launcher.rs`, `security.rs`).

Règles de ce lot. Presque chacune a un essai qui échoue si on la retire ; les
exceptions sont dites sous « Ce qui n'est tenu que par relecture ».

- **l'assistant ne lit que des noms** : valeurs texte de `Run` dont le nom n'est pas
  vide (sans leurs données), noms de fichiers du dossier commun, et la seule valeur d'approbation ; ni variable, ni ressource
  de version, ni contenu de fichier ;
- **il se revérifie** : son fichier est `shinku-helper.exe`, sans lien, sûr
  (propriétaire Administrateurs, Système ou TrustedInstaller ; aucun droit
  d'écriture pour un autre) ; son **dossier** est l'enfant direct de
  `Program Files`, et ce dossier et `Program Files` sont sûrs aussi ; chemins
  comparés par composants, majuscules ASCII seulement ;
- **il montre ce qu'il fait** : une boîte `MessageBoxW` affiche l'entrée (nom
  sans caractère trompeur, `⟨U+XXXX⟩` pour les autres) ; échéance de deux
  minutes, **atomique** : une réponse tardive ne change rien ;
- **ordre** : demande valide, droits, emplacement sûr, noms, entrée trouvée
  **une seule fois** (et aucun autre nom égal à la casse près : code 4), état
  lu, question, puis les mêmes vérifications relues, écriture, relecture. Une
  valeur relue qui n'est pas celle écrite donne 8 ;
- **codes de sortie** : 0 fait ; 2 demande refusée ; 3 refusé ou échéance de la boîte ; 4
  entrée introuvable ou ambiguë (y compris disparue ou doublée pendant la
  question) ; 5 droits insuffisants ; 6 emplacement non sûr ; 7 réglage
  illisible ; 8 échec (écriture, place illisible, ou durcissement des
  bibliothèques refusé) ; **9 déjà dans cet état**. Le code 1 n'est **jamais**
  rendu : c'est celui d'un processus tué (`taskkill /F`, Gestionnaire des
  tâches), et le guichet le range dans « échec » comme tout code inconnu ;
- **le cœur ne croit pas le code de sortie** : il relit l'approbation et ne met
  `startup.json` à jour que d'après la valeur relue (et un code 0 pour une
  coupure) ;
- **pas de verrou pendant l'attente** : la fenêtre répond pendant la boîte ; une
  seconde demande donne « occupé » ; `pending` dit à la page quelle entrée
  attend, et n'est retiré qu'après la relecture et la mise à jour de
  `startup.json` ;
- **accès** (`pc` dans la vue), décidé dans cet ordre : assistant absent ou
  pas sûr, `unavailable` ; jeton limité d'une paire, `consent` (un clic) ;
  groupe Administrateurs **activé** dans le jeton, quel que soit le type,
  `direct` (Windows ne demande rien : la boîte de l'assistant est la seule
  question ; c'est le mode D) ; sinon `EnableLUA` ou `ConsentPromptBehaviorUser` à 0,
  `unavailable` ; sinon `password` (mot de passe d'un administrateur, y compris
  pour un jeton « complet » filtré) ;
- **refus** de la page : `cancelled` (accord ou boîte refusés, échéance de la boîte ; l'attente de dix
  minutes de `shinku.exe` donne `failed`),
  `needsAdmin`.

Fermer `shinku.exe` pendant l'attente n'arrête pas l'assistant : il termine ou
expire seul, **sans la marque « par Shinku »** (la marque ne vient que de la
fenêtre qui a vu la valeur relue). La désinstallation ne remet pas les entrées
coupées.

#### Liste fermée des fonctions de Windows

Une seule liste fait foi : **l'en-tête de
`crates/shinku-core/src/platform/windows/helper_host.rs`**, tenue d'après le
code. Cette section ne la recopie pas.

#### Ce qui n'est tenu que par relecture

Aucun essai ne voit ces règles si on les retire :

- « jamais `HKCU` » : un essai de source refuse `HKCU` et `Hive` dans le code de
  l'assistant (une ruche écrite par sa valeur numérique y échapperait), et il
  vaut pour **le code de Shinku, pas pour tout le processus** (`user32` et le
  cadre de saisie lisent ce qu'ils veulent dans le `HKCU` du compte élevé) ;
- la poignée du fichier de l'assistant reste ouverte **jusqu'au retour de
  `ShellExecuteExW`** et pas après (retirer le `drop` ou le placer plus tôt ne
  casse rien) ;
- COM est initialisé avant `ShellExecuteExW` ;
- dans `ask`, la sortie du processus avec le code 3 à l'échéance (la décision
  « réponse ou échéance » est essayée dans `ask_with`, pas l'appel à
  `process::exit`) ;
- l'énumération du dossier commun passe par `common_folder` : `names()` pourrait
  ouvrir un autre dossier (la fonction d'énumération, elle, ne prend qu'une
  poignée vérifiée et n'énumère que son chemin final) ;
- que la poignée de `run()` demande l'accès aux données (`open_helper(.., true)`)
  n'est prouvé que par l'essai de la vraie installation
  (`SHINKU_ESSAI_ASSISTANT`, sur NTFS, élevé : écriture et suppression refusées
  par violation de partage), pas par les essais ordinaires.

Pour la Confidentialité, deux courses ne sont pas essayables simplement et ne sont tenues
que par relecture : l'état du grand vide est lu **après** la prise du verrou d'exclusivité
(`Backend::erase_traces` ; lu avant, aucun essai ne tombe) ; un lien qui apparaît entre
l'énumération d'une clé de registre et la suppression d'une de ses valeurs (le faux hôte ne sait
pas le rendre en pleine suppression).

Sont en revanche tenus par un essai : la sortie avec le code 8 de `main` si le
durcissement échoue (essai de structure du crate de l'assistant), la décision de
l'échéance, la lecture des droits (descripteurs fabriqués), les valeurs de
`Run`, le dossier commun refusé.

#### Limites connues

- **La poignée du fichier** (accès aux données, partage en lecture seule) empêche
  de réécrire ou supprimer le fichier entre le contrôle et le retour de
  `ShellExecuteExW`. C'est un garde-fou contre nos erreurs, pas une barrière,
  et rien n'est promis sur le renommage du dossier. Elle n'est demandée que par
  le lancement (`run()`) : `access()` et la vérification que l'assistant fait de
  lui-même ouvrent sans accès aux données, pour qu'un outil qui tient le fichier
  ne fasse pas disparaître les interrupteurs. Les essais qui la prouvent sont
  sautés sous Wine et tournent sur un vrai NTFS en intégration continue.
- **Les bibliothèques.** L'assistant lie tout `shinku-core` : des bibliothèques
  sont chargées avant `main`, donc avant `SetDefaultDllDirectories`. Sur le
  binaire publié, le drapeau de l'éditeur de liens
  (`/DEPENDENTLOADFLAG:0x800`, MSVC seulement) doit les tenir à `System32` : le
  drapeau est posé à la construction, sa présence est lue sur le binaire
  installé par le script d'essai, et son effet sur le chargeur est le
  comportement documenté de Windows, pas prouvé par nous. **Un petit
  crate dédié à l'assistant reste à faire avant la sortie publique.**
- **Le nom montré** garde les marques combinantes et l'écriture de droite à
  gauche (au plus 80 caractères, mais ils peuvent s'empiler et réordonner les
  « » voisins dans la boîte). Conforme à la décision, et ces noms sont écrits
  par des administrateurs.
- Le dossier de l'assistant est vérifié, pas le contenu du fichier : la
  signature viendra avec la version publique.
- Une entrée dont le nom dépasse 255 unités, ou dont la place est un dossier non
  sûr, est montrée mais non réglable.
- L'appel `set_startup_entry` reste bloqué sur un fil du moteur de Tauri jusqu'à
  la fin de l'assistant (deux minutes au plus, plus l'accord de Windows) ; la
  page répond, mais un clic refusé « occupé » ne libère pas ce fil.

#### Ce que seul un vrai PC montre (D10)

Droits réels de `Program Files`, fenêtre de Windows (consentement, mot de passe,
refus), apparence de la boîte et ce qu'un programme du compte peut en faire,
fichier gardé ouvert pendant `ShellExecuteExW`, `GetFinalPathNameByHandleW` sur
un vrai volume, effet du réglage à l'ouverture de session. Les essais sous Wine
ne le prouvent pas (ni droits, ni élévation, ni partage de fichiers). L'essai
`SHINKU_ESSAI_ASSISTANT` (nom inconnu, code 4, jamais de boîte) se lance à la
main sur l'installé.

**Vu sur le vrai Windows de l'intégration continue** (Windows 11 build 26100,
passage du 16 novembre 2026, `essai-assistant.ps1` et l'essai du lanceur, verts au
premier passage) : le vrai lanceur démarre l'assistant installé, qui répond 4 pour un
nom inconnu ; pendant le lancement, le fichier gardé ouvert refuse une écriture et une
suppression (erreur 32) ; un dossier créé sous `Program Files` est accepté avec ses
droits de départ et refusé (code 6) dès qu'« Utilisateurs » peut y écrire ; OK écrit
les douze octets attendus (`03 00 00 00` puis la date) et rien d'autre dans `HKLM`,
Annuler n'écrit rien (code 3), « déjà dans cet état » rend 9 sans boîte, l'échéance
tombe à 120,0 s (code 3) ; un programme d'intégrité moyenne ne peut ni cliquer ni
fermer la boîte (erreur 5) ; l'assistant publié porte `requireAdministrator` et
`DependentLoadFlags = 0x0800`. La page, lancée avec un jeton filtré, lit l'accès
`password` et montre l'interrupteur, sans clic. **Restent pour un vrai PC** : la
fenêtre d'accord de Windows elle-même (la machine d'essai est déjà élevée, personne
n'y répond), ce qu'elle écrit pour un assistant non signé, et l'effet du réglage à
l'ouverture de session.

## Le Journal

Une page, « Journal », qui rassemble ce que Shinku a fait, la ligne la plus
récente en haut. Elle ne fait rien : elle se lit. Code : `crates/shinku-core/src/history/`
(`assemble`, pure, essayée sous Linux), `Backend::history_view`, et la page
`views/journal.ts` avec `model/journal.ts`.

**Quatre sources, aucune écriture de plus pour trois d'entre elles.**

- Les grands vides : `journal.json`, tel quel. Il est lu **avec borne** : plus de
  1 Mio, il est vide (comme un fichier illisible, la même `read_bounded` que les
  autres magasins), et la lecture ne garde que les 100 derniers grands vides.
  Un grand vide resté « en cours » est noté coupé **une seule fois, au lancement,
  par la seule fenêtre** : `Backend::close_interrupted_clean`, appelée dans le
  crochet `setup` avant l'ouverture de la fenêtre (la page demande `overview`
  dès qu'elle s'ouvre, l'annonce doit déjà y être). Elle ne sert qu'une fois par
  guichet : un second appel marquerait le grand vide en cours de cette fenêtre.
  Voir « Une seule fenêtre ». Un `journal.json` de plus de 1 Mio est lu vide,
  donc **remplacé** au prochain grand vide (comme un journal illisible) : c'est
  accepté, car Shinku n'écrit jamais plus de 100 grands vides, soit quelques
  dizaines de Kio ; un fichier plus gros n'est pas le sien.
- Les coupures du Démarrage : `startup.json`, lu **sans** le relire dans le
  registre (`StartupDesk::cuts`) : ouvrir le Journal ne touche pas au registre.
  Une ligne dit, quand l'entrée s'est rallumée, « S'est remise toute seule depuis… »,
  avec la formule de la page Démarrage.
- La dernière partie du mode jeu : `game.json`, par `GameDesk::last`. Le mode
  jeu ne garde que la dernière partie : la page le dit. Elle écrit la durée
  seulement si la fin est après le début (une fonction commune, `formatSpan`,
  qui sert aussi la page Mode jeu : l'ancien « 1 min » d'une fin avant le début
  est parti).
- Les désinstallations : `uninstalls.json`, **nouveau**, écrit par `AppsDesk`
  (`apps/notes.rs`). Il n'y avait nulle part où lire ce qu'une désinstallation
  avait donné : c'est la seule écriture que le Journal ajoute.

**Quand une désinstallation est notée.** Au passage du suivi de `Settling` à
`Searching` : l'app a disparu du registre (la vérité, voir « Désinstallation »)
et le désinstalleur a fini. Le suivi lève alors un drapeau (`take_gone`, sur le
modèle de `take_kept`) ; `AppsDesk` écrit la note **hors du verrou du suivi**,
avec l'horloge de la machine. Pas de note si la personne annule, si le
désinstalleur échoue, si 30 minutes passent sans que l'app disparaisse, ni pour
une mise à jour MSI qui garde l'app. Pas de seconde note quand la liste des
restes est recalculée. Les restes mis à la corbeille s'ajoutent à la même note
(seul `Removed` compte, pas `AlreadyGone`). Une écriture qui échoue ne change
rien et n'emprunte pas la clé de la note précédente.

**Ce que la page montre, et ne montre pas.** Une phrase par ligne, une date, et
quand le geste a un retour (la corbeille, la page Démarrage, la page Mode jeu)
un bouton qui y mène. Aucun chemin, aucun identifiant : un nom d'app est du
texte, écrit en entier, qui passe à la ligne comme le reste de la phrase. Une
date avant 1970, ou plus d'un jour après maintenant, devient « date inconnue »
(le cœur l'assainit). Une date du lendemain s'écrit avec son jour, pas comme une
heure d'aujourd'hui. Les poids et les compteurs ne dépassent jamais
`2^53 - 1`, le plus grand entier que la page lit sans erreur. 200 lignes au
plus, `more` le dit. La page se demande **une fois, quand on y arrive** ; elle
ne se redemande pas toute seule (pas de minuterie), ni quand la fenêtre
retrouve le focus : seul un grand vide qui se termine pendant qu'elle est
ouverte la relit, une fois. Pour que cette relecture ne redessine pas la page
entière, le Journal a sa propre clé de page (`pageKey`, qui ne dépend pas du
volet) ; les essais tiennent la valeur de cette clé et son emploi dans
`appView`, pas le focus lui-même, qu'aucun essai automatique ne regarde.

**« Oublier les désinstallations ».** Un bouton, après confirmation, vide
`uninstalls.json` (`Backend::forget_uninstalls`) et rend la vue relue. Il
n'existe que s'il y a une ligne de désinstallation. Les autres sources ont leur
propre page pour s'effacer ou s'oublier.

**« À propos », dans les Paramètres.** La version, la phrase « Shinku
n'envoie rien : ni compte, ni télémétrie, ni publicité », et « Voir leurs
licences ». Le bouton appelle `open_licences` : le cœur cherche
`LICENCES-TIERS.txt` à côté de `shinku.exe`, **décide** (fichier ordinaire, pas
un point d'analyse, plus de 0 et moins de 4 Mio ; `licences::decide`, pure) puis
le fait ouvrir par `ShellExecuteW` (`platform::open_file`, qui généralise
`shell_open`). La page ne donne ni chemin ni nom. Réponses : `opened` (la page
ne dit rien), `missing`, `failed` (une phrase).

**La note courante et ses verrous.** `AppsDesk` garde la clé (l'heure) de la
note de la dernière app vue partie. Invariant : cette clé désigne cette note,
ou rien (écriture ratée, pas de fichier, « Oublier »). Les restes ne s'y
ajoutent qu'en phase `removed`, qu'on n'atteint qu'en passant par le drapeau,
donc par l'écriture de la note (`settle_note` s'en assure avant tout ajout) :
aucune remise à zéro n'est nécessaire au lancement d'une autre désinstallation.
Deux notes de même milliseconde ne se confondent pas (les restes vont à la plus
récente). **Ordre des deux verrous : les notes d'abord, puis le suivi**, rendu
aussitôt ; aucun chemin ne prend le suivi puis les notes, et le fichier ne
s'écrit jamais sous le verrou du suivi. `uninstalls.json` ne lit pas son champ
`version`, comme `startup.json` et `game.json` : une version inconnue est lue
comme la 1.

**Limites connues.** « S'est remise toute seule depuis… » veut dire « Shinku
l'a revue active une fois », pas « elle est active maintenant ». Un nom d'app
garde ses caractères de mise en forme (sens d'écriture, par exemple) : la page
les isole par `<bdi>`, elle ne les retire pas. La preuve de
`checkSingleWindow` (le second lancement laisse `journal.json` intact) n'est
essayée que sur le vrai Windows : hors de lui, deux essais ne lisent que la
source (l'ordre des appels dans `app.rs`, la forme de la remise dans le
script), ils ne voient ni un appel fait par un autre chemin, ni une comparaison
qui ne comparerait plus ; et l'essai qui construit le guichet sur un faux
profil ne voit pas une écriture hors de ce profil (registre). Si la fenêtre ne
peut pas s'ouvrir, le grand vide coupé est noté mais son annonce est perdue
(comme avant ce lot). Un désinstalleur qui rend la main avant d'avoir fini peut
faire noter la désinstallation un peu tôt, ou jamais (30 minutes) : le registre
décide. Une désinstallation faite hors de Shinku n'est pas notée. L'ouverture
du fichier des licences dépend de l'association des fichiers `.txt` de la
machine ; elle n'est essayée que par la forme de la page sur la machine
d'intégration (où tout est élevé : l'essai cherche le bouton, il ne le clique
jamais). Le menu a maintenant onze entrées : à 960 × 600 il tient grâce à un
cran de plus (`.nav` à 31 px sous 660 px de haut).

## Désinstallation

Le module liste les apps installées, lance le **désinstalleur de l'éditeur**
(la fenêtre de Windows, jamais une désinstallation silencieuse), puis propose de
mettre à la corbeille ce que l'app a laissé dans le compte. Code :
`crates/shinku-core/src/apps/` ; l'hôte Windows, `platform/windows/apps.rs`.

### Ce que lit le module

Les trois sources de « Programmes et fonctionnalités » : `HKLM\...\Uninstall`
en vue 64 bits (`m6:`), en vue 32 bits (`m3:`), et `HKCU\...\Uninstall` (`ac:`).
Une inscription est montrée sous les conditions de Windows : un nom ;
`SystemComponent` différent de 1 ; pas de `ParentKeyName` ; pas de
`ReleaseType` `Update` ou `Hotfix` ; `WindowsInstaller` à 1 ou une
`UninstallString` non vide. Bornes : 4096 sous-clés par place, 255 unités UTF-16
par nom de clé, 32 Kio par valeur ; une place coupée par une borne ou illisible
rend la liste **incomplète**, et la page le dit. Le module ne fait que **lire** le
registre : aucune méthode de l'hôte ne permet d'y écrire. Les noms, les éditeurs et les
versions sont nettoyés avant d'être montrés ; l'éditeur n'est pas une identité
vérifiée.

La page ne connaît que l'identifiant (`m6:{clé}`) : elle n'envoie jamais un
chemin ni une commande. Au clic, l'inscription est **relue à l'instant** et la
commande en est tirée à ce moment.

### Quelle commande lancer

- Windows Installer (`WindowsInstaller` à 1 et clé en GUID) : `msiexec.exe
  /x {GUID}` du dossier système, sans élévation de la part de Shinku ;
- sinon `UninstallString`, développée : guillemets (le chemin est ce qui est
  entre eux), ou, sans guillemets, le plus court préfixe qui est un fichier (64
  préfixes au plus). Un nom sans dossier n'est cherché que dans le dossier
  système puis dans celui de Windows. Le fichier doit finir par `.exe`, `.com`,
  `.bat` ou `.cmd` : le shell ouvrirait tout autre fichier selon son
  association (un `.msi` lancerait une installation). Chemin réseau, lecteur non
  fixe, existence indéterminée : inutilisable ;
- les paramètres sont passés **tels quels**, Shinku n'en ajoute aucun ;
- l'élévation (`runas`) est demandée pour les inscriptions du PC, jamais pour
  celles du compte. Windows demande lui-même l'accord par sa fenêtre.

Ne sont jamais proposées : Shinku lui-même, WebView2 (la fenêtre s'en sert), et
les apps dont `NoRemove` vaut 1.

### Le registre est la seule vérité

`ShellExecuteExW` ne rend la main qu'après la réponse à la fenêtre de Windows,
qui peut rester ouverte des minutes : la phase `launching` est posée **avant**
de lancer (un second geste est refusé, la page répond). Beaucoup de
désinstalleurs se recopient ailleurs et le premier processus sort tout de suite :
le processus n'est donc **pas** suivi, sauf Windows Installer, dont le processus
dure jusqu'à la fin : sorti deux lectures de suite alors que l'inscription est
encore là, c'est « la personne a annulé » (dit une seule fois). La page relit
toutes les deux secondes ; au bout de 30 minutes, la main est rendue.

Quand l'inscription disparaît (`settling`), on laisse le désinstalleur finir :
Inno Setup et NSIS la retirent **avant** les fichiers. La recherche des restes
part quand 5 secondes sont passées **et** que deux lectures de suite, à 2
secondes d'écart au moins, voient le même contenu direct dans l'emplacement de
l'app ; au plus 60 secondes. Si l'inscription revient, on retourne à l'attente.
Le travail de disque se fait hors du verrou de l'état, avec un numéro de
génération : un résultat qui arrive après « Revenir à la liste » est jeté.

### Les restes, règles une par une

Dans le doute, on ne propose pas : manquer un reste est sans gravité, proposer
le dossier d'une autre app ne l'est pas. Rien n'est coché d'avance.

1. **Le dossier de l'app** : l'emplacement (`InstallLocation`, sinon le dossier
   du programme du désinstalleur, sauf msiexec et les hôtes comme `cmd.exe`, et
   jamais sous Windows) si son dernier nom, plié, est **exactement** le nom nu de
   l'app (nom sans version ni architecture : « Python 3.12.1 (64-bit) » devient
   « python »). Jamais `D:\Jeux`, `Local\Programs`, `D:\Steam`.
2. **Réglages et données** : dans `%APPDATA%`, `%LOCALAPPDATA%` et `LocalLow` (pas
   `Programs`), un sous-dossier direct qui porte exactement le nom nu, ou le
   dernier nom de l'emplacement s'il est aussi un mot entier du nom de l'app
   (`VLC` pour « VLC media player » ; pas `Application`, `bin`, un numéro) ; et,
   sous un dossier qui porte exactement le nom de l'éditeur, un sous-dossier à
   ce nom (`Team Cherry\Hollow Knight`). Le dossier de l'éditeur seul n'est
   jamais proposé ; jamais « contient », jamais « commence par ». La famille
   vaut **pour chaque nom utilisé** : un nom (le nom nu, ou le dernier nom de
   l'emplacement) dont une voisine est de la famille est abandonné. Et B ne
   propose **jamais** l'emplacement lui-même, ni un dossier qui le contient, ni
   un dossier dedans : seul A peut proposer l'emplacement, avec sa preuve par
   le nom.
3. **Raccourcis** du menu Démarrer et du Bureau **du compte** dont la cible
   brute (lue, jamais suivie) est dans l'emplacement et **absente avec
   certitude**.

Un candidat est écarté s'il n'est pas un chemin propre d'un lecteur fixe (ni `.`
ni `..`, ni `~`, ni nom court, ni caractère interdit) ; s'il n'existe plus ; si
un lien est **n'importe où** sur son chemin ; si une voisine le réclame (son
emplacement est égal ou dedans, ou le contient ; ou le texte de son
désinstalleur ou de son icône **contient** le chemin du dossier suivi d'une
barre inverse, où que ce soit dans la ligne : `cmd /c "…\X\u.bat"` et
`rundll32 "…\X\y.dll",Run` réclament `X`) ; s'il contient le dossier de `shinku.exe` ; s'il est dans un
dossier synchronisé (OneDrive…) ; si le garde (`Guard`) le refuse ; ou, pour un
dossier, s'il ne se compte pas en entier ou contient un lien. Tous les restes
montrés sont donc retirables.

**La famille.** Tant qu'une app installée porte le même nom nu, ou commence par
lui suivi d'une espace, **rien** n'est proposé (Python 3.11 à côté de 3.12, une
nouvelle version installée pendant l'attente). Les inscriptions des quatre
places (dont `HKCU\Software\Wow6432Node`) sont relues sans le filtre de la
liste : si l'une est illisible ou incomplète, rien n'est proposé non plus, et la
page le dit (`complete: false`).

### Les voisines se lisent par le texte, jamais par le disque

Pour une voisine, **Shinku ne touche jamais au disque** : ce qu'elle écrit
(emplacement, désinstalleur, icône) est un texte, comparé aux **écritures du
candidat**, lui qui existe et vient d'être vérifié. Chaque composant du chemin
du candidat a une écriture longue et, à côté, une écriture courte
(`GetShortPathNameW`, lecteur fixe vérifié avant l'appel). Une voisine réclame
le candidat si :

- **3a** une ligne (désinstalleur, icône) le mentionne composant par composant.
  Après le dernier composant, la mention tient **sauf si** le caractère suivant
  est une lettre ou un chiffre : fin de texte, barre, guillemet, apostrophe, `;`,
  `)`, `=`, `|`, `:`, virgule, blanc, point, tout réclame (volontairement large :
  `-Target 'D:\…\Hades'` réclame `Hades`, `…\Hades2\x.exe` non). Le texte est
  aussi lu avec `.` et `..` résolus par le texte seul, jamais au-dessus du
  lecteur ;
- **3b** son emplacement est le candidat, dedans, ou le contient (sauf si le
  garde refuse ce préfixe comme racine) ;
- **3c** le candidat n'a pas d'écriture courte connue et un texte de voisine
  porte un nom court (`~` suivi d'un chiffre) : dans le doute, ce candidat seul
  est écarté.

**Une seule mise en forme, des deux côtés** (candidat et voisines), avant toute
comparaison : `/` devient `\`, un préfixe `\\?\` ou `\\.\` d'un emplacement
est retiré, une suite de barres n'en fait qu'une, `.` et `..` sont résolus. Les
points et espaces **finaux** d'un composant sont ignorés (Windows les ignore).
Le pliage est celui de Windows, caractère par caractère (la majuscule simple,
sinon le caractère tel quel : `σ` et `ς` se valent, `ß` reste `ß`). Un `%` dans
un emplacement n'écarte rien (`100% Jus` est un vrai nom).

Une voisine orpheline, une icône absente, un lecteur réseau débranché ne
rendent donc rien illisible. **Les voisines ne sont illisibles que si une place
du registre l'est** (illisible, incomplète, plus de 4096 inscriptions) : la page
le dit (`complete: false`), rien n'est proposé ni retiré.

Limite assumée de sûreté : **une seule sous-clé illisible, ou une seule valeur
de plus de 32 Kio, dans l'une des quatre places suffit à ne rien proposer sur
ce PC** : on ne sait pas ce que cette inscription réclame.

Limites assumées : une voisine inscrite à travers une jonction ou un `subst`
n'est comparée que par son texte ; une app Windows Installer sans emplacement
ni icône ne protège aucun dossier.

### La mise à la corbeille

Un geste qui **écrit** à la fois (`exclusive`) : une mise à la corbeille, un
`rescan` ou le démarrage d'un grand vide en refusent un autre (`busy`). La
première recherche des restes, lancée par la page qui attend, ne le prend pas :
elle ne fait que lire. Le grand vide en cours est lu **une fois l'exclusivité
prise** (`start_clean` ne la rend qu'après son départ), donc la course est
fermée dans les deux sens. Plus de 64 clés demandées : tout est refusé. À chaque demande, tout
est **revérifié à l'instant** : il existe, aucun lien, le garde l'accepte, aucune
voisine ne le réclame, aucune app de la même famille n'est apparue, et un
raccourci mène encore dans l'emplacement et nulle part. Seule la corbeille est
demandée ; une garde lit d'abord les réglages (voir plus bas) et, si Windows
supprime quand même pour de bon, la page le dit (`destroyed`) ; si elle refuse, l'élément reste et
la page le dit (`inUse`, `refused`, `partial`, `destroyed`). Un dossier entre à la corbeille
comme **un seul élément** (`FOF_ALLOWUNDO`, sans suivre les liens), jamais fichier
par fichier ; si Windows répond oui mais que le chemin est encore là, c'est
`failed/partial`, jamais « parti ».

**La corbeille a-t-elle vraiment reçu l'élément ?** `shell::recycle` rend
`Sent::InBin` ou `Sent::Destroyed` : le puits de l'opération note, à
`PostDeleteItem`, si Windows a donné un élément de corbeille (absent : la
suppression est définitive). Le code de réussite ne distingue rien : sur le vrai
Windows (essai du 16 novembre 2026), un envoi réussi et une suppression
définitive rendent le même (`0x00270008`). Sans élément de corbeille, le disque
tranche : chemin absent, `Destroyed` ; encore là, un échec. `Destroyed` est un
**constat après coup**, pas une barrière : le puits ne peut rien arrêter
(`PreDeleteItem` porte le drapeau « corbeille si possible » même quand elle est
trop petite, et Windows supprime alors pour de bon). Le drapeau
`FOF_WANTNUKEWARNING` n'aide pas : même sans interface, il ouvre une fenêtre de
Windows qui attend une réponse (essayé le même jour). Côté restes, après `Destroyed`, le disque tranche :
absent avec certitude, `failed/destroyed` (sans octet compté) ; encore là,
`failed` sans raison.

**La garde, avant le shell** (la décision est une fonction pure, `bin_rule`,
essayée sur toutes les machines). Elle lit, sans rien écrire : `NukeOnDelete`
(global et du volume), la stratégie `NoRecycleFiles` (compte puis machine) et
`MaxCapacity` du volume (en Mio). Le volume est celui **du chemin** (point de
montage par `GetVolumePathNameW`, puis son GUID), pas celui de son lecteur.
Règles : un `NukeOnDelete` ou `NoRecycleFiles` non nul refuse ; capacité connue :
refus au-delà de **90 %** de `MaxCapacity` ; capacité inconnue : refus au-delà de
**5 %** de la taille du volume, et refus si cette taille est inconnue aussi.
La taille est toujours mesurée, après avoir vérifié que le chemin n'est pas un
lien (la garde ne lit jamais la cible d'une jonction).

Les essais sur la vraie corbeille ne tournent qu'en série, derrière
`SHINKU_ESSAI_CORBEILLE`, et remettent toujours les réglages (ils s'arrêtent
avant de rien changer si une valeur ne se lit pas ; une clé créée par l'essai
est retirée si elle est vide). Rien ne vide la corbeille de la machine sans
`SHINKU_ESSAI_VIDE_LA_CORBEILLE`. Ils **prouvent** : la garde refuse (corbeille
désactivée, trop petite), sans la garde la corbeille désactivée est refusée par
Windows, une corbeille de 8 Mio reçoit 1 Mio, et, sans la garde avec une
corbeille trop petite, « disparu sans que la corbeille ait reçu quoi que ce soit
» doit avoir donné `Destroyed` et « un élément de plus » `InBin`. Leurs lignes
`FAIT :` disent ce que Windows a répondu (drapeaux, code, élément de corbeille).

### Limites connues

- Un désinstalleur encore au travail 60 secondes après avoir retiré son
  inscription : les restes sont cherchés quand même, et la page peut montrer un
  dossier à moitié vidé (« Chercher à nouveau » corrige).
- Les restes de `Program Files`, de `ProgramData`, du menu Démarrer commun et du
  Bureau public ne sont ni montrés ni retirés.
- Aucun reste n'est cherché par ressemblance de nom : on manque `%APPDATA%\Code`
  pour Visual Studio Code, `Documents\My Games`, etc.
- Rien n'est proposé tant qu'une app de la même famille est installée.
- Une app nommée comme le dossier de réglages d'une autre (« Code 1.2 » et le
  `%APPDATA%\Code` de Visual Studio Code) ferait proposer ce dossier : c'est le
  prix du nom exact. D'où : jamais coché, une phrase d'avertissement, et la
  corbeille.
- Un registre qui change pendant la lecture d'une place (une sous-clé retirée
  entre deux énumérations, ou un nom de sous-clé qui diffère entre deux lectures)
  rend cette place **incomplète** : rien n'est proposé.
- Pendant l'attente (`settling`), le contenu du dossier de l'app (4096 noms au
  plus) est lu sous le verrou du suivi : un seul dossier, borné, donc court.
- Les indices `runtime` et `driver` sont des **noms**, des ressemblances, jamais
  des certitudes.
- L'élévation des désinstalleurs du PC suit ce que des tiers ont observé de
  Windows, pas une documentation de Microsoft. Recette de vérification, à la
  main : une inscription d'essai dont la ligne est `cmd.exe /k whoami /groups`,
  sous `HKLM` puis sous `HKCU`, lancée depuis la page.
- Une corbeille pleine abandonne ses éléments les plus anciens (réglage de
  Windows).
- La garde ne voit que des réglages : un cas qu'elle ne connaît pas se voit
  seulement après coup (`destroyed`).
- Quand Windows répond « supprimé pour de bon » (`Sent::Destroyed`), l'étape des
  doublons s'arrête (`NotInRecycleBin`) sans jamais compter la copie comme
  envoyée à la corbeille ; le bilan sous-estime alors l'espace libéré (0 pour
  cette copie), par prudence.
- Une inscription du compte peut désigner n'importe quel programme (comme dans
  Windows) : Shinku ne lance qu'après un clic et une confirmation, sans droits de
  plus que la personne.
- Les apps du Store (MSIX) ne sont pas listées.
- Ce que les essais ne prouvent pas : l'essai sur la corbeille réelle
  (`the_real_recycle_bin_receives_a_whole_folder_or_nothing`, les essais de
  refus et de capacité décrits plus haut, tous derrière `SHINKU_ESSAI_CORBEILLE`)
  ne tourne que sur la machine jetable de la CI. Les valeurs que Windows 11 lit
  vraiment pour un volume (`NukeOnDelete`, `MaxCapacity`) se déduisent des
  essais, pas d'une documentation. `ShellExecuteExW` et sa fenêtre d'élévation, et les
  scripts PowerShell ne tournent que sur un vrai Windows (intégration continue,
  `scripts/essai-desinstallation.ps1`).

## Confidentialité

Le module efface des **traces de l'activité de la personne sur ce PC, dans son
compte**, par catégories. Rien n'est coché d'avance, rien n'est élevé, **aucun
journal n'est écrit**. Shinku ne montre jamais le **contenu** d'une trace (ni
adresse, ni nom de fichier) : il dit qu'elle existe, son poids ou son nombre
d'éléments. La conception complète est dans `specs/confidentialite-spec.md`
(révision 2) ; ce qui suit est ce que le code fait.

### Les règles (liste fermée, `privacy/rules.rs`)

- **Historique d'un profil** de Chrome, Edge, Brave, Vivaldi (version stable). Le
  dossier est `%LOCALAPPDATA%` plus des composants compilés (`Google`, `Chrome`,
  `User Data`…). Un profil est un dossier dont le nom est **exactement**
  `Default`, ou `Profile ` suivi de un à six chiffres décimaux sans zéro en tête
  (`profile 1`, `Profile 01`, `Profile 0`, `Guest Profile` n'en sont pas). Les
  fichiers : `History`, `Archived History`, `Visited Links`, `Favicons`,
  `Shortcuts`, `Network Action Predictor`, et `Top Sites` sauf pour Edge, chacun
  avec `-journal`, `-wal`, `-shm`, rien d'autre. **Jamais** : `Local State`,
  `Login Data`, `Web Data`, `Bookmarks`, `Preferences`, `Secure Preferences`, `Sessions`, `Current Session`, `Sync Data`, `Cookies`,
  `Network`, `Local Storage`, `IndexedDB`, `Extensions`, `Local Extension
  Settings` (la liste de la spécification, plus trois noms de la critique ; un essai la compare, écrite en dur). Un essai parcourt toutes les règles et échoue si l'un de ces noms y
  apparaît ; l'effacement refuse aussi, par `is_forbidden`, tout lot qui en
  contiendrait un.
- **Fichiers récents** : `SHAddToRecentDocs(SHARD_PIDL, NULL)` sous COM, puis le
  nombre de raccourcis de `Recent` est **relu** (l'appel ne rend rien). Tant qu'il en reste, la relecture attend et recommence (cinq lectures au plus, 100 ms entre deux) : le premier zéro donne `erased` ; sinon, moins qu'avant, `partial` ; autant ou plus, `failed`.
- **Ce qui a été tapé** : les **valeurs** de `RunMRU`, `TypedPaths`,
  `WordWheelQuery` sous `HKCU`. Les clés sont une énumération compilée : l'hôte
  ne reçoit jamais une chaîne de chemin.
- **Presse-papiers** : `Clipboard.ClearHistory()` puis le contenu actuel
  (`OpenClipboard`/`EmptyClipboard`, dix essais à 50 ms). `ClearHistory` faux
  alors que l'historique est actif : `failed` ; historique désactivé : cas normal.
  L'issue est `erased` quand l'historique est vidé ou désactivé et le contenu vidé.

### Fichiers : tout ou rien

Quatre étapes, avec `fs::Removal` : (1) la chaîne de dossiers du profil est
revérifiée **à l'effacement** (aucun lien) ; (2) **tout le lot** est ouvert sans
aucun partage et sans suivre de lien, chaque fichier doit être ordinaire, **identifié
par la liste** (un fichier sans identifiant rend le lot irrégulier : rien ne part),
**n'avoir qu'un seul nom** et ne pas être « en ligne uniquement » ; une seule ouverture
qui échoue (`inUse`, `failed`, `changed`) et rien n'est touché, annexes comprises ;
(3) poignées tenues, dans la section critique (voir « Exclusivité »), le navigateur est
revérifié fermé **et le dossier du profil est relisté** : les fichiers de la règle
présents doivent être exactement ceux qui sont ouverts, sinon `changed` et rien n'est
marqué (une annexe apparue depuis l'inventaire) ; (4) tout est marqué, **chaque base
avant ses propres annexes** (`marking_order`), puis tout est confirmé. La garantie réelle : **une base ne reste jamais sans ses
annexes, et rien ne part si une seule ouverture échoue** (un arrêt brutal pendant le marquage
peut en revanche supprimer la base et laisser ses annexes, voir « Exclusivité ») ; `partial` n'existe pas pour les fichiers (il vaut pour le
registre et pour `recent`). Pour lire le nombre de noms et « en ligne uniquement » sur
l'entrée ouverte, `fs::Removal::facts` a été ajouté ; ses autres usages (le grand vide)
sont inchangés.

**Pourquoi base d'abord.** Si le marquage était coupé net (arrêt brutal), Windows
supprimerait ce qui est déjà marqué. Une annexe (`-journal`, `-wal`) orpheline à côté d'une
base absente est écartée par SQLite quand le navigateur recrée la base **[non prouvé : sur
le vrai Windows, le passage normal ne laisse aucune annexe (preuves 1 et 8 du test) et
Chrome relancé recrée `History` ; la coupure elle-même n'a pas été fabriquée]** ; une base qui reste sans son journal chaud
est ce qui l'abîme. Avec cet ordre, une coupure ne laisse que des annexes sans base.

**Navigateur ouvert** : un processus du nom de l'exécutable (toutes les sessions)
**ou** le `lockfile` de `User Data` tenu. Une machine qui ne sait pas lister ses
processus est tenue pour « tout est ouvert ».

### Registre

Clé ouverte dans `HKCU` avec `REG_OPTION_OPEN_LINK` et les seuls droits de lire et
d'écrire des valeurs ; refusée si elle porte `SymbolicLinkValue` de type
`REG_LINK`. Les noms sont énumérés (4096 valeurs, 255 unités au plus), puis
supprimés **par nom**, la valeur d'index (`MRUList`, `MRUListEx`) d'abord ; la clé
reste ; puis **relecture** : des valeurs restées ou revenues (l'Explorateur qui
réécrit ce qu'il a en mémoire) donnent `partial`, jamais `erased`. Un nom de valeur qui contient un zéro n'est **jamais** passé à la suppression (Windows le couperait et viserait une autre valeur) : la valeur reste, et c'est la relecture qui la voit : `failed` si elle est seule dans sa clé (rien n'est parti), `partial` si d'autres valeurs sont parties. Le compte affiché (`typed`) ne compte ni les valeurs d'index (`MRUList`, `MRUListEx`) ni le nom vide.

### Identifiants, nom des profils, résultats

Un identifiant porte le navigateur, le numéro du profil, **l'identité du dossier**
(volume et identifiant de fichier) et une empreinte du nom affiché ; le décodage
est strict. À l'effacement la liste est refaite : autre identité ou autre nom
affiché, `changed`. Un identifiant que le cœur n'aurait pas fabriqué (vue périmée) donne aussi `changed`, sans rien toucher ; les autres traces de la demande sont traitées. Plus de 64 identifiants : tout est refusé (`tooMany`).
Le nom affiché est lu dans `Local State` (`profile.info_cache.<dossier>.name`
seulement, 4 Mio au plus, jamais écrit, jamais l'adresse du compte), nettoyé par
`startup::clean_text`. `PrivacyGroup.slot` (0 : `Default`, N : `Profile N`) permet à
la page d'écrire « Profil principal » ou « Profil N ».

### Exclusivité

Le verrou est celui d'`AppsDesk::try_exclusive`, pris dans `Backend` ; l'état du
grand vide est lu une fois le verrou pris. Un effacement refuse (`busy`) pendant un
grand vide, une mise à la corbeille de restes ou un autre effacement, **et
l'inverse**.

**Fenêtre fermée pendant un effacement.** `Backend::shutdown` lève d'abord un drapeau
« on ferme » : plus aucune trace ne démarre, le guichet le lit avant chacune. Puis il
attend, sur un verrou, la fin de la section critique d'un lot de fichiers
(de la revérification du navigateur au dernier effacement : quelques appels
système sur des fichiers déjà ouverts, donc très court). Les fichiers d'un profil sont
donc tenus **par construction** : la fermeture ne peut pas tomber entre le premier
marquage et le dernier effacement. Sur un échec de marquage aussi : les
demandes déjà posées sont retirées dans la section, avant qu'elle ne rende son verrou. Ils le sont aussi **par l'ordre** : chaque base est
marquée avant ses annexes, de sorte que même un arrêt qu'aucun verrou n'arrête (le
Gestionnaire des tâches, un plantage) ne laisse que des annexes sans base. Mais l'ensemble
**n'est pas atomique** : le registre, les récents et le presse-papiers sont faits trace par
trace, et un effacement coupé en route peut laisser l'une d'elles à moitié vidée, sans
dommage (l'index part d'abord) et sans bilan affiché.

### Limites connues

- Presse-papiers : la ligne est `present` si le contenu actuel est plein **ou**
  si l'historique de Windows (Windows+V) est activé (Shinku ne le lit pas, il ne
  sait donc pas s'il est vide) ; `empty` seulement si le contenu est vide et
  l'historique désactivé ; `unreadable` si l'un des deux ne se lit pas.
- Un `User Data` derrière un lien ou une jonction (déplacement du profil sur
  un autre disque) n'est pas proposé : Shinku n'entre dans aucun lien.
- Un lot marqué puis une coupure de courant : aucun bilan (aucun journal).
- `REG_OPTION_OPEN_LINK` ne protège que le **dernier** composant du chemin d'une clé : un lien
  posé plus haut (`…\Explorer`) serait suivi. Il faut déjà un programme du même
  compte pour le poser.
- Les noms de fichiers d'un lot sont comparés **exactement** : un `HISTORY` en majuscules
  n'est pas vu, donc pas effacé : si aucun autre fichier de la règle n'est là, la ligne dit
  « Rien à effacer » ; sinon elle est proposée, l'effacement dit « Effacé » et `HISTORY` reste.
  Sans danger pour le reste du profil.
- `lockfile` est ouvert avec l'accès en écriture, **jamais écrit ni créé** : c'est la
  demande d'écriture qui entre en conflit de partage avec un navigateur ouvert
  (vu sur le vrai Windows, preuves 2 et 8 : l'ouverture est refusée pour partage face
  à un `lockfile` tenu comme le fait Chromium, et face au vrai Chrome ouvert).
- Si le retrait d'une demande de suppression échoue après l'échec du marquage d'un autre fichier,
  l'erreur est avalée (`let _ =` dans `fs/windows.rs`, code partagé avec le grand vide, non
  touché ici) : le fichier déjà marqué part, et le lot est à moitié effacé. Très improbable
  (le même appel vient de réussir sur ce fichier).
- Le **volume** n'est pas comparé à l'ouverture : seule l'identité du fichier l'est
  (identifiant, taille, date, un seul nom), et l'ouverture se fait par le chemin, non par rapport à
  la poignée du dossier. Si le dossier du profil était remplacé par une jonction entre la liste
  et l'ouverture, un fichier d'un autre volume ne passerait que s'il avait le même identifiant
  de fichier, la même taille et la même date à 100 ns ; ce n'est pas impossible en théorie, ce
  n'est pas un cas réaliste. La chaîne de dossiers est de toute façon revérifiée juste avant.
- Pas de nom en double distingué (« Perso (2) ») : deux profils de même nom
  portent des numéros différents.
- Ne sont pas faits : cookies et données de sites, Firefox, Opera, versions
  Beta/Dev/Canary, `dialogs`, `Sessions`, Recall, sauvegardes et points de
  restauration. Les copies de l'historique que Windows (VSS) ou un outil de
  récupération gardent, la synchronisation du navigateur, et la réimportation
  d'Edge depuis Chrome ne sont pas touchées.
- **Vu sur le vrai Windows** (machine de GitHub, Windows 11 build 26100, passage du
  16 novembre 2026, les huit preuves de `essai-confidentialite.ps1`, 13 scénarios) :
  `lockfile` tenu en écriture refuse notre ouverture, avec le partage de Chromium comme
  sans aucun partage, et avec le vrai Chrome ouvert ; un fichier tenu ouvert, un lien
  physique NTFS (deux noms), une jonction à la place du profil et un lien de registre
  ne laissent rien partir, ni d'un côté ni de l'autre ; un profil supprimé puis recréé
  est reconnu comme un autre (identifiant de fichier différent) ; `SHAddToRecentDocs(NULL)`
  marche depuis la commande `async` (fil de travail sans boucle de messages), le récent
  posé a disparu dès la première lecture et n'est pas revenu deux secondes plus tard,
  et **le dossier épinglé à l'Accès rapide est resté** : `recent` reste une ligne et ne
  devient pas un bouton ; le presse-papiers est vide après l'effacement lancé depuis la
  page ; une fenêtre de l'Explorateur fermée après l'effacement ne réécrit rien dans
  `RunMRU`, `TypedPaths` ni `WordWheelQuery` ; le vrai Chrome, effacé puis relancé,
  démarre et recrée `History`, et `Login Data`, `Network\Cookies`, `Web Data` et
  `Local State` gardent la même empreinte SHA-256.
- **Reste à vérifier** : ce que fait `SHAddToRecentDocs(NULL)` de « Recommandé » ;
  `ClearHistory` quand l'historique du presse-papiers est activé (il était coupé sur la
  machine d'essai : `IsHistoryEnabled()` rend `False`, et `ClearHistory()` appelé par le
  script rend `True`, ce qui ne dit rien de l'appel fait par Shinku avec un historique
  plein) ; l'annexe SQLite orpheline après une coupure (voir « Pourquoi base d'abord ») ;
  un vrai profil de tous les jours, avec la synchronisation active : c'est l'essai que
  seule la personne peut faire sur son PC.
- Sept pages des réglages de Windows sont ouvertes par Shinku (liste fermée,
  `WindowsPage`), dont `ms-settings:privacy-activityhistory`,
  `ms-settings:search-permissions` et `ms-settings:privacy-feedback`.

## L'agent

`shinku-agent.exe` (`crates/shinku-agent`) n'a ni console ni fenêtre visible. Sa
boucle (`game::agent::Agent`) ne connaît pas Windows : elle reçoit des
événements (`Tick`, `PowerChanged`, `Release`, `Quit`, `Remove`, `SessionEnding`,
`Disconnected`, `Reconnected`) et rend « continuer » ou « s'arrêter ». Le reste
(`platform/windows/agent.rs`) est la fenêtre cachée qui les produit.

**Qui le lance.** Shinku, au démarrage et à chaque changement des réglages du
mode jeu (`Backend::sync_agent`), hors du fil de la fenêtre : si le mode
automatique est allumé, ou si le journal contient une partie, et que l'agent ne
tourne pas. Windows le lance aussi, à l'ouverture de session, par la valeur
`Run` (`shinku-agent.exe --demarrage` : reprise immédiate, détection après deux
minutes). Cette valeur n'existe que si le mode automatique est allumé : Shinku
n'apparaît dans les apps de démarrage de Windows qu'à cette condition, et c'est
elle qui garantit la reprise après une coupure de courant. Si la personne l'a
désactivée dans le Gestionnaire des tâches, la page le dit et Shinku n'y touche
pas. Fermer la fenêtre ne ferme pas l'agent.

**Quand il sort.** Quand le mode automatique est éteint et qu'il n'y a plus de
session ; quand il reçoit « quitter » ou « retirer » ; quand Windows ferme la
session (il remet tout en place avant de rendre la main). Une déconnexion de la
session (console ou bureau à distance) remet aussi tout en place, et la
détection reprend à la reconnexion.

**Ses messages.** Une fenêtre de premier niveau, jamais affichée (pas une
fenêtre « message seulement » : elle ne recevrait pas la fin de session), de
classe `Shinku.Agent`. Elle reçoit une minuterie de 2 s, les changements
d'alimentation, la fin et le changement de session, et trois messages de
Windows enregistrés (`Shinku.Agent.Relacher`, `.Quitter`, `.Retirer`) que
`shinku.exe` et l'installeur lui postent. Un processus élevé peut poster à une
fenêtre qui ne l'est pas : c'est ainsi que l'installeur lui parle. Une
instance par session, garantie par un mutex nommé.

**Ses arguments.** Aucun (veille tout de suite), `--demarrage`, `--relacher`
(reprend la session du journal, la relâche, puis veille), `--quitter` et
`--retirer` (postent l'ordre à l'agent en cours et attendent sa fin, dix
secondes au plus ; permis même élevé, c'est l'installeur qui les appelle),
`--version`. Tout autre argument : code 2. Lancé en administrateur, il sort sans
rien dire, code 3 : un agent élevé n'aurait pas les droits des jeux de la
personne.

**Son installation.** `shinku-agent.exe` n'est pas le programme principal : Tauri
ne l'embarque pas tout seul. `tauri.conf.json` le compile avant l'installeur
(`beforeBuildCommand`), et les crochets de l'installeur le copient à côté de
`shinku.exe` et font sortir l'ancien agent avant toute copie ou suppression.

Les crochets sont répartis en deux fichiers, parce que Tauri n'accepte qu'un
fichier de crochets et NSIS une seule définition par macro.
`src-tauri/installeur/habillage.nsh` (`bundle.windows.nsis.installerHooks`,
voir « L'installeur ») définit les quatre macros `NSIS_HOOK_*` : chacune appelle
d'abord celle de l'agent, puis fait ce qui est propre à l'habillage. Tout ce qui
concerne l'agent est dans `src-tauri/installeur/agent.nsh`, qui ne dépend que de
`LogicLib.nsh` et
définit `ShinkuAgentPreInstall`, `ShinkuAgentPostInstall`,
`ShinkuAgentPreUninstall` et `ShinkuAgentPostUninstall` (les macros internes
portent aussi le préfixe `ShinkuAgent`). Ces fichiers restent en ASCII : un
accent y serait mal lu selon la page de code.

Ce que font les crochets :

- **La sortie de l'agent** (`ShinkuAgentLeave`) lance l'agent installé avec
  l'ordre voulu et lit son code de sortie. Code 0 : il a tout remis en place et
  il est parti. Autre code (il n'est pas sorti en dix secondes, ou n'a pas pu
  démarrer) : `taskkill /F /IM shinku-agent.exe`, puis le même ordre une
  seconde fois ; plus aucun agent ne répond, c'est alors cette commande qui
  reprend le journal, remet l'alimentation et les apps en place, et pour
  `--retirer` efface la valeur `Run`. `taskkill` n'est donc un dernier recours
  que pour l'agent du compte. Même après une sortie réussie, un `tasklist`
  cherche un `shinku-agent.exe` encore en marche (ceux des autres sessions, voir
  « Limites connues ») et l'arrête de force.
- **À l'installation**, la sortie a lieu deux fois : avant toute copie, et juste
  avant de déposer le nouveau fichier. Tauri ne ferme `shinku.exe` qu'entre les
  deux, et tant que la fenêtre vit, elle relance l'agent qu'on vient de faire
  sortir (page Mode jeu ouverte, mode allumé) : l'ancien agent verrouillerait
  alors le fichier. Une fois la fenêtre fermée, rien ne le relance plus.
- **À la désinstallation**, `--retirer` si c'est une vraie désinstallation,
  `--quitter` si la variable `$UpdateMode` de Tauri vaut 1 (désinstalleur lancé
  avec `/UPDATE`, ce que Tauri ne fait pas aujourd'hui : la branche est sans
  danger et juste si cela change). Le fichier de
  l'agent est supprimé tout de suite : Tauri retire le dossier d'installation
  avant le crochet suivant, et un dossier qui contient encore l'agent ne se
  retire pas. Une fois `shinku.exe` fermé par Tauri (il a pu relancer l'agent
  entre les deux crochets), le crochet suivant fait sortir une dernière fois
  l'agent s'il reste un fichier. Sans `/REBOOTOK` : une suppression remise au redémarrage effacerait
  l'agent d'une réinstallation faite entre-temps.

Le chemin du fichier compilé est bâti à partir de `${__FILEDIR__}`, par un
`!define` hors de toute macro : NSIS résout sinon les chemins relatifs par
rapport au script généré, et relirait `${__FILEDIR__}` à l'endroit où la macro
est insérée, pas où le fichier est écrit. Comme `${__FILEDIR__}` vaut le dossier
du fichier où il est écrit, c'est depuis `installeur/` qu'il remonte de quatre
dossiers jusqu'à `target/release`.

## L'installeur

NSIS, installation par machine dans Program Files, en français, avec l'amorce
de WebView2 embarquée : c'est le choix du document d'architecture. Program
Files protège les exécutables d'une modification par un programme qui n'a que
les droits de l'utilisateur.

WebView2 fait partie de Windows 11 : l'amorce ne sert que s'il a été retiré du
PC, et c'est le seul cas où l'installeur télécharge quelque chose (chez
Microsoft).

L'installeur contient `shinku.exe` et l'agent du mode jeu, `shinku-agent.exe`
(voir « L'agent »). Le helper y entrera avec ses premières fonctions.

### L'habillage

L'installeur reste l'assistant NSIS standard de Tauri, habillé : fond Brume
(`F4F1EE`) sur les pages Bienvenue et Terminé et sur le bandeau d'en-tête, le
trou noir de l'Accueil et le logotype à gauche, un petit signe au bord droit de
l'en-tête, une barre de progression plate couleur pêche, Segoe UI, tous les
textes en français et au tutoiement. Les pages intérieures gardent le gris
natif de Windows, volontairement. La liste des détails est masquée : la page
d'installation passe seule à la page Terminé, où la case « Lancer Shinku » est
cochée et le raccourci du bureau ne l'est pas.

Le modèle NSIS de Tauri n'est pas bifurqué (`template` reste absent de la
configuration) : toute la logique d'installation, de mise à jour et de
désinstallation est celle de Tauri, et suit ses versions. L'habillage passe
par trois réglages de `bundle.windows.nsis`, qui pointent sur
`apps/shinku/src-tauri/installeur/` :

1. `habillage.nsh` (`installerHooks`) : la mécanique. Le modèle l'inclut avant
   les pages. Il fixe les réglages de Modern UI (couleurs, images, hooks
   d'initialisation), inclut `textes.nsh`, redéclare cinq macros de page (voir
   plus bas) et porte les quatre hooks de Tauri, qui appellent aussi ceux de
   l'agent du mode jeu (`agent.nsh`, voir « Son installation » plus haut). Il
   est en ASCII strict : les textes
   n'y sont pas, ils ne dépendent donc pas de l'option `-INPUTCHARSET` que Tauri
   passe à makensis ;
2. `textes.nsh` : tous les textes de Modern UI (en UTF-8, inclus par le
   premier avec `/CHARSET=UTF8`). Le fichier de langue de Modern UI ne définit
   une chaîne que si elle ne l'est pas déjà : une pré-définition l'emporte. Cela
   vaut parce que l'installeur n'a qu'une langue ;
3. `francais.nsh` (`customLanguageFiles`) : les 27 chaînes de Tauri, les textes
   fixés par commande (boutons, textes des pages Dossier et Désinstaller,
   message d'erreur d'écriture), la police, et les fonctions d'habillage. Tauri
   l'inclut après les pages : les fonctions peuvent donc utiliser les variables
   `$mui.*` des pages, qui n'existent pas avant. Tauri en fait une copie
   ailleurs : le fichier retrouve les images par `SHINKU_INSTALLEUR`, jamais par
   un chemin relatif.

Pour accrocher une fonction à l'affichage d'une page (placer les contrôles de
Bienvenue et Terminé, colorer la barre, poser « Installer » sur le bouton de
la page Dossier), il faut définir
`MUI_PAGE_CUSTOMFUNCTION_SHOW` juste avant la page. Le modèle de Tauri insère
lui-même les pages : `habillage.nsh` redéclare donc `MUI_PAGE_WELCOME`,
`MUI_PAGE_DIRECTORY`, `MUI_PAGE_INSTFILES`, `MUI_PAGE_FINISH` et
`MUI_UNPAGE_INSTFILES` par une copie
exacte du corps de Modern UI, précédée de cette seule définition. Le risque : si
une version de NSIS change le corps d'une de ces macros, la copie garde
l'ancienne. Une macro disparue ou une définition en double casse la
compilation, bruyamment ; un contrôle ajouté à une page ne se verrait que sur
les captures de l'essai. À chaque changement de version de NSIS, comparer les
cinq copies avec `Pages/*.nsh`.

La page Dossier est redéclarée pour une raison de plus : makensis ne met le
texte « Installer » que sur la page qui précède immédiatement la page
d'installation, et dans le modèle de Tauri c'est la page Menu Démarrer, toujours
sautée. Sans `ShinkuDirectoryShow`, le bouton de la page Dossier dirait
« Suivant ».

**Les images** (`installeur/images/`, versionnées) sont fabriquées par
`apps/shinku/scripts/images-installeur.mjs`, qui reprend le dessin de
l'interface (`src/views/art.ts` : le script doit le suivre). Chacune existe en
cinq tailles, de 100 à 200 % : le bandeau de gauche (191 × 362 pixels à 100 %)
et le signe de l'en-tête (76 × 58). Elles sont affichées à leur taille réelle,
sans étirement : leur fond est celui de la page, aucune couture ne se voit (un
test vérifie la couleur exacte de leurs coins). À l'ouverture, l'installeur lit
la résolution de la fenêtre (`GetDpiForWindow`) et prend la taille la plus
proche ; la configuration pointe sur les images à 100 %, qui servent de repli.
Le signe est posé à la main (bord droit de la fenêtre, haut à 0) : appeler la
fonction d'alignement de Modern UI le décalerait hors de la fenêtre à 150 %.

**Les réglages.** Shinku range ses réglages dans `%APPDATA%\Shinku` et les
journaux du mode jeu et du Démarrage dans `%LOCALAPPDATA%\Shinku`, pas sous
l'identifiant du paquet : la case « Supprimer aussi mes réglages » de Tauri ne
les supprimait donc pas. **Le désinstalleur n'efface plus aucun fichier ni
dossier dans un profil** : quand la case est cochée, il demande à
`shinku.exe --oublier`, lancé avec les droits de la personne devant le bureau,
de le faire. Tout est dans la section suivante.

Le mode silencieux (`/S`) n'a pas de fenêtre : les fonctions d'interface n'y
tournent pas, et aucun hook de section ne suppose une fenêtre. En mode passif
(`/P`), la page d'installation s'affiche et nos fonctions d'interface tournent,
sans danger ; les pages Bienvenue et Terminé sont sautées. La mise à jour
(`/UPDATE`) ne touche pas aux réglages.

### Effacer les réglages à la désinstallation : `shinku.exe --oublier`

**Les défauts d'avant.** Avec la case cochée, le désinstalleur, élevé, effaçait
dans le profil. Trois défauts :

1. Le modèle NSIS de Tauri fait `RmDir /r "$APPDATA\fr.shinku.app"` et
   `RmDir /r "$LOCALAPPDATA\fr.shinku.app"` avec les droits d'administrateur
   (le bloc `${If} $DeleteAppDataCheckboxState = 1` / `${AndIf} $UpdateMode <> 1`,
   placé entre les deux crochets de désinstallation). Un programme du compte peut
   remplacer ce dossier par une jonction : c'est un effacement récursif, élevé,
   d'un dossier de son choix. C'est le défaut le plus grave.
2. L'ancienne macro `ShinkuDeleteSettings` effaçait des fichiers nommés, sans
   `/r`, après un contrôle du point d'analyse : une fenêtre restait entre le
   contrôle et l'effacement.
3. Si l'élévation se fait avec un autre compte (compte ordinaire et mot de
   passe d'un administrateur, ou protection administrateur de Windows 11, qui
   élève dans un compte fantôme : à vérifier), ce sont les réglages de cet autre
   compte qui étaient visés.

**Ce que fait `--oublier`.** Le code est `crates/shinku-core/src/forget/` (la
décision, pure, avec un faux hôte), `fs/tree.rs` (les suppressions) et
`platform/forget.rs` (la vraie machine, sans code `unsafe` : l'élévation, les
dossiers connus et les suppressions sont ceux du reste du cœur).

- La ligne de commande est **exactement ce seul mot**. Toute autre forme
  (`--oublier x`, `--OUBLIER`, `--oublier=1`) démarre Shinku comme d'habitude.
- Il est reconnu **en tout premier**, dans `main`, avant `app::run` qui contient
  le reste : le contrôle d'élévation et sa boîte, le rapport de plantage, la
  lecture de la version de WebView2, le constructeur de Tauri et le module
  d'instance unique. La fonction ne reçoit rien de Tauri. Un essai de `main.rs`
  lit sa propre source et échoue si une ligne de code précède l'appel (il garde
  l'ordre, il ne l'exécute pas ; la preuve en vrai est sous Wine, plus bas).
- **Aucune fenêtre, aucune boîte, aucun WebView, rien d'écrit sur le disque**,
  quel que soit le résultat.
- **Il refuse s'il est élevé par rapport au bureau, ou s'il ne sait pas** : code
  2, rien n'est touché, pas même un dossier regardé. Le cas connu est
  `elevated_launch`, la décision de la fenêtre, d'après le code : droits du
  processus ordinaires, il passe ; droits du processus élevés (moitié
  « administrateur » d'une paire de jetons), il refuse ; tous les droits sans
  paire de jetons, tout dépend du bureau : bureau ordinaire ou d'un autre compte
  (illisible par refus d'accès), il refuse ; bureau avec tous les droits ou
  élevé, ou **pas de bureau** (ou bureau disparu pendant la lecture), il passe
  (le cas « contrôle de compte coupé »). Le cas inconnu est propre à
  `--oublier` : si Windows ne dit pas les droits **du processus** (le jeton ne se
  lit pas), il refuse. Un bureau dont le jeton ne se lit pas compte comme un bureau
  ordinaire, donc refuse quand le processus a tous les droits sans paire. La
  fenêtre garde le choix contraire (ne bloquer personne, `runs_elevated`) ; un
  effaceur ne peut pas se permettre de viser, sans le savoir, les réglages d'un
  autre compte. La lecture garde donc « inconnu » (`own_rights_known`,
  `Option<Rights>`) jusqu'à la décision `elevated_or_unknown`.
- Dans l'ordre : (1) les fichiers de la **liste fermée**, chacun par les règles
  de `fs` ; (2) les deux dossiers `Shinku`, **seulement s'ils sont vides** ;
  (3) les deux arborescences `%APPDATA%\fr.shinku.app` et
  `%LOCALAPPDATA%\fr.shinku.app`, par une suppression récursive.
- **La liste fermée** (`forget/list.rs`) est faite des constantes de nom des
  modules qui écrivent : `settings.json`, `settings.illisible.json` et
  `journal.json` dans `%APPDATA%\Shinku` ; `game.json`, `startup.json` et
  `uninstalls.json` dans `%LOCALAPPDATA%\Shinku` ; et le voisin `.tmp` de chaque fichier écrit par
  `write_replacing` (le nom du suffixe vient de la même fonction que l'écriture).
  Le fichier « illisible » est mis de côté par un `rename` : il n'a pas de
  `.tmp`. C'est ce que l'ancienne macro effaçait, plus `uninstalls.json` et son `.tmp` (onze noms),
  et rien de plus : un fichier qui n'est pas dans la liste reste, et son dossier avec lui.
- **Chaque fichier** passe par `fs::Removal` : fichier ordinaire (ni lien, ni
  dossier, ni point d'analyse inconnu), un seul nom, pas « en ligne
  uniquement », avec un identifiant de fichier, aucun point d'analyse dans la
  chaîne des dossiers (`inspect_dir_chain`), contrôle de la nature, de la taille,
  de la date et de l'identifiant sur le handle ouvert, puis suppression par ce
  handle. Pas de Corbeille : ce sont des fichiers de Shinku, sans valeur pour
  personne.
- **La suppression récursive** (`fs::remove_tree`, nouvelle) :
  - la chaîne des dossiers **au-dessus** de la racine ne traverse aucun lien (un
    profil derrière une jonction est refusé en entier : code 1, les fichiers de
    réglages et le cache restent ensemble, au lieu de perdre l'un et de garder
    l'autre) ;
  - la racine doit être un vrai dossier (une jonction ou un fichier à sa place
    est laissé, et le résultat est « il reste quelque chose ») ;
  - un point d'analyse rencontré plus bas n'est **jamais traversé** et reste en
    place, avec les dossiers qui y mènent ;
  - **sans identifiant de fichier** (FAT, exFAT), un dossier n'est pas traversé
    et un fichier n'est pas supprimé : « sans identité, pas effaçable », comme
    pour la Confidentialité. Il reste, code 1 ;
  - un nom d'entrée vide, `.`, `..` ou qui contient un séparateur n'est jamais
    joint à un chemin : l'entrée est comptée comme restante ;
  - un fichier à plusieurs noms est supprimé comme un nom (un cache n'est pas une
    trace : l'autre nom n'est pas touché) ;
  - un dossier qui n'est plus vide au moment de partir (une app y a écrit) reste
    et n'est jamais compté « parti » : il est occupé, donc réessayé, puis code 1 ;
  - bornes : 40 niveaux de profondeur, et **50 000 entrées** en tout. Un vrai cache
    de WebView2 compte quelques milliers de fichiers : 50 000 laissent dix fois
    la marge. La borne compte aussi pour le temps : chaque entrée coûte une
    ouverture, quelques lectures, une disposition et une fermeture, et le
    désinstalleur n'attend que 30 s ;
  - chaque dossier est lu par un handle ouvert sans suivre de lien et revérifié
    par son identifiant ; chaque entrée est supprimée par le handle qui l'a
    contrôlée.
- **Le nom `fr.shinku.app`** n'a qu'une source, `tauri.conf.json`
  (`identifier`) : `forget::TAURI_IDENTIFIER` le répète, et un essai lit le
  fichier et échoue si les deux diffèrent.
- **Le réessai et l'échéance.** Un fichier occupé (les processus de WebView2
  finissent de sortir : le désinstalleur ne les ferme pas, ils sortent d'eux-mêmes
  après la fenêtre) est réessayé : toute la suite des suppressions est reprise, de
  pause de 250 ms en pause, pendant **dix secondes au plus en tout**, une seule
  horloge pour l'ensemble et non par fichier ; sur le vrai Windows, fenêtre de Shinku
  ouverte au départ, elles ont suffi (rien n'est resté, rien n'est revenu cinq secondes
  plus tard), sur une machine rapide et un cache presque vide. Tout cela tient dans une **échéance totale de
  vingt secondes** (`TOTAL_DEADLINE`), donnée par l'horloge de l'hôte et regardée
  entre deux gestes et, dans une arborescence, **entre deux entrées** : à
  l'échéance, on s'arrête, ce qui reste est compté comme restant (code 1). Un
  geste commencé n'est pas interrompu : au pire vingt secondes plus un geste, sous
  les trente que le désinstalleur attend. Seul un fichier « occupé » se réessaie ;
  un lien, un droit refusé ou un fichier à plusieurs noms sont laissés sans
  attente.
- **La seconde de vie minimale** est un garde-fou d'attente, pas une fonction :
  le désinstalleur ne reçoit de `RunAsUser` ni poignée ni code de sortie, il ne
  voit le processus qu'en interrogeant Windows sur lui (voir plus bas), et doit
  pouvoir le voir exister. Le processus dort donc le reste de la seconde, même
  quand il refuse ou n'a rien à faire. L'horloge et l'attente passent par l'hôte :
  les essais ne dorment pas.
- **Codes de sortie** : `0` tout est parti, ou il n'y avait rien ; `1` il reste
  quelque chose (un fichier occupé au bout du réessai, un lien laissé, un fichier
  hors liste qui garde son dossier, un droit refusé, l'échéance, **un dossier
  connu que Windows ne sait pas donner** : rien n'y a été regardé) ; `2` refusé
  parce qu'élevé ou parce que les droits ne se lisent pas (y compris ceux du
  bureau, quand ils comptent). Ils servent aux essais : le désinstalleur ne peut
  pas les lire.
- **La fenêtre ou l'agent encore ouverts** : `--oublier` ne le sait pas et n'a
  pas à le savoir. Il efface ce qu'il peut ; un fichier qu'ils tiennent est
  « occupé » et reste après le réessai. Seul le désinstalleur l'appelle, après
  avoir fermé la fenêtre et l'agent. Un programme du compte qui le lance
  lui-même n'y gagne rien : il peut déjà effacer ces fichiers.
- **Une panique** en version finale (`panic = "abort"`) arrête le processus sans
  code 0, 1 ni 2 et sans la seconde de vie ; Windows peut écrire un rapport
  d'erreur. Aucune panique atteignable n'a été trouvée. Le désinstalleur n'a
  rien à ajouter : il traite ce cas comme « jamais vu occupé » ou « fini » selon
  l'instant où le processus a disparu.

**Le désinstalleur** (`installeur/oublier.nsh`, appelé par `habillage.nsh`).
Cette partie est en cours d'écriture par l'autre constructeur : **le détail est
dans `apps/shinku/src-tauri/installeur/oublier.nsh`**, qui fait foi. Ce que la
conception fixe, dans `NSIS_HOOK_PREUNINSTALL`, dans cet ordre :

1. fermer la fenêtre de Shinku d'abord, par la macro du modèle
   (`CheckIfAppIsRunning`, la même question « Shinku est ouvert » qu'une
   seconde plus tard) : cela ferme aussi le trou d'avant, la fenêtre qui
   relançait l'agent entre les deux crochets ;
2. l'agent (`ShinkuAgentPreUninstall`), puis l'assistant, inchangés ;
3. si la case est cochée et hors mise à jour (`$DeleteAppDataCheckboxState = 1`
   et `$UpdateMode <> 1`) : garder la réponse dans une variable à nous, écrire
   `StrCpy $DeleteAppDataCheckboxState 0` (le bloc du modèle ne fait plus ses
   deux `RmDir /r`), lancer `"$INSTDIR\shinku.exe" --oublier` par
   `nsis_tauri_utils::RunAsUser` (le greffon que le modèle embarque déjà pour
   « Lancer Shinku »), puis **attendre en interrogeant le Gestionnaire de
   redémarrage de Windows** sur le chemin exact `$INSTDIR\shinku.exe` : jusqu'à
   dix secondes pour le voir en marche, puis jusqu'à trente pour le voir fini ;
   ces deux attentes sont comptées **en temps réel**, pas en nombre de pas. Une
   réponse inattendue du Gestionnaire compte comme un échec de l'attente, jamais
   comme « fini » ; quelques réponses « on ne sait pas » de suite sont tolérées
   avant d'abandonner, et quand il abandonne sur « on ne sait pas », le
   désinstalleur attend la fin de l'échéance de `--oublier` (un peu plus de vingt
   secondes depuis le lancement) avant de continuer, pour ne pas supprimer
   `shinku.exe` sous lui, puis affiche le message. Si le processus tourne encore
   au bout du délai, il est **arrêté** par ce même Gestionnaire. Portée : le
   Gestionnaire arrête **tout processus qui utilise ce fichier**, ce qui peut
   inclure le Shinku ouvert par un autre compte du PC s'il le voit (non essayé : il
   faut deux comptes) ; de même il peut répondre « en marche » pour un tel Shinku
   sans que `--oublier` ait tourné. C'est la même portée que la fermeture de la
   fenêtre par le modèle de Tauri, juste avant. S'il n'a jamais été vu, ou s'il a
   dû être arrêté :
   un message, « Shinku n'a pas pu supprimer tes réglages. Ils sont dans
   %APPDATA%\Shinku et %LOCALAPPDATA%\Shinku : tu peux les supprimer à la
   main. », avec `/SD IDOK`. **Jamais de repli sur un effacement élevé.**

**L'état que laisse un `--oublier` arrêté.** Les fichiers de réglages partent en
premier : au moment de l'arrêt, ils sont déjà partis (ou occupés), et le message
peut donc être trop pessimiste pour eux. Seul le cache peut rester en partie : une
suppression par handle est faite ou pas faite, elle ne laisse pas de fichier
abîmé, et `--oublier` n'écrit rien. Un arrêt en pleine arborescence laisse une
arborescence entamée, jamais un fichier à moitié.

Dans `NSIS_HOOK_POSTUNINSTALL`, `ShinkuDeleteSettings` a disparu. Quand notre
variable dit que la case était cochée, le crochet refait seulement le ménage du
**registre** que le bloc du modèle faisait (la clé de l'emplacement
d'installation et la langue de l'installeur) : ce ne sont pas des fichiers d'un
profil. `/S` et le mode passif ne montrent pas la page de la case : elle vaut 0,
donc **une désinstallation silencieuse n'appelle jamais `--oublier`** et ne
supprime jamais les réglages, comme avant.

**Ce que coûte l'effacement.** Effacer `startup.json` fait oublier ce que Shinku
avait coupé au démarrage : les entrées restent coupées, et se rallument depuis le
Gestionnaire des tâches de Windows. Effacer `game.json` efface le journal du mode
jeu : si l'agent n'a pas pu tout remettre en place en sortant (le mode
d'alimentation, par exemple), la trace de l'état d'avant part avec lui, et une
réinstallation ne le reprendra pas. Effacer `uninstalls.json` efface les
désinstallations notées du Journal, rien d'autre. Case décochée, ces fichiers
restent et sont repris.

**Ce qui le tient.**

- Cœur, sous Linux : la ligne de commande exacte ; la liste fermée (noms exacts,
  et couverture de ce que l'ancienne macro effaçait) ; l'ordre ; les trois codes ;
  le refus élevé sans un appel autre que la question d'élévation ; la décision
  « inconnu refuse, connu suit la fenêtre » ; le dossier connu introuvable (code
  1) ; le réessai borné **en tout** (dix secondes pour des fichiers et une
  arborescence occupés, le temps de travail compté, la dernière pause raccourcie) ;
  l'échéance de vingt secondes (au milieu d'une arborescence, et avec un hôte lent) ;
  la seconde de vie minimale ; le nom de Tauri contre `tauri.conf.json`. Sur de
  vrais dossiers, avec un changement glissé entre deux gestes : la racine, puis un
  dossier, remplacés après leur lecture ; un dossier disparu ; un dossier rempli
  avant son retrait ; l'échéance entre deux entrées. Et : un fichier à deux
  noms, un lien à la place du fichier, un lien au milieu de la chaîne (au-dessus
  d'un fichier, d'un dossier vide, d'une arborescence), une racine qui est un
  lien, un lien plus bas, les deux bornes, et `forget::run` de bout en bout
  (témoins voisins intacts).
- **Mutations.** Chaque règle citée ici a été retirée, et un essai a échoué :
  refus élevé, inconnu qui refuse, seconde de vie, horloge unique du réessai,
  échéance (dans l'arborescence et entre deux gestes), identité de la racine,
  identité de chaque dossier, `NotEmpty` jamais « parti », dossier disparu,
  `regular && !online_only` (par une fonction pure, `acceptable` : un fichier
  « en ligne uniquement » ou à point d'analyse inconnu ne se fabrique pas ici),
  chaîne des dossiers (fichier nommé, dossier vide, arborescence), sans identifiant,
  noms impossibles, dossier connu introuvable, bornes. **Une seule règle n'a pas
  d'essai à son point d'appel** : le refus d'un fichier nommé sans identifiant
  dans `remove_plain_file` (un système de fichiers de développement en donne
  toujours un). La décision qu'il appelle (`plan_for`) est, elle, essayée sans
  disque.
- **La garde « un module gagne un fichier »** lit les sources livrées du cœur, de
  l'assistant, de l'agent et de la fenêtre, **par fichier et par nombre
  d'occurrences**, sans exempter un fichier en entier. Chaque module d'essais est résolu à partir du fichier qui le déclare (un essai de la garde elle-même le vérifie : un `fake.rs` livré n'est pas confondu avec un `fake.rs` d'essais). Elle ne retire que les
  modules d'essais (`#[cfg(test)] mod x;` et leurs fichiers, `#[cfg(test)] mod x {`
  fermé en colonne 0, éléments isolés) : le code qui suit un module d'essais est lu.
  Elle échoue si : un cinquième appel de `write_replacing` apparaît (un par magasin,
  deux dans `settings.rs` avec sa définition) ; un magasin définit un deuxième nom de
  fichier ; `File::create`, `fs::write`, `.create(true)`, `.create_new(true)`,
  `.append(true)` ou `fs::rename` apparaissent ailleurs que là où ils sont
  aujourd'hui ; ou un nom de fichier (`.json`, `.tmp`, `.bak`, `.old`, `.log`,
  `.ini`, `.toml`, `.dat`) écrit entre guillemets dans le code livré sort de la
  liste épinglée. **Elle n'attrape pas** : une écriture d'une autre sorte (API de
  Windows, `OpenOptions` composé autrement, autre crate), un nom de fichier
  construit au lieu d'être écrit en entier, un fichier écrit par WebView2 ou par
  Tauri lui-même, un élément `#[cfg(test)]` écrit en retrait. Ses mutations
  (File::create dans `forget/mod.rs`, `fs::write` dans `apps/mod.rs`, un
  `settings.bak` dans `settings.rs`, un cinquième appel dans `journal.rs`) ont été
  faites : elle échoue à chacune.
- `installer.test.ts` épingle l'ordre du `PREUNINSTALL`, le
  `StrCpy $DeleteAppDataCheckboxState 0` et la condition `$UpdateMode <> 1`
  autour de l'appel, l'absence de tout `Delete`, `RMDir` ou `RmDir` vers
  `$APPDATA` ou `$LOCALAPPDATA` dans nos `.nsh`, et que, dans le modèle embarqué
  par la version de Tauri installée, le bloc des deux `RmDir /r` est toujours
  gardé par la case et placé après `NSIS_HOOK_PREUNINSTALL` : si Tauri change
  cela, l'essai casse au lieu de laisser revenir l'effacement élevé.
- **Sous Wine**, `shinku.exe --oublier` construit pour Windows est lancé pour de
  bon : Wine répond « élevé » à `runs_elevated` (même en simple utilisateur), le
  programme refuse donc ; on y voit le code 2, aucun témoin touché, aucune
  fenêtre ni boîte, et une durée d'au moins une seconde (un lancement de Wine
  seul dure 0,14 s). Le chemin qui efface n'y est pas atteint par le vrai
  programme : il l'est par `forget::run` sur la vraie implémentation Windows de
  `fs`, avec une élévation fabriquée. Wine ne sait pas créer de jonction
  (`mklink /J` répond « fait » sans rien créer) : **neuf essais de liens
  sautent sous Wine** (huit dans `forget_fs`, un dans `forget_run`). Ils
  impriment « SAUTÉ » sur la sortie d'erreur, mais **comptent « ok » dans le
  bilan de cargo** : un bilan vert ne les distingue pas des réussis. Les mêmes
  tournent sous Linux avec des liens symboliques.
- Vrai Windows : voir « Sur le vrai Windows », plus bas.

**Limites, écrites ici telles quelles.**

- **Le désinstalleur ne lit pas le résultat de `--oublier`** : `RunAsUser` ne rend
  ni poignée ni code. S'il reste un fichier occupé au bout du délai, rien ne le
  dit à la personne.
- **« Mes réglages » sont ceux de la personne devant le bureau.** Les réglages des
  autres comptes du PC restent.
- **L'agent retiré dans le mauvais compte : une limite connue, non corrigée.**
  `--retirer` est lancé par le désinstalleur élevé. Avec un agent en marche, c'est
  l'agent de la personne qui reçoit l'ordre (fenêtre cachée du même bureau, mutex
  de la session) : il remet tout en place et efface sa valeur `Run` dans le bon
  compte, même si l'élévation s'est faite avec un autre compte. Sans agent qui
  réponde **et** avec une élévation par un autre compte, le programme élevé fait
  la reprise et efface la valeur `Run` dans le profil du compte élevé : le
  journal de la personne n'est pas repris, et sa valeur `Run` reste, pointée vers
  un fichier supprimé (Windows l'ignore). Voir « Avant une version publique ».
- **Le modèle de Tauri touche encore le profil du compte qui a élevé**, hors de la
  case : il retire les raccourcis, l'épinglage et la valeur `Run` de `HKCU` (des
  appels ciblés, sans récursion). Même défaut de cible que l'agent quand
  l'élévation s'est faite avec un autre compte.
- **Un profil derrière une jonction** (`C:\Users\Léa` déplacé par un lien) fait
  refuser tout : les fichiers de la liste comme les arborescences contrôlent la
  chaîne de leurs dossiers, rien n'est effacé, le code est 1. Cet écart avec
  l'ancienne macro (qui ne regardait que le dossier `Shinku` lui-même) est voulu :
  mieux vaut tout garder que perdre le cache et garder les réglages. À vérifier
  sur un profil déplacé.
- **La lecture d'un dossier n'est pas interrompue.** Un dossier est lu d'un bloc
  par le système, et l'échéance n'est regardée qu'entre deux entrées. La liste
  d'un dossier n'est gardée en mémoire que dans la limite des 50 000 entrées
  permises, mais la lecture d'un dossier de plusieurs millions d'entrées dure
  autant qu'elle dure.
- **Un réglage occupé n'est plus réessayé une fois le budget de dix secondes
  dépensé**, même s'il reste du temps avant l'échéance de vingt secondes : si
  l'arborescence d'une même passe a déjà consommé dix secondes, le réessai de la
  suivante n'a pas lieu. Le code 1 reste juste (il reste bien quelque chose).
- **Windows n'offre pas ici d'ouverture relative à un handle de dossier** : une
  entrée est ouverte par son chemin et revérifiée (nature, taille, date,
  identifiant) avant d'être supprimée sur ce handle. Un dossier du chemin
  remplacé par un lien entre la lecture et l'ouverture ne ferait partir que ce
  qui a exactement la même identité, la même taille et la même date (sur NTFS et
  ReFS ; sans identifiant, rien n'est supprimé, voir plus haut). Le processus
  n'est pas élevé : il n'a de toute façon que les droits de la personne.

**Sur le vrai Windows** (machine de GitHub, Windows 11 build 26100, passage du
16 novembre 2026, les trois étapes de `essai-installeur.ps1`, toutes vertes au premier
passage). Ce qui est vu :

- `RunAsUser` transmet l'argument : pendant la désinstallation, le script a vu
  `"C:\Program Files\Shinku\shinku.exe" --oublier`, et les fichiers sont partis.
- Le Gestionnaire de redémarrage voit ce processus en marche puis fini : le message
  d'échec ne s'est pas affiché, et la désinstallation n'a pas attendu son délai.
- `--oublier` lancé à la main avec un jeton filtré (les droits ordinaires) : code 0,
  1,01 s de vie, les 11 témoins partis, les deux dossiers `fr.shinku.app` partis.
- Fenêtre de Shinku ouverte au départ, case cochée dans la vraie page : la question
  « Shinku est ouvert » est posée une seule fois ; `%APPDATA%\Shinku`,
  `%LOCALAPPDATA%\Shinku` et les deux `fr.shinku.app` ont disparu avec les 9 fichiers
  de réglages et de journaux ; rien n'est revenu cinq secondes plus tard ; le dossier
  d'installation et le raccourci du menu Démarrer ont disparu. Le réessai de dix
  secondes a donc suffi à un vrai WebView2 qui sort.
- Une vraie jonction NTFS à la place de `%LOCALAPPDATA%\fr.shinku.app` : les six
  éléments derrière elle sont intacts, la jonction est laissée en place, et le reste
  (réglages, `%APPDATA%\fr.shinku.app`) est parti.
- Sur un vrai NTFS, les essais de liens de `forget_fs` (21 essais) et `forget_run` (6)
  passent ; un bilan vert de cargo cache leur sortie, donc une éventuelle ligne
  « SAUTÉ » ne s'y verrait pas.

Ce qui **n'est pas** vu, et pourquoi :

- **Le lancement « avec les droits ordinaires » par `RunAsUser` n'est pas prouvé.** Sur
  la machine de GitHub tout est élevé, l'Explorateur de la session aussi : `RunAsUser`
  y a donc lancé `--oublier` avec un jeton élevé, face à un bureau élevé, ce que
  `--oublier` accepte (tout le PC tourne avec tous les droits) ; c'est déduit du jeton
  de l'Explorateur, que le script imprime, pas lu sur le processus lui-même. Ce que
  l'essai prouve est le passage de l'argument, l'attente et le résultat, pas la baisse
  des droits. Elle ne se verra que sur le PC de quelqu'un : désinstaller avec la case,
  sans message d'échec et sans reste.
- **Le code 2** (`--oublier` élevé face à un bureau qui ne l'est pas) : même raison,
  l'étape l'écrit en `NOTE`. La décision elle-même est tenue par les essais du cœur,
  avec une élévation fabriquée.
- Le `RMDir /r` de NSIS suit-il une jonction ? Non mesuré, et plus nécessaire : le
  désinstalleur ne le joue plus dans un profil (tenu par `installer.test.ts`).
- Les deux cas à deux comptes (compte ordinaire et mot de passe d'un administrateur ;
  protection administrateur de Windows 11) : jamais essayables en intégration continue,
  ils restent une limite dite.
- La durée réelle d'une passe sur un gros cache, sous Defender, sur un disque lent.
- Un volume FAT ou exFAT (sans identifiant), des dossiers redirigés sur un
  partage réseau, un fichier « en ligne uniquement », un profil déplacé.
- « Rien d'écrit sur le disque » : un arrêt brutal (panique) peut faire écrire un
  rapport par Windows.

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
le disque de la machine, laisser les doublons décochés, demander confirmation
avant de faire le vide, montrer chacun de ses écrans, enregistrer un réglage,
refuser de s'ouvrir en double et se fermer. Sur cette machine jetable, il va
au bout du grand vide (`--grand-vide`) : il dépose un faux fichier inutile et
deux fichiers identiques, confirme, puis vérifie sur le disque que le premier
est parti et qu'il reste un exemplaire des seconds. Les tests du cœur y
essaient aussi la vraie corbeille de Windows (`SHINKU_ESSAI_CORBEILLE`).
Une erreur dans la console de la page ou un message d'erreur à l'écran fait
échouer l'essai. L'installeur n'est publié que s'il passe.

L'habillage a son propre essai, `apps/shinku/scripts/essai-installeur.ps1`
(Windows PowerShell 5.1, sans dépendance), lancé en trois étapes avant et
après celui de la fenêtre. Il parcourt le vrai installeur comme une personne :
il attend chaque page par son titre et ses textes, lit tout ce qu'elle
affiche, échoue si un texte manque ou vouvoie, photographie la fenêtre, puis
clique par `BM_CLICK`.

- `installer` : Bienvenue, Dossier, Installation, Terminé (« Lancer Shinku » est
  décoché, le raccourci du bureau doit l'être déjà), puis code de sortie,
  `shinku.exe`, `shinku-agent.exe`, `uninstall.exe`, pas de raccourci sur le
  bureau, un raccourci dans le menu Démarrer ;
- `deja-installe` : la page « Shinku est déjà installé », puis Annuler. Rien ne
  doit changer sur le disque ;
- `oublier` : `shinku.exe --oublier` lancé à la main, avec les droits ordinaires
  (`lancer-en-utilisateur.ps1`) : code 0, témoins partis, durée d'au moins une
  seconde ; puis, si le runner le permet, élevé face à un bureau qui ne l'est
  pas : code 2 et rien de touché ;
- `desinstaller-jonction` : la désinstallation avec la case cochée, un témoin posé
  derrière une jonction qui remplace un des deux dossiers `fr.shinku.app` : le
  témoin doit rester intact ;
- `desinstaller` : la page de désinstallation, la case « Supprimer aussi mes
  réglages », puis le dossier d'installation, les fichiers de réglages, les deux
  dossiers `Shinku` et les deux dossiers `fr.shinku.app` doivent avoir disparu, et
  aucun fichier ne doit revenir quand la fenêtre était ouverte au départ. L'étape
  tourne en dernier, sur ce que les essais d'avant y ont vraiment écrit (réglages
  de la fenêtre, journal du grand vide) : un fichier oublié dans la liste de
  `forget::list` ferait rester son dossier, et le code 1 de `--oublier`.

Un aller-retour silencieux (`/S`) garde la preuve que ce mode marche. Les
captures sont publiées (artefact `captures-de-l-installeur`) ; un lancement à
la main avec `captures_dans_le_journal` les écrit aussi dans le journal, en
JPEG base64 entre deux lignes repères, pour les lire sans télécharger
l'artefact. Les pages d'installation et de désinstallation en cours sont
guettées sans être exigées : elles ne durent que quelques secondes.

Les limites : l'essai tourne à 100 % de mise à l'échelle seulement, comme la
machine de GitHub. Le choix des images à 125, 150, 175 et 200 % et la mise en
page à ces échelles n'ont été vus que sous Wine, dont les polices ne sont pas
Segoe UI : seule la mise en page y est jugée.

L'essai lit la page et clique par le port de débogage de WebView2, demandé de
l'extérieur par une variable d'environnement : `shinku.exe` n'a aucune option
d'essai.

Il essaie aussi l'interrupteur principal du mode jeu : éteint au départ ;
allumé, plus d'avertissement et un processus `shinku-agent.exe` tourne ; éteint,
l'agent sort ; l'état de départ est restauré.

Un second essai, `apps/shinku/scripts/essai-mode-jeu.ps1`, mène le mode jeu de
bout en bout avec l'agent installé. Il force le mode d'alimentation « Utilisation
normale » (sans cela il ne prouverait rien), écrit des réglages d'essai, puis
lance avec le jeton filtré un `ping` en veilleuse, l'agent, et un faux jeu : une
petite application compilée à l'instant, avec son propre chemin. Un
`powershell.exe` aurait celui de tous les autres processus PowerShell de la
machine, dont celui de l'essai : l'agent croirait la partie encore en cours.
Quand le faux jeu a le premier plan, l'essai exige une session au journal, le
ping en priorité basse et, si le mode « Performances élevées » existe sur la
machine, l'alimentation dedans. Il ferme le jeu, exige un bilan complet, le ping
et l'alimentation revenus, puis arrête l'agent par `--quitter`. Les réglages d'essai retirent aussi les trois apps cochées d'office
(OneDrive, Dropbox, Google Drive) : seul le ping est touché. Une étape de la CI,
exécutée quoi qu'il arrive, remet le mode d'alimentation, les réglages et le
journal d'origine. Si un essai a été interrompu sans cette remise en place, le
suivant commence par elle ; elle n'arrête que les processus que l'essai a
lancés (chemin et heure de départ notés, comparés avant d'arrêter quoi que ce
soit), jamais un agent qu'il n'a pas lancé (s'il en trouve un au départ, il
s'arrête avec un message), et relance `--quitter` après un arrêt forcé pour
que le journal soit repris avant d'être remplacé. Son dossier temporaire est
effacé à la fin. Si Windows refuse le premier plan à la fenêtre du faux jeu,
l'essai le dit (« premier plan refusé par la machine d'essai ») et échoue : pas
de faux succès.

**Ce que l'essai prouve de l'alimentation, et ce qu'il ne prouve pas.** Avant de
lancer l'agent, une sonde fait, avec le jeton filtré de l'agent, le même appel
que lui : `powercfg /setactive` vers « Performances élevées ». Sur un Windows de
particulier, un compte ordinaire en a le droit. Sur un Windows Server, comme les
machines de GitHub, `PowerSetActiveScheme` lui est refusé : il faut les droits
d'administrateur. L'essai affiche le résultat de la sonde, la sortie de
`powercfg /getsecuritydescriptor ActionSetActive` et `whoami /groups` pris avec
les droits ordinaires, pour le compte rendu, puis exige de l'agent ce que la
sonde a montré. Sonde réussie : l'agent dit `applied`, l'alimentation passe en
« Performances élevées » puis revient. Sonde refusée : l'agent dit `refused`,
l'alimentation n'a pas bougé, et l'essai continue (veilleuse, fin de partie,
bilan) en écrivant : « cette machine refuse le changement d'alimentation à un
compte ordinaire : le passage en Performances élevées n'est pas prouvé ici ».
Sur les machines de GitHub, le passage effectif n'est donc **pas prouvé** ; il
l'est seulement par un essai sur un Windows 11 de particulier.

L'essai peut pourtant mettre la machine dans l'état d'un Windows 11. La
description de sécurité de l'action (`powercfg /getsecuritydescriptor
ActionSetActive`) donne, sur le Windows Server de GitHub, la seule lecture aux
comptes ordinaires (`(A;CI;KR;;;BU)`) ; un Windows 11 de particulier les laisse
changer de mode. Quand la sonde échoue, que l'essai tourne en administrateur et
que cette entrée est présente une seule fois, il pose la même description avec
`KRKW` pour `BU` seulement (`powercfg /setsecuritydescriptor`), refait la sonde,
et, si elle passe, écrit « machine mise dans l'état d'un Windows 11 : un compte
ordinaire peut changer l'alimentation » puis exige `applied`, le mode
« Performances élevées » pendant la partie et le retour au mode d'avant. La
description d'origine est notée dans `etat.json` avant le changement, remise à
la fin (y compris par `-Restaurer` et après une interruption) et relue : si elle
n'est pas identique, l'essai échoue. Si la sonde échoue encore, ou si l'essai
n'a pas les droits, on retombe sur la branche `refused` ci-dessus. Ce que cela
prouve : l'agent applique le mode quand le droit existe, pas que Windows 11
donne ce droit (c'est un fait de Windows, pas de Shinku).

Un troisième essai, `apps/shinku/scripts/essai-installeur-agent.ps1`, exerce les
crochets de l'installeur, que l'installation sur machine vierge ne touche pas
(pas encore d'agent). Il part de l'installation faite par la CI : réglages
d'essai avec le mode automatique allumé, valeur `Run\Shinku` posée (c'est
`shinku.exe` qui l'écrit, pas l'agent), agent lancé avec le jeton filtré. Sur la
CI, l'installation vient du même installeur : l'essai ajoute d'abord un octet à
la fin de `shinku-agent.exe` installé, et vérifie que son hachage diffère alors
de l'agent compilé, pour que le retour du hachage prouve le remplacement (en
silencieux, un `File` qui échoue est ignoré et l'installeur rend quand même 0).
Il lance aussi `shinku.exe` avant la mise à jour. Il relance l'installeur tout
juste fabriqué en `/S` et exige : code 0, `shinku-agent.exe` remplacé (identique
à l'agent compilé), `shinku.exe` fermé par l'installeur, aucun agent en marche
(l'ancien est parti, rien ne l'a relancé), valeur `Run` toujours là. Il relance
alors l'agent et lance `uninstall.exe /S`, le chemin de « Paramètres >
Applications » : NSIS se recopie dans `%TEMP%\~nsu<n>.tmp\Un.exe` (NSIS 3.13,
téléchargé par Tauri) et rend la main tout de suite. L'essai attend ce processus,
borné, puis exige, dans l'ordre où le désinstalleur finit : dossier
d'installation disparu, clé `HKLM\...\Uninstall\Shinku` disparue, valeur `Run`
effacée, plus aucun agent. Le code de sortie n'est pas lisible sur ce chemin : un
crochet qui abandonne se voit à la clé ou au dossier restés là. La remise en
place ne tourne jamais tant qu'un désinstalleur recopié vit encore. La valeur
`Run` est aussi effacée par le modèle de Tauri : son absence prouve l'état final,
pas lequel des deux l'a effacée. Le contrôle « alimentation inchangée » est un
garde-fou, pas une preuve : aucune partie n'a lieu. Shinku n'est plus installé
ensuite : les deux étapes suivantes (l'aller-retour silencieux, puis la
désinstallation à la main) le réinstallent elles-mêmes. Les
réglages, le journal et la valeur `Run` d'origine sont remis comme pour l'essai
précédent (`--quitter` d'abord, arrêt forcé ensuite). Il ne couvre pas un agent
qui refuse de sortir (le `taskkill` puis la reprise du journal), un agent en
pleine partie, ni la relance de l'agent par la page Mode jeu entre les deux
crochets (la fenêtre n'est pas pilotée) : ces cas ne sont couverts que par la
lecture du code.

Un quatrième essai, `apps/shinku/scripts/essai-demarrage.ps1`, prépare la page
Démarrage avant celui de la fenêtre (`-Preparer`) et la remet en place après,
quoi qu'il arrive (`-Nettoyer`). Il crée cinq entrées d'essai aux noms
`Essai Shinku …` : trois valeurs `Run` (dont une avec une commande portée par
`powershell.exe`, une vers `notepad.exe`, qui a une description, et une sous
`Wow6432Node`, c'est-à-dire `Run32`), un raccourci dans le dossier Démarrage du
compte et une valeur `Run` de `HKLM` pour la ligne du PC. Il affiche aussi, pour
le compte rendu, les groupes du jeton, les valeurs du contrôle de compte
d'utilisateur et le contenu des clés `StartupApproved` : de quoi préparer le lot
suivant. `-Nettoyer` ne retire que ces cinq entrées et leurs approbations, par
leurs noms exacts. L'essai de la fenêtre (`checkStartup`) vérifie ensuite les
quatre lignes du compte et la ligne du PC (sans interrupteur), que la description
du fichier a été lue (« inscrite sous »), puis, pour chacune des quatre lignes du
compte, éteint l'interrupteur (la zone de retour dit « est coupée » et
`reg query /reg:64` montre douze octets commençant par `03000000`), puis remet
l'entrée par « Annuler » ou par l'interrupteur (valeur
`020000000000000000000000`). Les entrées appartiennent au compte de la machine
d'essai ; Shinku y tourne avec un jeton filtré du même compte, `HKCU` est donc le
même.

Les machines de GitHub font tout tourner en administrateur. Shinku y est donc
lancé avec un jeton filtré, comme celui que donne le contrôle de compte
d'utilisateur (`apps/shinku/scripts/lancer-en-utilisateur.ps1`) : ce sont les
droits qu'il a sur le PC de quelqu'un, et WebView2 n'écoute cette variable
d'environnement que dans un programme sans droits d'administrateur.

## Licences des dépendances

Shinku embarque des composants écrits par d'autres : des paquets Rust (MIT,
Apache-2.0, BSD, Unicode-3.0, Zlib, MPL-2.0…), `lit-html`, `@tauri-apps/api`,
deux polices sous licence OFL et quelques composants de l'installeur. Presque
toutes ces licences demandent que leur texte accompagne chaque copie distribuée.
L'installeur dépose donc `LICENCES-TIERS.txt` à côté de `shinku.exe`.

**Ce que fait `apps/shinku/scripts/licences-tiers.mjs`** (Node 22, sans
dépendance). Il fabrique le fichier, par défaut dans
`apps/shinku/src-tauri/licences/LICENCES-TIERS.txt` (ignoré par git) :

- *Les paquets Rust* : ceux que les trois programmes lient vraiment, c'est-à-dire
  l'union de `cargo tree --locked -e normal,no-proc-macro --target
  x86_64-pc-windows-msvc --prefix none` lancé comme la fabrication compile, une
  fois pour la fenêtre (`-p shinku`, ce que `tauri build` compile) et une fois
  pour l'agent et l'assistant (`-p shinku-agent -p shinku-helper`, ce que
  `beforeBuildCommand` compile). Les outils qui servent seulement à fabriquer les
  programmes (dépendances de construction) et les macros procédurales ne sont
  pas dans les programmes : ils ne sont pas listés. Ce choix remplace
  `-e normal,build`, dont le résultat dépendait de la machine : `cargo tree`
  résout les sous-arbres de construction et de macros pour la machine qui le
  lance, si bien que le fichier fabriqué sous Windows n'était pas celui que la
  garde vérifiait sous Linux (une dépendance de construction propre à Windows, sans
  texte, aurait passé la garde sous Linux et cassé l'installeur). Avec
  `normal,no-proc-macro` et la cible Windows, la liste se calcule pour cette cible
  et ne dépend plus de la machine qui la calcule. **Vu le 16 novembre 2026** dans
  les journaux de la CI (commit `1e7f43d`) : la garde sous Linux et la fabrication
  sous Windows écrivent la même empreinte, pour 138 paquets Rust (voir plus bas).
  Les paquets du dépôt
  (sans `source`) sont écartés ; le dossier, l'expression de licence et l'adresse
  du dépôt de chacun viennent de `cargo metadata --format-version 1 --locked`.
  `--hors-ligne` (ou `SHINKU_CARGO_HORS_LIGNE=1`) ajoute `--offline` aux appels de
  cargo.
- *Les paquets de la page* : les `dependencies` de `package-lock.json` (donc leurs
  dépendances transitives) et les deux polices, nommées dans `PAGE_DEV_PACKAGES`
  parce qu'elles sont en `devDependencies` mais que leur contenu part dans la page.
  Un essai échoue si `src` importe (code ou CSS) un paquet qui n'est dans aucune
  des deux listes.
- *Les textes* : les fichiers `LICENSE*`, `LICENCE*`, `COPYING*`, `NOTICE*` et
  `UNLICENSE*` de la racine du paquet (casse ignorée, liens non suivis, 256 Kio au
  plus, non vides une fois normalisés). Un `NOTICE*` est toujours recopié (Apache-2.0
  le demande) mais ne compte pas comme texte de licence. Les textes identiques
  (fins de ligne normalisées) ne sont écrits qu'une fois ; chaque paquet renvoie
  aux siens par un numéro.
- *La section fixe* : `src-tauri/licences/section-fixe.txt`, écrite à la main,
  recopiée en tête : la bibliothèque standard de Rust, NSIS, le greffon
  `nsis_tauri_utils`, le chargeur du SDK WebView2, l'amorce de WebView2 et la
  bibliothèque d'exécution Visual C++. Les textes qu'on n'a pas pu tirer d'une
  source sûre ne sont pas écrits : la section nomme le composant, d'où il vient,
  et renvoie à l'adresse officielle de sa licence.
- *MPL-2.0* : chaque paquet sous MPL-2.0 porte la phrase qui dit où obtenir le
  code source de sa version (adresse du dépôt et version).
- La sortie est triée, sans date ni chemin de la machine : deux passages sur la
  même machine donnent le même fichier, octet pour octet (essayé sous Linux).
  Le script écrit à la fin sur la sortie d'erreur une ligne
  `licences-tiers : sha256 <empreinte>` : celle du contenu fabriqué, avec ou sans
  `--verifier`. L'étape « Licences des dépendances » du travail « Cœur et
  interface » de la CI lance `--verifier` (après `npm ci`, qui apporte
  `node_modules`) pour écrire cette ligne dans le journal ; `beforeBuildCommand`
  l'écrit dans celui du travail Windows. Les deux lignes doivent être égales :
  elles l'étaient le 16 novembre 2026. Rien ne les compare tout seul : c'est à relire
  dans les journaux quand le script ou la fabrication change.

**La garde** : `node scripts/licences-tiers.mjs --verifier` n'écrit rien et sort
en erreur, en nommant chaque paquet fautif, si une expression de licence n'est pas
satisfaite par la liste permise (`ALLOWED_LICENSES`, dans le script : MIT, MIT-0,
Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib,
0BSD, Unlicense, CC0-1.0, Unicode-3.0, Unicode-DFS-2016, BSL-1.0, MPL-2.0,
OFL-1.1 ; `OR` et l'ancien `/` : une suffit ; `AND` : toutes ; `WITH` fait partie
du nom), si une expression est illisible ou absente, si un paquet n'a aucun texte
de licence (un `NOTICE` n'en est pas un), ou si un fichier au nom de licence est
écarté sans que rien le dise : vide après normalisation, de plus de 256 Kio, lien
ou autre chose qu'un fichier ordinaire (l'erreur nomme le paquet et le fichier ; seul
un dossier au nom de licence, comme `LICENSES/`, reste ignoré, et seulement quand
le paquet a par ailleurs un texte et que son expression n'a aucun `AND` : avec un
`AND`, un des textes demandés peut être dans ce dossier, qui n'est pas lu, et
l'erreur nomme le paquet et le dossier). Les compléments passent par les mêmes règles.
Fabriquer le fichier lance la même garde et n'écrit rien si elle
échoue. Un essai de `npm test` la lance sur le vrai dépôt ; il échoue (il ne saute
pas) si `cargo` manque. En CI, `cargo metadata` donne le dossier de chaque paquet :
cela suppose que cargo ait téléchargé les sources (non vérifié ici, hors ligne).

**Les compléments à la main.** Un paquet dont le dossier ne contient aucun texte
reçoit un dossier `src-tauri/licences/complements/<nom>@<version>/` (versionné ;
le nom d'un paquet à portée, comme `@x/y`, n'est pas géré : les paquets Rust n'en
ont pas et la page n'a pas besoin de complément) :
le fichier de licence, pris dans le dépôt d'origine (à l'étiquette ou au commit de
la version utilisée) ou, à défaut, dans une autre source sûre, jamais écrit de
mémoire, et un `SOURCE.txt` qui dit d'où il vient. Un complément s'ajoute aux textes du paquet **Rust** de ce nom **et de cette version**
seulement (un paquet de la page de même nom et de même version ne le reçoit pas ; le
complément est alors orphelin, donc une erreur), et sa provenance (`SOURCE.txt`) paraît sous le paquet dans le fichier
fabriqué. Un complément qui ne va à aucun paquet du graphe est une erreur : une
montée de version oblige donc à revérifier le texte. Un `SOURCE.txt` vide est une
erreur, de même que deux dossiers de compléments dont les noms ne diffèrent que par la
casse (ils se heurteraient sous Windows). Un complément pour un paquet qui
a déjà ses propres textes de licence (hors `NOTICE`) est aussi une erreur
(« inutile, à supprimer »). Aujourd'hui : `alloc-stdlib@0.3.0`,
`webview2-com@0.39.1` et `webview2-com-sys@0.39.1`. Les compléments de `selectors` et
de `webview2-com-macros` ont été supprimés : ces deux paquets ne sont pas liés
dans les programmes (ils n'apparaissent plus dans la liste).

**Ajouter une dépendance.** Rien à faire si sa licence est dans la liste et si son
dossier contient son texte : le fichier la reprend à la prochaine fabrication.
Sinon, `npm test` (ou `--verifier`) échoue en la nommant : soit on ajoute un
complément (texte pris d'une source sûre, avec son `SOURCE.txt`), soit une
personne décide d'accepter la licence et l'ajoute à `ALLOWED_LICENSES` (avec
son essai). Une dépendance de la page qui n'est pas dans `dependencies` mais dont
le contenu part dans la page va dans `PAGE_DEV_PACKAGES`.

**Dans l'installeur.** `beforeBuildCommand` lance le script après les compilations.
`installeur/licences.nsh` (sur le modèle de `helper.nsh`) copie le fichier après
l'installation et le supprime avant la désinstallation, après l'agent et
l'assistant ; ce n'est pas une ressource de Tauri (un fichier manquant casserait
toute compilation Windows, y compris `cargo build` et `clippy` ; seul `makensis`
l'exige). L'essai de l'installeur sur le vrai Windows vérifie que le fichier est
posé, non vide, et nomme `serde` ; l'aller-retour silencieux, qu'il est posé puis
retiré avec le dossier.

**Limites, écrites telles quelles.**

- **La liste est celle des paquets liés, pour la cible Windows.** Un paquet qui
  ne serait lié que pour une autre cible, ou qui ne sert qu'à fabriquer les
  programmes, n'y est pas, et la garde ne le voit pas : c'est voulu (il n'est pas
  distribué), mais si la fabrication change (un autre `--target`, une dépendance
  de construction qui laisse du code dans un programme), la liste est à revoir.
  Un outil de construction peut aussi laisser un fichier dans un programme sans
  laisser de code : `tauri-build` (non listé) a un manifeste Windows par défaut,
  `windows-app-manifest.xml`, qu'il incorpore à `shinku.exe` quand on ne lui en donne
  pas (lu dans `tauri-build` 2.7.4 : `WindowsAttributes::new`, puis `set_manifest`).
  Ici `build.rs` lui donne le manifeste de Shinku (`windows-manifest.xml`), qui le
  remplace : ce fichier-là n'est donc pas dans `shinku.exe`, et nous n'avons trouvé
  aucune autre trace d'un outil de construction dans un programme. Si `build.rs`
  cessait de fournir le sien, le manifeste de `tauri-build` (une simple dépendance
  aux contrôles communs de Windows, de peu de contenu) serait à nommer.
- **La section fixe est écrite à la main** et ne vieillit pas toute seule. Sa
  section A nomme ce que la bibliothèque standard de Rust lie sous Windows (tables
  Unicode, `compiler_builtins` et `libm`, `cfg-if`, `hashbrown`, `rustc-demangle`),
  d'après la chaîne d'outils et le dépôt `rust-lang/rust` à l'étiquette de la
  version ; elle est à relire à chaque changement de `rust-toolchain.toml` (un essai
  vérifie que la version nommée dans la section est celle du fichier). Le
  texte du SDK WebView2 (licence et notices) vient du paquet NuGet officiel
  `Microsoft.Web.WebView2` 1.0.3800.47, la version que le dépôt de
  `webview2-com-sys` désigne ; à reprendre quand ce paquet change de SDK. Les
  textes de l'amorce de WebView2 et de la bibliothèque d'exécution Visual C++ n'y
  sont pas (ce sont des composants que Microsoft laisse redistribuer sous ses
  propres conditions ; leur texte n'a pas pu être obtenu d'une source sûre). Le
  texte de NSIS vient du paquet Debian 3.09, pas de la version que Tauri
  télécharge.
- **La garde lit ce que les paquets déclarent** (leur expression de licence et
  leurs fichiers) ; elle ne lit pas leur code, et elle ne vérifie pas qu'un texte
  fourni est bien celui de la licence déclarée.
- **Les textes de licence reproduisent leurs notices de droits d'auteur
  telles quelles**, noms de personnes compris : ils sont la licence.
- Les licences ne sont pas encore montrées dans la fenêtre (page « À propos »,
  plus tard), et le choix de la licence de Shinku reste à la personne.

## Avant une version publique

- **Signature du code.** L'installeur et les exécutables ne sont pas signés.
  Voir la décision prise sur le certificat individuel.
- **L'agent `--retirer` dans le mauvais compte : une limite, écrite telle quelle
  (non corrigée dans ce lot).** Les faits : avec un agent en marche, c'est
  l'agent de la personne qui reçoit l'ordre (fenêtre cachée du même bureau, mutex
  de la session) et il remet tout en place et efface sa valeur `Run` dans le bon
  compte, même si l'élévation s'est faite avec un autre compte. Sans agent qui
  réponde **et** avec une élévation par un autre compte, le programme élevé fait
  la reprise et efface la valeur `Run` dans le profil du compte élevé : le
  journal de la personne n'est pas repris, et sa valeur `HKCU\…\Run\Shinku`
  reste, pointée vers un fichier supprimé (Windows l'ignore). Le seul dommage
  durable est ce journal non repris ; il l'est au prochain démarrage de l'agent si
  Shinku est réinstallé. Pourquoi ce n'est pas corrigé : le gain demande trois
  conditions à la fois (deux comptes, aucun agent en marche, une élévation par
  l'autre compte) ; la correction ajoute une seconde attente sans poignée ni code,
  un risque de boîte d'erreur de l'Explorateur si le lancement arrive après la
  suppression du fichier, et surtout elle ne peut pas être essayée : la machine de
  GitHub n'a qu'un compte, tout élevé. La forme minimale retenue pour plus tard,
  avec la signature : `RunAsUser`, une seconde de vie minimale pour l'ordre
  `--retirer` (comme `--oublier`), une attente recopiée et non partagée avec
  `oublier.nsh`, et un essai à deux comptes fait à la main.
- **`--oublier` sur le PC de quelqu'un.** Les trois étapes de l'installeur sont
  vertes sur le vrai Windows de l'intégration continue, mais tout y est élevé : la
  baisse des droits par `RunAsUser` et le refus (code 2) ne s'y voient pas (voir
  « Sur le vrai Windows », dans « Effacer les réglages à la désinstallation »).
  Reste l'essai à la main : désinstaller avec la case sur un PC ordinaire.
- **Variables et stratégies de WebView2 : risque accepté.** `shinku.exe` laisse
  WebView2 lire sept variables `WEBVIEW2_*` (dont
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`) et les stratégies de
  `Software\Policies\Microsoft\Edge\WebView2`. Un programme du même compte peut
  ainsi lancer Shinku avec un port de débogage et piloter sa page sans que rien
  ne se voie : c'est ce que font nos essais de la fenêtre. Rien n'est construit,
  et c'est choisi : retirer ces variables ne fermerait rien, car le même
  programme peut injecter du code dans `shinku.exe`, en lancer une copie
  modifiée, simuler des clics, ou lancer l'assistant installé directement. Il
  n'y gagne aucun droit dans tous les cas : ce que la page sait faire sans
  élévation, il sait déjà le faire, et l'accord de Windows reste la seule
  barrière de ce qui est élevé. À revoir avec la signature : si `shinku.exe`
  vérifie un jour quelque chose de lui-même au lancement, piloter les essais par
  une stratégie `HKLM` posée par la CI (administrateur) permettrait de retirer
  les variables sans exception dans le code.
- **Signature de `shinku-helper.exe`.** La sortie publique est conditionnée à
  la signature de l'assistant, comme à celle de `shinku.exe`, et `shinku.exe`
  doit vérifier cette signature avant de le lancer : le dossier sûr ne prouve
  pas le contenu du fichier. Sans signature, la fenêtre de Windows ne distingue
  pas Shinku d'un imitateur. Aujourd'hui rien n'est signé.
- **Crate dédié à l'assistant : repoussé à la signature.** L'assistant lie tout
  `shinku-core` (voir « Limites connues » de « Régler une entrée du PC »). Tant
  qu'il n'est pas signé, détourner une de ses bibliothèques n'apporte rien à un
  attaquant (il peut présenter son propre programme sous le même nom), et
  `/DEPENDENTLOADFLAG:0x800` tient déjà les imports statiques à `System32`. Le
  gain réel sur le binaire publié (MSVC, release, LTO) est faible, d'après la
  table d'imports de `shinku-helper.exe` relevée sur le vrai Windows le
  16 novembre 2026 (374 784 octets) : `kernel32`, `advapi32`, `shell32`, `combase`,
  `oleaut32`, `user32`, `ntdll`, `api-ms-win-core-synch-l1-2-0` et six
  `api-ms-win-crt-*`. Ni `powrprof`, ni `wtsapi32`, ni `dwmapi` : l'éditeur de
  liens a déjà écarté le code du mode jeu et de l'agent. Un crate dédié retirerait
  surtout du code source à relire, pas des bibliothèques chargées. Les
  quatre faits gardés pour ce jour-là : (1) `State`, `Scope` et `Origin` dérivent
  `Serialize` et partent avec `Place` : il faut `#[serde(remote)]` dans le cœur
  (la règle de l'orphelin interdit le reste), pas de fonctionnalité `serde`
  optionnelle ; (2) l'assistant doit être construit dans sa propre commande
  cargo, sinon les fonctionnalités s'unifient avec celles de l'agent, et l'essai
  de l'arbre porte sur cette commande-là ; (3) une seule copie de `known_folder`,
  `Key` et `os_error`, dans le petit crate, `elevation_type` restant dans le
  cœur ; (4) ajouter `-p shinku-elevated` à la ligne d'essais de la CI, sinon les
  essais `cfg(windows)` déplacés cessent de tourner sans que rien ne casse.
- **Recette Démarrage à la main.** Couper puis rouvrir une session, sur un vrai
  Windows 11, pour une entrée de chaque place du compte, dans les deux sens
  (voir « Démarrage »). Aucun essai automatique ne l'a faite.
- **Signer l'agent.** L'installeur embarque `shinku-agent.exe` par un crochet
  NSIS, pas comme une ressource ou un binaire externe de Tauri : rien ne dit que
  Tauri le signera avec `shinku.exe` (à vérifier), il faudra donc le signer
  avant de fabriquer l'installeur. Un agent non signé, qui change la
  priorité d'autres programmes, est exactement ce qu'un antivirus regarde de
  près.
- **Helper et agent.** Leur bibliothèque d'exécution Visual C++ est liée
  statiquement (`build.rs`), comme celle de `shinku.exe`. Vu dans la table d'imports
  de l'assistant publié : aucun `VCRUNTIME140.dll`, seulement les `api-ms-win-crt-*`
  de la bibliothèque C universelle, qui fait partie de Windows 10 et 11. L'essai
  sur un PC sans Visual C++ installé reste à faire pour le dire prouvé.
- **ARM64.** Le code ne dépend pas de l'architecture, mais seul l'installeur
  x64 est fabriqué et testé.
- **Intégration continue.** Les actions GitHub sont désignées par l'empreinte de
  leur commit (`propriétaire/dépôt@<40 hexadécimaux> # vX.Y.Z`, la dernière
  version de la même version majeure le 16 novembre 2026) ; un essai de
  `npm test` refuse toute autre forme. **Limite** : rien ne vérifie que le
  commentaire dit la bonne version de l'empreinte, et les empreintes vieillissent
  (pas de Dependabot tant que la personne ne l'a pas demandé : il ouvre des
  demandes de fusion à son nom) : les relever de nouveau à la main de temps en
  temps. L'artefact `shinku-installeur-x64` porte aussi `SHA256SUMS.txt` et
  `LICENCES-TIERS.txt`.
- **Licences.** La liste des licences des dépendances est fabriquée et vérifiée
  (voir « Licences des dépendances »). Reste : choisir la licence de Shinku
  (à la personne) et décider de ce que disent les licences de Microsoft de la
  section fixe.
- **Icône.** L'icône actuelle reprend le signe de l'interface
  (`scripts/icone.mjs`). Le logo définitif reste à choisir dans le canevas.
- **Identifiant.** `fr.shinku.app` nomme le dossier du cache de WebView2 : le
  changer après une publication laisserait l'ancien dossier derrière lui, et
  `--oublier` ne viserait plus que le nouveau (un essai du cœur lit
  `tauri.conf.json` et échoue si `forget::TAURI_IDENTIFIER` diffère).
- **Journal local : fait** (voir « Le Journal »). Il rassemble les grands vides,
  les coupures du Démarrage, la dernière partie du mode jeu et les
  désinstallations ; il n'a pas été essayé sur un PC ordinaire (l'ouverture du
  fichier des licences, les dates d'un vrai désinstalleur).
- **Mise à jour** : prévue par l'architecture, pas encore écrite.
