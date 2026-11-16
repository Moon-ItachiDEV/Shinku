; Habillage de l'installeur de Shinku : la mecanique.
;
; Ce fichier est celui de `bundle.windows.nsis.installerHooks` (voir
; tauri.conf.json). Le modele NSIS de Tauri l'inclut AVANT les pages de
; l'assistant. Il ne contient donc que ce qui doit etre dit avant elles : les
; reglages de Modern UI, la re-declaration de cinq pages, les quatre hooks
; (qui appellent aussi ceux de l'agent du mode jeu, agent.nsh).
; Il reste en ASCII strict (tests/installer.test.ts le verifie), et les
; textes, en UTF-8, sont dans textes.nsh, inclus plus bas avec /CHARSET=UTF8 :
; ainsi les textes ne dependent pas de l'option -INPUTCHARSET que Tauri passe
; (ou non) a makensis.
;
; Ordre d'inclusion, dans le modele de Tauri :
;   1. habillage.nsh  (ici)   : reglages, textes, macros de page, hooks ;
;   2. les pages de l'assistant (MUI_PAGE_*), puis MUI_LANGUAGE "French" ;
;   3. francais.nsh           : les chaines de Tauri, les textes fixes par
;      commande et les fonctions (elles utilisent les variables $mui.* des
;      pages, qui n'existent qu'apres elles). Tauri en fait une copie avec un
;      BOM : ce fichier ne depend donc d'aucun chemin relatif et retrouve les
;      images par SHINKU_INSTALLEUR.

; Le dossier de ce fichier : c'est la que sont les images.
!define SHINKU_INSTALLEUR "${__FILEDIR__}"

; --- Couleurs et images (Modern UI) ---

; Fond Brume sur Bienvenue, Termine et le bandeau d'en-tete ; encre pour le texte.
; Ces deux couleurs sont aussi celles des images (scripts/images-installeur.mjs).
!define MUI_BGCOLOR "F4F1EE"
!define MUI_TEXTCOLOR "1F1A26"

; Le signe (petit trou noir) est pose au bord droit de l'en-tete. Les images
; sont affichees a leur taille reelle, jamais etirees : leur fond est celui de
; la page, aucune couture ne se voit. La bonne taille est choisie selon l'ecran
; (ShinkuGuiInit, dans francais.nsh).
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP_STRETCH "NoStretchNoCropNoAlign"
!define MUI_WELCOMEFINISHPAGE_BITMAP_STRETCH "NoStretchNoCropNoAlign"
!define MUI_CUSTOMFUNCTION_GUIINIT ShinkuGuiInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.ShinkuGuiInit

; Le raccourci sur le bureau n'est pas coche par defaut ("Lancer Shinku" l'est).
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

; Tous les textes : voir textes.nsh.
!include /CHARSET=UTF8 "${SHINKU_INSTALLEUR}\textes.nsh"

; --- Pages re-declarees ---
;
; Pour habiller ces cinq pages (placer les controles, colorer la barre de
; progression, mettre "Installer" sur le bouton de la page Dossier), il faut
; accrocher une fonction a leur affichage avec
; MUI_PAGE_CUSTOMFUNCTION_SHOW, qui doit etre definie juste avant la page.
; Le modele de Tauri insere lui-meme ces pages, et on ne veut pas le
; bifurquer : on redefinit donc les macros MUI_PAGE_WELCOME,
; MUI_PAGE_DIRECTORY, MUI_PAGE_INSTFILES, MUI_PAGE_FINISH et
; MUI_UNPAGE_INSTFILES. Chaque copie reprend a l'identique le corps de la macro
; d'origine (Modern UI 2, NSIS 3.09 a 3.13 : Pages/Welcome.nsh, Directory.nsh,
; InstallFiles.nsh et Finish.nsh), precedee de la seule ligne
; `!define MUI_PAGE_CUSTOMFUNCTION_SHOW`.
;
; Le risque : si une version de Modern UI change le corps d'une de ces
; macros, notre copie reste l'ancienne. Ce qui casserait alors, a la
; compilation et bruyamment : un !insertmacro sur une macro qui n'existe plus
; (MUI_PAGEDECLARATION_*, MUI_PAGE_INIT), ou un !define en double. Une
; difference plus discrete (un controle ajoute a une page, par exemple) ne
; se verrait que sur les captures de l'essai de l'installeur. A chaque
; changement de version de NSIS, comparer ces corps avec les originaux.

!macroundef MUI_PAGE_WELCOME
!macro MUI_PAGE_WELCOME
  !verbose push
  !verbose ${MUI_VERBOSE}
  !define MUI_PAGE_CUSTOMFUNCTION_SHOW ShinkuWelcomeShow
  !insertmacro MUI_PAGE_INIT
  !insertmacro MUI_PAGEDECLARATION_WELCOME
  !verbose pop
!macroend

; makensis ne met le texte "Installer" que sur la page qui precede
; immediatement la page d'installation. Dans le modele de Tauri, c'est la page
; Menu Demarrer, toujours sautee : la page Dossier afficherait "Suivant".
; ShinkuDirectoryShow pose donc elle-meme le texte d'installation.
!macroundef MUI_PAGE_DIRECTORY
!macro MUI_PAGE_DIRECTORY
  !verbose push
  !verbose ${MUI_VERBOSE}
  !define MUI_PAGE_CUSTOMFUNCTION_SHOW ShinkuDirectoryShow
  !insertmacro MUI_PAGE_INIT
  !insertmacro MUI_PAGEDECLARATION_DIRECTORY
  !verbose pop
!macroend

!macroundef MUI_PAGE_INSTFILES
!macro MUI_PAGE_INSTFILES
  !verbose push
  !verbose ${MUI_VERBOSE}
  !define MUI_PAGE_CUSTOMFUNCTION_SHOW ShinkuInstFilesShow
  !insertmacro MUI_PAGE_INIT
  !insertmacro MUI_PAGEDECLARATION_INSTFILES
  !verbose pop
!macroend

!macroundef MUI_PAGE_FINISH
!macro MUI_PAGE_FINISH
  !verbose push
  !verbose ${MUI_VERBOSE}
  !define MUI_PAGE_CUSTOMFUNCTION_SHOW ShinkuFinishShow
  !insertmacro MUI_PAGE_INIT
  !insertmacro MUI_PAGEDECLARATION_FINISH
  !verbose pop
!macroend

!macroundef MUI_UNPAGE_INSTFILES
!macro MUI_UNPAGE_INSTFILES
  !verbose push
  !verbose ${MUI_VERBOSE}
  !define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ShinkuInstFilesShow
  !insertmacro MUI_UNPAGE_INIT
  !insertmacro MUI_PAGEDECLARATION_INSTFILES
  !verbose pop
!macroend

; --- Hooks de Tauri ---

; Tauri n'accepte qu'un fichier de hooks et NSIS une seule definition par
; macro : les quatre hooks sont donc tous ici. Ce qui concerne l'agent du mode
; jeu (shinku-agent.exe, que Tauri n'embarque pas tout seul) est dans
; agent.nsh, sous forme de macros ShinkuAgent* ; chaque hook les appelle, puis
; fait ce qui est propre a l'habillage.
!include "${SHINKU_INSTALLEUR}\agent.nsh"

; Avant toute copie : l'ancien agent, s'il tourne, verrouille son fichier.
!macro NSIS_HOOK_PREINSTALL
  !insertmacro ShinkuAgentPreInstall
!macroend

; L'agent est depose a cote de shinku.exe. Puis : le modele de Tauri garde la
; page d'installation affichee jusqu'a un clic (MUI_FINISHPAGE_NOAUTOCLOSE,
; pour lire la liste des details en cas de probleme). Ici la liste est
; masquee : la page passe seule a la page Termine. Ce hook n'est atteint que si
; la copie de Shinku a reussi ; en cas d'echec, la page reste affichee avec
; l'en-tete "Installation interrompue". Seul cas ou la page passe sans que tout
; soit en place : la copie de l'agent echoue et la personne choisit "Ignorer"
; dans le message de NSIS ; Shinku marche alors sans mode jeu automatique.
!macro NSIS_HOOK_POSTINSTALL
  !insertmacro ShinkuAgentPostInstall
  SetAutoClose true
!macroend

; Avant de retirer quoi que ce soit : l'agent remet tout en place et son
; fichier part.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro ShinkuAgentPreUninstall
!macroend

; Shinku range ses reglages dans %APPDATA%\Shinku (crates/shinku-core/src/settings.rs,
; APP_DIR) et les journaux du mode jeu et du Demarrage dans
; %LOCALAPPDATA%\Shinku (game/store.rs, startup/store.rs), pas sous
; l'identifiant du paquet : la case "Supprimer aussi mes reglages" de Tauri ne
; les supprime donc pas. Ce hook, place par le modele a la fin de la section
; Uninstall, le fait a sa place : seulement si la case est cochee et hors mise
; a jour. $APPDATA et $LOCALAPPDATA sont ceux de l'utilisateur
; (SetShellVarContext current, comme le fait Tauri pour ses propres dossiers)
; et non ceux de "tous les utilisateurs", que l'installation pour toute la
; machine selectionne par defaut. L'agent passe avant ce bloc : il lit encore
; ses fichiers pour tout remettre en place.
;
; Pas de "RMDir /r" : il suit les jonctions, et le desinstalleur tourne avec
; les droits d'administrateur sur des dossiers que n'importe quel programme de
; l'utilisateur peut modifier. On ne touche pas a un dossier s'il est un point
; d'analyse (jonction ou lien), on supprime un par un les fichiers que Shinku
; y ecrit, puis "RMDir" sans /r, qui ne retire le dossier que s'il est vide.
; Tout nouveau fichier ecrit par Shinku dans ces dossiers doit etre ajoute a
; ces listes (aujourd'hui : settings.rs, clean/journal.rs, game/store.rs et
; startup/store.rs, avec leurs fichiers temporaires ".tmp" de write_replacing).
!macro ShinkuDeleteSettings
  SetShellVarContext current
  ${If} $APPDATA != ""
    ; -1 : le dossier n'existe pas. Bit 0x400 : point d'analyse.
    System::Call 'kernel32::GetFileAttributesW(w "$APPDATA\Shinku") i .r0'
    ${If} $0 <> -1
      IntOp $0 $0 & 0x400
      ${If} $0 = 0
        Delete "$APPDATA\Shinku\settings.json"
        Delete "$APPDATA\Shinku\settings.illisible.json"
        Delete "$APPDATA\Shinku\journal.json"
        Delete "$APPDATA\Shinku\settings.json.tmp"
        Delete "$APPDATA\Shinku\journal.json.tmp"
        RMDir "$APPDATA\Shinku"
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $LOCALAPPDATA != ""
    System::Call 'kernel32::GetFileAttributesW(w "$LOCALAPPDATA\Shinku") i .r0'
    ${If} $0 <> -1
      IntOp $0 $0 & 0x400
      ${If} $0 = 0
        Delete "$LOCALAPPDATA\Shinku\game.json"
        Delete "$LOCALAPPDATA\Shinku\game.json.tmp"
        Delete "$LOCALAPPDATA\Shinku\startup.json"
        Delete "$LOCALAPPDATA\Shinku\startup.json.tmp"
        RMDir "$LOCALAPPDATA\Shinku"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro ShinkuAgentPostUninstall
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    !insertmacro ShinkuDeleteSettings
  ${EndIf}
!macroend
