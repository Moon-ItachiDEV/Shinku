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

La page ne parle qu'à `shinku_core::backend::Backend`, au travers de vingt-quatre
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
  affichée.
- Les pages de résultats sont plafonnées côté cœur (`PAGE_LIMIT`), quelle que
  soit la taille demandée.

### Ce que la page a le droit de faire

- **Permissions Tauri au plus juste.** `capabilities/default.json` donne à la
  fenêtre ses vingt-quatre commandes et les cinq gestes de sa barre de titre. Rien
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
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, valeur `Shinku` | Lance l'agent avec Windows. La seule écriture de registre de Shinku. Présente seulement quand le mode jeu automatique est allumé. |
| `%LOCALAPPDATA%\fr.shinku.app\` | Le cache de WebView2 pour la fenêtre. |

Rien d'autre, et rien hors du PC. L'analyse ne modifie aucun fichier. Seul le
grand vide supprime, et seulement ce que décrit la section qui lui est
consacrée. Le mode jeu change des réglages en cours de partie (priorité de
processus, mode d'alimentation), jamais un fichier de la personne ; il remet
tout en place.

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

**Les racines et les descendants.** Un processus créé par un processus en
veilleuse hérite de sa priorité. La session note donc ses *racines* (`roots` :
numéro, date de création, et `goneMs` / `goneUptime`, l'instant où la racine a
disparu, à l'heure murale et à l'horloge monotone ; 512 au plus, la racine
morte depuis le plus longtemps part la première, jamais une vivante tant qu'il
reste une morte) : tout ce qui est en veilleuse
ou l'a été. La mort d'une racine est notée à chaque pas, avant tout retour
anticipé (veilleuse éteinte, partie relâchée, remise en cours), et à la reprise
de l'agent pour celles trouvées mortes (à cet instant). À la remise en place,
les descendants des racines (né après son parent, né après `opened`, et, si la
racine est morte, né dans les trente secondes qui suivent sa mort : au-delà,
c'est un numéro recyclé) trouvés en
priorité basse y sont remis aussi. Un descendant de la liste, trouvé déjà bas
parce qu'il a hérité, est noté « normale » comme état d'avant. Un descendant qui
est le jeu (ou un ancêtre du jeu) n'est jamais ralenti, et est remis « normale »
à chaque pas s'il l'a été ; une app en veilleuse qui a lancé le jeu retrouve son
état d'avant à chaque pas, y compris après un changement de jeu. Un relèvement
ou une remise qui échoue est retenté toutes les 30 secondes, vingt fois au plus ;
au-delà, l'entrée reste au journal pour la remise de fin de partie. Cela ne dépend pas du journal des processus en
veilleuse : un orphelin dont le parent a quitté le journal est toujours remis.

**Les dates de la session.** `opened` est l'ouverture de la partie : elle ne
change jamais, et c'est elle qui borne ce qui peut être remis (rien de plus
ancien). `openedUptime` est la valeur de l'horloge monotone à ce moment.
`since` est le début du jeu affiché : un changement de jeu en cours de partie le
déplace sans toucher à `opened` ni aux racines. Les dates de naissance des
processus sont à l'heure murale du moment : avant toute comparaison, `opened`
(et la date de mort d'une racine) est ramené au repère d'aujourd'hui avec
l'horloge monotone (`min(opened, maintenant − (uptime − openedUptime))`), et si
l'heure murale a reculé de plus d'une minute depuis l'ouverture, l'ordre « né
après son parent » n'est plus exigé. Les champs `openedUptime` et `goneUptime`
sont lisibles à partir d'un ancien `game.json` : s'ils manquent, `opened` vaut
`since` et les dates murales seules servent.

**L'horloge et le redémarrage.** Tous les délais (deux pas pour confirmer un
jeu, dix minutes d'arrière-plan, trente secondes de relevé, soixante secondes
avant de réessayer une écriture, les deux minutes de l'agent lancé par Windows)
se comptent avec l'horloge monotone (`GetTickCount64`), jamais avec l'heure
murale. **Windows a redémarré si, au démarrage de l'agent, l'horloge monotone
est plus petite que `openedUptime`** (elle ne fait que croître tant que le PC
reste allumé). Alors la partie est close, seul le mode d'alimentation est
remis, aucun processus n'est touché et aucun descendant n'est cherché (les
numéros ne désignent plus les mêmes). Sans `openedUptime` (ancien journal), on
suppose qu'il n'a pas redémarré : `inspect` et `apply` vérifient la date de
création, et au pire un processus est relevé à tort. Un saut de l'heure murale
n'est jamais pris pour un redémarrage.

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
- **Une mise à jour qui passe par le désinstalleur** garde la valeur `Run`
  quand Tauri la lance avec `/UPDATE` (le crochet envoie alors `--quitter`, pas
  `--retirer`). Le choix « désinstaller avant d'installer » de la page de
  réinstallation, lui, lance l'ancien désinstalleur sans `/UPDATE` : la valeur
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
  avec `/UPDATE` : la valeur `Run` doit survivre à la mise à jour). Le fichier de
  l'agent est supprimé tout de suite : Tauri retire le dossier d'installation
  avant le crochet suivant, et un dossier qui contient encore l'agent ne se
  retire pas. Sans `/REBOOTOK` : une suppression remise au redémarrage effacerait
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

Un troisième essai, `apps/shinku/scripts/essai-installeur-agent.ps1`, exerce les
crochets de l'installeur, que l'installation sur machine vierge ne touche pas
(pas encore d'agent). Il part de l'installation faite par la CI : réglages
d'essai avec le mode automatique allumé, valeur `Run\Shinku` posée (c'est
`shinku.exe` qui l'écrit, pas l'agent), agent lancé avec le jeton filtré. Il
relance l'installeur tout juste fabriqué en `/S` et exige : code 0,
`shinku-agent.exe` en place et identique à l'agent compilé, aucun agent en marche
(l'ancien est parti, rien ne l'a relancé), valeur `Run` toujours là. Il relance
alors l'agent, lance `uninstall.exe /S` (NSIS se recopie ailleurs et rend la main
tout de suite : l'essai attend la disparition du dossier) et exige : plus aucun
agent, plus de valeur `Run`, plus de dossier d'installation, alimentation
inchangée. La valeur `Run` est aussi effacée par le modèle de Tauri : son
absence prouve l'état final, pas lequel des deux l'a effacée. Shinku n'est plus
installé ensuite, ce qui ne gêne rien : l'étape suivante publie l'installeur.
Les réglages, le journal et la valeur `Run` d'origine sont remis comme pour
l'essai précédent. Il ne couvre pas un agent qui refuse de sortir (le
`taskkill` puis la reprise du journal) ni un agent en pleine partie : ces cas
ne sont couverts que par la lecture du code.

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
