# Composants tiers embarqués dans l'interface

L'interface de Shinku embarque les composants ci-dessous. Le texte complet de
chaque licence est copié dans `public/licences/` : il part ainsi dans
l'application installée, à côté des fichiers de l'interface.

| Composant | Rôle | Titulaire des droits | Licence | Texte |
| --- | --- | --- | --- | --- |
| M PLUS 1 (variable) | Police du texte | The M+ FONTS Project Authors | SIL Open Font License 1.1 | `public/licences/m-plus-1.txt` |
| Orbitron 600 | Police du logotype | The Orbitron Project Authors | SIL Open Font License 1.1 | `public/licences/orbitron.txt` |
| lit-html | Rendu des écrans | Google LLC | BSD 3-Clause | `public/licences/lit-html.txt` |
| API JavaScript de Tauri | Dialogue avec la fenêtre | Tauri Apps Contributors | MIT ou Apache 2.0 | `public/licences/tauri.txt` |

Les polices viennent des paquets Fontsource (`@fontsource-variable/m-plus-1`
et `@fontsource/orbitron`). Elles sont embarquées telles quelles, sans
modification, et uniquement pour les jeux de caractères utiles : latin, latin
étendu, et les deux tranches japonaises qui contiennent 真 et 空.

Les images de l'installeur (`src-tauri/installeur/images/`) contiennent le
logotype « SHINKU 真空 », dessiné avec Orbitron et M PLUS 1, déjà cités
ci-dessus : ces polices n'y sont pas embarquées, seules des images de leurs
lettres le sont (`scripts/images-installeur.mjs`).

Quand une dépendance change de version, recopier son fichier de licence :

```sh
cp node_modules/@fontsource-variable/m-plus-1/LICENSE public/licences/m-plus-1.txt
cp node_modules/@fontsource/orbitron/LICENSE public/licences/orbitron.txt
cp node_modules/lit-html/LICENSE public/licences/lit-html.txt
cp node_modules/@tauri-apps/api/LICENSE-MIT public/licences/tauri.txt
```

Les dépendances Rust (Tauri, BLAKE3, serde et les autres) ont leurs propres
licences. Leur liste complète, avec le texte de chaque licence, celui des
composants ci-dessus et celui de ce qui vient avec l'installeur, est fabriquée
par `scripts/licences-tiers.mjs` : c'est le fichier `LICENCES-TIERS.txt` que
l'installeur dépose à côté de `shinku.exe`. Voir `docs/conception.md`, section
« Licences des dépendances ».
