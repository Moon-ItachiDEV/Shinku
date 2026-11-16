; Licences des composants tiers dans l'installeur NSIS de Shinku.
;
; Ce fichier ne definit que deux macros publiques, que les crochets de Tauri
; (habillage.nsh) appellent, sur le modele de helper.nsh :
;
;   ShinkuLicencesPostInstall    LICENCES-TIERS.txt est copie a cote de
;                                shinku.exe ;
;   ShinkuLicencesPreUninstall   le fichier part, avant que le modele de Tauri
;                                retire le dossier d'installation (qui ne se
;                                retire que vide).
;
; Dans chaque crochet, ces macros passent apres celles de l'agent du mode jeu et
; de l'assistant eleve.
;
; Le fichier n'est PAS une ressource de Tauri : une ressource manquante ferait
; echouer toute compilation Windows, y compris `cargo build` et `clippy`. Seul
; makensis a besoin du fichier, et un fichier absent le fait echouer a la
; compilation de l'installeur, pas avant.
;
; Chemin du fichier : meme principe que pour l'agent (voir agent.nsh).
; `${__FILEDIR__}` vaut le dossier de ce fichier, lu hors de toute macro. Le
; fichier est fabrique par `beforeBuildCommand` (scripts/licences-tiers.mjs) dans
; src-tauri/licences, a cote de installeur : un dossier plus haut. Le chemin peut
; etre impose de l'exterieur en definissant SHINKU_LICENCES_SOURCE avant
; l'inclusion.

!ifndef SHINKU_LICENCES_SOURCE
  !define SHINKU_LICENCES_SOURCE "${__FILEDIR__}\..\licences\LICENCES-TIERS.txt"
!endif

; Apres l'application : la liste des licences, a cote de shinku.exe.
!macro ShinkuLicencesPostInstall
  SetOutPath "$INSTDIR"
  File "${SHINKU_LICENCES_SOURCE}"
!macroend

; Avant de retirer quoi que ce soit : le fichier part tout de suite (le dossier
; d'installation qui le contient encore ne se retire pas). Une mise a jour par le
; desinstalleur le redepose ensuite.
!macro ShinkuLicencesPreUninstall
  Delete "$INSTDIR\LICENCES-TIERS.txt"
!macroend
