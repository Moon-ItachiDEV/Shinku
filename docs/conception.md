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
| `shinku-agent.exe` | Le mode jeu, sans fenêtre (voir plus bas). Plus tard, l'analyse planifiée. | Ceux de l'utilisateur ; il refuse de tourner en administrateur. |

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

La page ne parle qu'à `shinku_core::backend::Backend`, au travers de vingt-six
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
  de registre, ni commande ne viennent de la page.
- Les pages de résultats sont plafonnées côté cœur (`PAGE_LIMIT`), quelle que
  soit la taille demandée.

### Ce que la page a le droit de faire

- **Permissions Tauri au plus juste.** `capabilities/default.json` donne à la
  fenêtre ses vingt-six commandes et les cinq gestes de sa barre de titre. Rien
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
| `%APPDATA%\Shinku\journal.json` | Les grands vides : dates et totaux, aucun chemin. |
| `%LOCALAPPDATA%\Shinku\game.json` | Le journal du mode jeu : la partie en cours, le bilan de la dernière, les jeux reconnus. Écrit par l'agent seulement. |
| `%LOCALAPPDATA%\Shinku\startup.json` | Ce que le module Démarrage a coupé (au plus 200 lignes : identifiant, nom, dates, `FILETIME` écrit). Écrit par la fenêtre seulement. |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, valeur `Shinku` | Lance l'agent avec Windows. La seule écriture que Shinku fasse dans une clé `Run`. Présente seulement quand le mode jeu automatique est allumé. |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\{Run, Run32, StartupFolder}` | Une valeur de douze octets par entrée de démarrage du compte que la personne coupe ou remet (voir « Démarrage »). Le même réglage que le Gestionnaire des tâches. Jamais sous `HKLM`. |
| `%LOCALAPPDATA%\fr.shinku.app\` | Le cache de WebView2 pour la fenêtre. |

Rien d'autre, et rien hors du PC. Shinku n'écrit jamais sous `HKLM` (le lot du
module Démarrage qui réglera les entrées du PC passera par l'assistant élevé,
voir plus bas). L'analyse ne modifie aucun fichier. Seul le
grand vide supprime, et seulement ce que décrit la section qui lui est
consacrée. Le mode jeu change des réglages en cours de partie (priorité de
processus, mode d'alimentation), jamais un fichier de la personne ; il remet
tout en place. Le module Démarrage ne change que des approbations de démarrage
du compte, et ne supprime aucune entrée.

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
nettoyer ne compte pas. Seul le grand vide supprime pour l'instant : les
boutons des modules restent inactifs.

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
  (un compte standard élevé avec les identifiants d'un administrateur) vise le
  profil de l'administrateur : `--retirer` ne trouve pas le `game.json` de la
  personne et ne remet rien. La valeur `Run` qui resterait pointe sur un
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
`approval.rs`, `host.rs`, `store.rs`, `view.rs`, `desk.rs`) et, pour Windows,
`platform/windows/startup.rs`.

Les règles du produit :

- rien n'est modifié sans un geste de la personne, et tout changement se défait ;
- **pas de chiffre inventé** : aucun « impact », aucune durée de démarrage ;
- **aucune phrase ne promet plus que ce que Shinku lit**. Shinku ne voit ni les
  apps du Microsoft Store, ni les tâches planifiées, ni les services : on dit
  « apps inscrites au démarrage », jamais « rien ne se lance » ni « plus rien » ;
- Shinku ne supprime aucune entrée : il ne fait que l'approuver ou la
  désapprouver, là où Windows le fait ;
- aucune écriture sous `HKLM`.

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
- **Les entrées du PC sont en lecture seule** dans ce lot : il faut l'accord
  d'un administrateur. La page ouvre la page de Windows.
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

### Le lot suivant : régler les entrées du PC

Régler une entrée de `HKLM` demande un administrateur : ce sera le travail de
l'assistant élevé `shinku-helper.exe`, relu à part. Ses exigences sont déjà
arrêtées :

- **il ne lit que des noms** : ni variables, ni fichiers, ni ressources de
  version dans le processus élevé. Tout ce qui demande d'ouvrir un fichier ou de
  développer une variable se fait dans `shinku.exe`, sans élévation ;
- **il ne se lance que depuis un dossier inscriptible par les seuls
  administrateurs** (un assistant élevé qu'un programme du compte pourrait
  remplacer serait une porte ouverte) ;
- **il dit lui-même, une fois élevé, quelle entrée il va changer** : ce que la
  personne approuve dans la fenêtre de Windows est ce que l'assistant affiche,
  pas ce que la page prétend.

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

Les crochets sont répartis en deux fichiers, parce que NSIS n'accepte qu'une
définition par macro et qu'un autre fichier de crochets doit pouvoir cohabiter.
`src-tauri/installer-hooks.nsh` (`bundle.windows.nsis.installerHooks`) ne garde
que l'inclusion et quatre macros `NSIS_HOOK_*` d'une ligne. Tout le reste est
dans `src-tauri/installeur/agent.nsh`, qui ne dépend que de `LogicLib.nsh` et
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
ensuite, ce qui ne gêne rien : l'étape suivante publie l'installeur. Les
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
  fait l'essai de la fenêtre, et piloter sa page. Depuis le grand vide, il
  peut ainsi faire supprimer ce que Shinku juge inutile, ce qu'il pouvait
  déjà faire lui-même avec les mêmes droits ; il n'y gagne aucune élévation.
  À réexaminer avec le helper.
- **Recette Démarrage à la main.** Couper puis rouvrir une session, sur un vrai
  Windows 11, pour une entrée de chaque place du compte, dans les deux sens
  (voir « Démarrage »). Aucun essai automatique ne l'a faite.
- **Signer l'agent.** L'installeur embarque `shinku-agent.exe` par un crochet
  NSIS, pas comme une ressource ou un binaire externe de Tauri : rien ne dit que
  Tauri le signera avec `shinku.exe` (à vérifier), il faudra donc le signer
  avant de fabriquer l'installeur. Un agent non signé, qui change la
  priorité d'autres programmes, est exactement ce qu'un antivirus regarde de
  près.
- **Helper et agent.** Leur bibliothèque d'exécution C++ est liée statiquement
  (`build.rs`), comme celle de `shinku.exe` ; à vérifier sur un PC sans
  Visual C++ installé.
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
