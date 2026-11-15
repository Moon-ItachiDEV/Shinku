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
- l'interface complète, en deux modes d'affichage (Crépuscule et Brume),
  branchée sur le moteur ;
- la fenêtre de l'application (`shinku.exe`) et son installeur pour Windows.

Ce qui n'existe pas encore :

- toute suppression. Les boutons d'action sont visibles mais inactifs, et
  l'interface le dit ;
- l'assistant élevé (`shinku-helper.exe`) et l'agent lancé avec Windows
  (`shinku-agent.exe`), dont il n'existe que le squelette ;
- les modules Démarrage, Mode jeu, Désinstallation et Confidentialité.

## Plan du dépôt

| Dossier | Contenu |
| --- | --- |
| `crates/shinku-core` | Le cœur : règles, liste d'interdits, moteur d'analyse, réglages, et le « guichet » que la fenêtre appelle. Tout ce qui décide est ici, et se teste sans fenêtre. |
| `crates/shinku-helper` | L'assistant élevé. Il n'accepte que des verbes fixes, jamais un chemin. Squelette. |
| `crates/shinku-agent` | L'agent sans fenêtre, lancé avec Windows. Squelette. |
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
npm run tauri build  # Windows : shinku.exe et son installeur
```

L'installeur est écrit dans `target/release/bundle/nsis/`.

La démo accepte quelques réglages dans son adresse : `?theme=light` ou
`?theme=dark`, `?speed=20` (analyse accélérée), `?bin=empty` (corbeille vide),
`?issue=unreadable` ou `?issue=partial` (réglages illisibles au démarrage).

`npm run build` puis `npm run preview` servent l'interface compilée sous la
même politique de sécurité de contenu que la fenêtre : c'est le moyen de
vérifier, sans Windows, qu'aucun style ni script ne sera refusé.

## Les tests

- Le cœur se teste sur de petits PC fabriqués dans un dossier temporaire :
  aucune analyse de test ne touche aux vrais dossiers de la machine.
- Le contrat entre le cœur et l'interface est un fichier de référence,
  `apps/shinku/tests/contract/contract.json`. Le cœur vérifie qu'il produit
  bien ces formes de JSON, l'interface vérifie qu'elle attend les mêmes. Après
  un changement voulu du JSON :

  ```sh
  SHINKU_UPDATE_CONTRACT=1 cargo test -p shinku-core --test ui_contract
  ```

- Le câblage de la fenêtre (commandes, permissions, politique de sécurité) est
  vérifié à la lecture des fichiers par `apps/shinku/tests/wiring.test.ts` :
  une commande oubliée se voit sans Windows.
- La fenêtre elle-même est essayée sur un vrai Windows, une fois installée :
  `apps/shinku/scripts/essai-fenetre.mjs` lance `shinku.exe`, lit ce qu'il
  affiche et clique comme le ferait quelqu'un (analyse, résultats et cases
  cochées d'office, chaque module, réglages, fermeture). Sur un PC d'essai,
  depuis `apps/shinku` :

  ```sh
  node scripts/essai-fenetre.mjs --programme "C:\Program Files\Shinku\shinku.exe" --captures captures
  ```

  Il mène une vraie analyse du disque et change un instant le mode
  d'affichage enregistré.
- L'intégration continue (`.github/workflows/ci.yml`) rejoue le tout sous
  Linux, passe le code propre à Windows au lint, puis, sur un vrai Windows,
  lance les tests du cœur, fabrique l'installeur, l'installe et essaie la
  fenêtre. L'installeur n'est publié que si cet essai passe.

## Pour aller plus loin

- `docs/conception.md` : les choix techniques, leurs raisons, et ce qui reste
  à faire avant une version publique.
- `apps/shinku/LICENCES-TIERCES.md` : les composants tiers embarqués dans
  l'interface.

## Licence

Shinku sera distribué gratuitement. Le texte de sa licence n'est pas encore
choisi : tant qu'il ne l'est pas, tous les droits sont réservés.
