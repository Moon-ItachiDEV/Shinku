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
; agent.nsh, sous forme de macros ShinkuAgent* ; ce qui concerne l'assistant
; eleve du Demarrage (shinku-helper.exe) est dans helper.nsh, sous forme de
; macros ShinkuHelper* ; ce qui concerne "Supprimer aussi mes reglages"
; (shinku.exe --oublier, lance avec les droits ordinaires) est dans oublier.nsh,
; sous forme de macros ShinkuOublier* ; chaque hook les appelle, puis fait ce qui
; est propre a l'habillage. L'agent passe toujours en premier (sauf devant la
; fermeture de la fenetre, a la desinstallation) : il lit encore ses fichiers,
; que la suite peut supprimer.
!include "${SHINKU_INSTALLEUR}\agent.nsh"
!include "${SHINKU_INSTALLEUR}\helper.nsh"
!include "${SHINKU_INSTALLEUR}\oublier.nsh"

; Avant toute copie : l'ancien agent, s'il tourne, verrouille son fichier.
!macro NSIS_HOOK_PREINSTALL
  !insertmacro ShinkuAgentPreInstall
  !insertmacro ShinkuHelperPreInstall
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
  !insertmacro ShinkuHelperPostInstall
  SetAutoClose true
!macroend

; Avant de retirer quoi que ce soit, dans cet ordre :
;   1. la fenetre de Shinku est fermee, par la macro du modele de Tauri
;      (CheckIfAppIsRunning : la meme question "Shinku est ouvert" que la
;      personne aurait vue un instant plus tard, et que le modele rejoue ensuite
;      sans plus rien trouver). Cela ferme aussi le trou d'une fenetre qui
;      relancerait l'agent entre les deux crochets, et fait que `--oublier` ne
;      se heurte pas a une fenetre qui reecrit ses reglages ;
;   2. l'agent remet tout en place et son fichier part (il lit encore ses
;      fichiers) ;
;   3. l'assistant eleve est arrete et son fichier part ;
;   4. si la case "Supprimer aussi mes reglages" est cochee (hors mise a jour) :
;      `shinku.exe --oublier` est lance avec les droits ordinaires de la
;      personne, et le desinstalleur attend qu'il ait fini (oublier.nsh).
; Le desinstalleur n'efface plus aucun fichier dans un profil : ni ici, ni dans
; le bloc "Delete app data" du modele (ses deux "RmDir /r" sont rendus sans
; effet, la variable de Tauri etant remise a 0 par oublier.nsh).
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !insertmacro ShinkuAgentPreUninstall
  !insertmacro ShinkuHelperPreUninstall
  !insertmacro ShinkuOublierPreUninstall
!macroend

; Apres le bloc "Delete app data" du modele, devenu sans effet : le menage du
; registre que ce bloc faisait, refait par oublier.nsh quand la case etait
; cochee (hors mise a jour).
!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro ShinkuAgentPostUninstall
  !insertmacro ShinkuHelperPostUninstall
  !insertmacro ShinkuOublierPostUninstall
!macroend
