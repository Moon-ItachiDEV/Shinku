# Shinku 真空

Shinku fait de la place sur un PC Windows 11 sans rien supprimer dans le dos de
son utilisateur. Il cherche ce qui encombre le disque, montre ce qu'il a trouvé
et attend un accord avant d'agir. Il est gratuit, sans compte, sans publicité
et sans télémétrie : rien ne quitte le PC.

## Où en est le projet

Version 0.1, en cours de développement. Rien n'est encore publié.

Ce qui fonctionne :

- le moteur d'analyse, en lecture seule : fichiers inutiles (42 règles),
  doublons, gros fichiers, état de la corbeille ;
- le grand vide : après confirmation, il vide la corbeille, supprime les
  fichiers inutiles et envoie à la corbeille les copies en double que la
  personne a cochées. Un exemplaire de chaque doublon est toujours gardé :
  le plus ancien, ou celui que la personne choisit sur l'écran Doublons,
  fichier par fichier ou pour tout un dossier ;
- le mode jeu : un agent sans fenêtre (`shinku-agent.exe`) reconnaît un jeu
  quand il passe au premier plan, passe l'alimentation en « Performances
  élevées » (jamais sur batterie), met en veilleuse les apps d'arrière-plan
  choisies, puis remet tout en place à la fin de la partie, y compris après un
  plantage ou une coupure de courant. Éteint par défaut ; Shinku ne lance
  jamais un jeu ;
- le module Démarrage : la liste des apps inscrites au démarrage de Windows
  (six places : les valeurs `Run` du compte et du PC, en vues 64 et 32 bits, et
  les dossiers Démarrage du compte et commun), et un interrupteur par app du
  **compte** qui la coupe ou la remet **au même endroit que le Gestionnaire des
  tâches** : l'app reste installée, rien n'est supprimé. Les entrées du PC sont
  montrées en lecture seule, avec un bouton qui ouvre la page « Apps de
  démarrage » de Windows. Rien n'y est lu hors d'un lecteur fixe (ni réseau, ni
  amovible), et une liste que Windows n'a pas laissé lire en entier est dite
  incomplète au lieu de passer pour complète. Voir `docs/conception.md`, section
  « Démarrage » ;
- l'interface complète, en deux modes d'affichage (Crépuscule et Brume),
  branchée sur le moteur ;
- la fenêtre de l'application (`shinku.exe`) et son installeur pour Windows,
  qui embarque l'agent.

Ce qui n'existe pas encore :

- les actions des modules (supprimer un seul doublon, un gros fichier…).
  Leurs boutons sont visibles mais inactifs, et l'interface le dit ;
- l'assistant élevé (`shinku-helper.exe`), dont il n'existe que le squelette :
  c'est lui qui réglera les entrées de démarrage du PC (tous les comptes), qui
  demandent un administrateur ;
- l'analyse planifiée par l'agent : pour l'instant, il ne fait que le mode jeu ;
- les modules Désinstallation et Confidentialité ;
- la preuve que Windows respecte, à l'ouverture d'une session, ce que le module
  Démarrage écrit : elle ne se fait qu'à la main, sur un vrai Windows 11 (voir
  `docs/conception.md`).

## Plan du dépôt

| Dossier | Contenu |
| --- | --- |
| `crates/shinku-core` | Le cœur : règles, liste d'interdits, moteur d'analyse, mode jeu (`game/`), module Démarrage (`startup/`), réglages, et le « guichet » que la fenêtre appelle. Tout ce qui décide est ici, et se teste sans fenêtre. |
| `crates/shinku-helper` | L'assistant élevé. Il n'accepte que des verbes fixes, jamais un chemin. Squelette. |
| `crates/shinku-agent` | L'agent du mode jeu : sans fenêtre, lancé par Shinku ou avec Windows. Il ne décide rien : il appelle le cœur toutes les deux secondes. |
| `apps/shinku` | L'interface : TypeScript, lit-html, Vite. |
| `apps/shinku/src-tauri` | La fenêtre : Tauri 2 sur WebView2. Elle affiche l'interface et transmet ses demandes au cœur. |
| `docs` | Les notes de conception. |

Les identifiants du code sont en anglais ; les commentaires, la documentation
et les messages des tests sont en français.

## Préparer sa machine

- [Rust](https://rustup.rs) : la version est fixée par `rust-toolchain.toml`,
  rustup l'installe tout seul.
- [Node.js](https://nodejs.org) 22 ou plus récent.
- Pour compiler la fenêtre : Windows 11 et les outils de compilation C++ de
  Visual Studio (« Développement Desktop en C++ »).

Le cœur et l'interface se développent aussi sous Linux ou macOS. Seule la
fenêtre a besoin de Windows.

## Commandes

À la racine du dépôt :

```sh
cargo test                     # les tests du cœur, du helper et de l'agent
cargo clippy --all-targets     # le lint
cargo fmt --all                # la mise en forme

# Une vraie analyse de ce PC, dans le terminal. Lecture seule.
cargo run --release -p shinku-core --example analyse
cargo run --release -p shinku-core --example analyse -- --json > rapport.json
```

Dans `apps/shinku` :

```sh
npm install        # une fois
npm run demo       # l'interface dans le navigateur, sur des données factices
npm run check      # les types
npm test           # les tests de l'interface, du contrat et du câblage
npm run tauri dev  # Windows : la vraie fenêtre, rechargée à chaque changement
npm run tauri build  # Windows : shinku.exe, shinku-agent.exe et l'installeur
```

L'installeur est écrit dans `target/release/bundle/nsis/`. Son habillage
(images, textes) est dans `apps/shinku/src-tauri/installeur/` ; voir
`docs/conception.md`. Il embarque `shinku-agent.exe`, compilé juste avant par
`beforeBuildCommand` (`target/release/`) : un `tauri build` avec `--target` ne
le trouverait pas. Les crochets qui le déposent et le font sortir sont dans
`apps/shinku/src-tauri/installeur/agent.nsh`, appelés depuis `habillage.nsh`.

La démo accepte quelques réglages dans son adresse : `?theme=light` ou
`?theme=dark`, `?speed=20` (analyse accélérée), `?bin=empty` (corbeille vide),
`?issue=unreadable` ou `?issue=partial` (réglages illisibles au démarrage).
Pour le mode jeu : `?game=off|idle|playing|released|pending` (où en est la
partie), `?agent=missing|stopped`, `?power=unavailable|unsuited|battery` et
`?startup=disabled`. La démo n'a pas de vrai agent : tout y est simulé en
mémoire.

`npm run build` puis `npm run preview` servent l'interface compilée sous la
même politique de sécurité de contenu que la fenêtre : c'est le moyen de
vérifier, sans Windows, qu'aucun style ni script ne sera refusé.

## Les tests

- Le cœur se teste sur de petits PC fabriqués dans un dossier temporaire :
  aucun test ne lit ni ne supprime dans les vrais dossiers de la machine, et
  leur corbeille est un simple dossier. La vraie corbeille de Windows n'est
  essayée que sur demande (`SHINKU_ESSAI_CORBEILLE=1`), sur des fichiers
  fabriqués pour l'occasion.
- Le contrat entre le cœur et l'interface est un fichier de référence,
  `apps/shinku/tests/contract/contract.json`. Le cœur vérifie qu'il produit
  bien ces formes de JSON, l'interface vérifie qu'elle attend les mêmes (les
  types, et les réponses de la démo). Après
  un changement voulu du JSON :

  ```sh
  SHINKU_UPDATE_CONTRACT=1 cargo test -p shinku-core --test ui_contract
  ```

- Le module Démarrage se teste sur une machine fabriquée (`FakeStartupHost`),
  et son hôte Windows sur un vrai registre : ses tests écrivent sous une clé
  d'essai de `HKCU` (`Software\Shinku-Essai-<aléa>`, supprimée à la fin), jamais
  sous les vraies clés `Run` ni `StartupApproved`, et n'ouvrent jamais `HKLM`
  en écriture.
- Le câblage de la fenêtre (commandes, permissions, politique de sécurité) est
  vérifié à la lecture des fichiers par `apps/shinku/tests/wiring.test.ts` :
  une commande oubliée se voit sans Windows.
- La fenêtre elle-même est essayée sur un vrai Windows, une fois installée :
  `apps/shinku/scripts/essai-fenetre.mjs` lance `shinku.exe`, lit ce qu'il
  affiche et clique comme le ferait quelqu'un (analyse, résultats et cases
  cochées d'office, confirmation du grand vide, chaque module, réglages,
  fermeture). Sur un PC d'essai,
  depuis `apps/shinku` :

  ```sh
  node scripts/essai-fenetre.mjs --programme "C:\Program Files\Shinku\shinku.exe" --captures captures
  ```

  Il mène une vraie analyse du disque et change un instant le mode
  d'affichage enregistré. Il annule la confirmation du grand vide : rien
  n'est supprimé. L'option `--grand-vide` le fait aller au bout, pour de
  bon : elle est réservée à une machine jetable. Il allume aussi un instant
  le mode jeu automatique et vérifie que l'agent démarre puis sort.
- Le mode jeu est essayé de bout en bout par
  `apps/shinku/scripts/essai-mode-jeu.ps1`, sur un vrai Windows : l'agent
  installé reconnaît un faux jeu (une petite fenêtre compilée à l'instant) au
  premier plan, met un `ping` en veilleuse, change le mode d'alimentation, puis
  remet tout en place à la fermeture du jeu. Une sonde dit d'abord si un compte
  ordinaire peut changer le mode d'alimentation : sur un Windows Server (les
  machines de GitHub), non, et l'essai exige alors `refused` de l'agent, en
  écrivant que le passage en « Performances élevées » n'est pas prouvé ici. Il
  remet à la fin le mode d'alimentation et les réglages d'origine, et efface son
  dossier temporaire ; à réserver, lui aussi, à un PC d'essai. Depuis
  `apps/shinku` :

  ```sh
  powershell -File scripts/essai-mode-jeu.ps1 -Agent "C:\Program Files\Shinku\shinku-agent.exe"
  ```

- Les crochets de l'installeur sont essayés par
  `apps/shinku/scripts/essai-installeur-agent.ps1` : mise à jour par-dessus
  l'installation, puis désinstallation, avec un agent qui tourne. Plus aucun
  agent, la valeur de démarrage (conservée à la mise à jour, effacée à la
  désinstallation), le dossier d'installation et l'alimentation sont vérifiés.
  Shinku n'est plus installé ensuite : machine jetable seulement. Depuis
  `apps/shinku` :

  ```sh
  powershell -File scripts/essai-installeur-agent.ps1 -Installeur "..\..\target\release\bundle\nsis\Shinku_0.1.0_x64-setup.exe"
  ```

- L'intégration continue (`.github/workflows/ci.yml`) rejoue le tout sous
  Linux, passe le code propre à Windows au lint, puis, sur un vrai Windows,
  lance les tests du cœur, fabrique l'installeur, le parcourt page par page
  (`apps/shinku/scripts/essai-installeur.ps1`), l'installe, essaie la fenêtre,
  puis le mode jeu, puis la mise à jour et la désinstallation.
  L'installeur n'est publié que si ces essais passent.

## Pour aller plus loin

- `docs/conception.md` : les choix techniques, leurs raisons, et ce qui reste
  à faire avant une version publique.
- `apps/shinku/LICENCES-TIERCES.md` : les composants tiers embarqués dans
  l'interface.

## Licence

Shinku sera distribué gratuitement. Le texte de sa licence n'est pas encore
choisi : tant qu'il ne l'est pas, tous les droits sont réservés.
