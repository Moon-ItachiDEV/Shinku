; Fichier de langue de Shinku pour l'installeur (« French »), celui de
; `bundle.windows.nsis.customLanguageFiles`. Tauri en fait une copie avec un
; BOM UTF-8 et l'inclut APRÈS les pages et après MUI_LANGUAGE : les variables
; $mui.* des pages existent donc ici, et les commandes de texte sans /LANG
; (MiscButtonText, DirText…) s'appliquent à la langue chargée juste avant,
; le français. Comme la copie n'est plus à côté des images, rien ici ne dépend
; d'un chemin relatif : on passe par SHINKU_INSTALLEUR (habillage.nsh).
;
; Au sommaire : les chaînes de Tauri (le modèle ne les définit que pour ses
; langues, et « Shinku » est écrit en toutes lettres : ce fichier ne passe
; pas par le moteur de gabarit), les textes fixés par commande, la police, les
; fonctions d'habillage.

; --- Chaînes de Tauri (les 27 clés de languages/French.nsh) ---
LangString addOrReinstall ${LANG_FRENCH} "Réinstaller Shinku"
LangString alreadyInstalled ${LANG_FRENCH} "Shinku est déjà installé"
LangString alreadyInstalledLong ${LANG_FRENCH} "Shinku ${VERSION} est déjà sur ton PC. Choisis ce que tu veux faire, puis clique sur Suivant."
LangString appRunning ${LANG_FRENCH} "Shinku est ouvert. Ferme-le, puis réessaie."
LangString appRunningOkKill ${LANG_FRENCH} "Shinku est ouvert.$\nClique sur OK pour le fermer et continuer."
LangString chooseMaintenanceOption ${LANG_FRENCH} "Choisis ce que tu veux faire."
LangString chooseHowToInstall ${LANG_FRENCH} "Choisis comment installer Shinku."
LangString createDesktop ${LANG_FRENCH} "Créer un raccourci sur le bureau"
LangString dontUninstall ${LANG_FRENCH} "Ne pas désinstaller"
LangString dontUninstallDowngrade ${LANG_FRENCH} "Ne pas désinstaller (impossible ici : cet installeur ne revient pas à une version plus ancienne sans désinstaller)"
LangString failedToKillApp ${LANG_FRENCH} "Shinku n'a pas pu être fermé. Ferme-le, puis réessaie."
LangString installingWebview2 ${LANG_FRENCH} "Installation de WebView2…"
LangString newerVersionInstalled ${LANG_FRENCH} "Une version plus récente de Shinku est déjà installée. Revenir à une version plus ancienne n'est pas conseillé. Si tu y tiens, désinstalle d'abord la version actuelle. Choisis ce que tu veux faire, puis clique sur Suivant."
LangString older ${LANG_FRENCH} "plus ancienne"
LangString olderOrUnknownVersionInstalled ${LANG_FRENCH} "Une version $R4 de Shinku est déjà installée. Mieux vaut la désinstaller avant d'installer celle-ci. Choisis ce que tu veux faire, puis clique sur Suivant."
LangString silentDowngrades ${LANG_FRENCH} "Cet installeur ne revient pas à une version plus ancienne. L'installation silencieuse s'arrête : relance l'installeur normalement.$\n"
LangString unableToUninstall ${LANG_FRENCH} "Impossible de désinstaller Shinku."
LangString uninstallApp ${LANG_FRENCH} "Désinstaller Shinku"
LangString uninstallBeforeInstalling ${LANG_FRENCH} "Désinstaller d'abord la version installée"
LangString unknown ${LANG_FRENCH} "inconnue"
LangString webview2AbortError ${LANG_FRENCH} "WebView2 n'a pas pu être installé. Shinku en a besoin pour s'afficher. Relance l'installation."
LangString webview2DownloadError ${LANG_FRENCH} "Erreur : le téléchargement de WebView2 a échoué - $0"
LangString webview2DownloadSuccess ${LANG_FRENCH} "WebView2 est téléchargé."
LangString webview2Downloading ${LANG_FRENCH} "Téléchargement de WebView2…"
LangString webview2InstallError ${LANG_FRENCH} "Erreur : l'installation de WebView2 a échoué (code $1)"
LangString webview2InstallSuccess ${LANG_FRENCH} "WebView2 est installé."
LangString deleteAppData ${LANG_FRENCH} "Supprimer aussi mes réglages"

; --- Texte du désinstalleur (oublier.nsh) ---
; Montré quand `shinku.exe --oublier` n'a pas fini, avec /SD IDOK : un
; désinstalleur silencieux ou passif ne s'arrête pas dessus. Jamais de repli sur
; un effacement avec les droits d'administrateur. Pas de chaîne de Tauri : le
; modèle ne la connaît pas.
LangString shinkuOublierEchec ${LANG_FRENCH} "Shinku n'a pas pu supprimer tes réglages. Ils sont dans %APPDATA%\Shinku et %LOCALAPPDATA%\Shinku : tu peux les supprimer à la main."

; --- Textes fixés par commande ---
MiscButtonText "&Précédent" "&Suivant" "Annuler" "&Fermer"
InstallButtonText "&Installer"
UninstallButtonText "&Désinstaller"
DirText "Shinku sera installé dans ce dossier. Pour en choisir un autre, clique sur Parcourir." "Dossier d'installation" "P&arcourir…" "Choisis le dossier où installer Shinku :"
UninstallText "Shinku sera désinstallé de ce dossier. Tes fichiers personnels ne sont pas touchés." "Dossier de Shinku :"
FileErrorText "Impossible d'écrire ce fichier :$\r$\n$\r$\n$0$\r$\n$\r$\nClique sur Abandonner pour arrêter l'installation, sur Réessayer pour tenter à nouveau, ou sur Ignorer pour passer ce fichier." "Impossible d'écrire ce fichier :$\r$\n$\r$\n$0$\r$\n$\r$\nClique sur Réessayer pour tenter à nouveau, ou sur Annuler pour arrêter l'installation."

; --- Police et liste des détails ---
SetFont /LANG=${LANG_FRENCH} "Segoe UI" 9
; La liste des détails est masquée : la page d'installation ne montre que la barre.
ShowInstDetails nevershow
ShowUninstDetails nevershow

; --- Fonctions d'habillage ---

Var ShinkuTitleFont
Var ShinkuSignWidth
Var ShinkuSignHeight

; Choix des images selon l'échelle de l'écran (taille la plus proche, seuils au
; milieu : 108, 132, 156, 180 dpi), puis mise en place du signe de l'en-tête.
;
; Le bandeau de gauche : Modern UI l'a déjà extrait à 100 % dans
; $PLUGINSDIR\modern-wizard.bmp (sidebarImage de la configuration, qui sert de
; repli). On le réécrit, et la page Bienvenue ou Terminé le charge à son
; ouverture. Le signe : avec NoStretchNoCropNoAlign, Modern UI charge l'image
; sans la placer. On charge la bonne taille avec SetBrandingImage, puis on pose
; le contrôle 1046 nous-mêmes : bord droit de la fenêtre, haut à 0. On ne
; rappelle surtout pas MUI_LOADANDXALIGNIMAGE : à 150 % il décale l'image hors
; de la fenêtre.
;
; Une seule définition pour l'installeur et le désinstalleur : NSIS veut le
; préfixe « un. » devant les fonctions de ce dernier, d'où la macro. Le
; désinstalleur n'a pas de page Bienvenue ni Terminé : pas de bandeau.
!macro ShinkuScale UN SCALE SIGNWIDTH SIGNHEIGHT
  !if "${UN}" == ""
    File "/oname=$PLUGINSDIR\modern-wizard.bmp" "${SHINKU_INSTALLEUR}\images\bandeau-${SCALE}.bmp"
  !endif
  File "/oname=$PLUGINSDIR\modern-header.bmp" "${SHINKU_INSTALLEUR}\images\signe-${SCALE}.bmp"
  StrCpy $ShinkuSignWidth ${SIGNWIDTH}
  StrCpy $ShinkuSignHeight ${SIGNHEIGHT}
!macroend

!macro ShinkuGuiInit UN
  Function ${UN}ShinkuGuiInit
    !if "${UN}" == ""
      CreateFont $ShinkuTitleFont "Segoe UI Light" 17 300
    !endif
    InitPluginsDir
    ; Avant Windows 10 1607, l'appel échoue et laisse 0 : on reste à 100 %.
    StrCpy $0 0
    System::Call "user32::GetDpiForWindow(p $HWNDPARENT) i .r0"
    ${If} $0 < 108
      !insertmacro ShinkuScale "${UN}" 100 76 58
    ${ElseIf} $0 < 132
      !insertmacro ShinkuScale "${UN}" 125 95 73
    ${ElseIf} $0 < 156
      !insertmacro ShinkuScale "${UN}" 150 114 87
    ${ElseIf} $0 < 180
      !insertmacro ShinkuScale "${UN}" 175 133 102
    ${Else}
      !insertmacro ShinkuScale "${UN}" 200 152 116
    ${EndIf}
    SetBrandingImage /IMGID=1046 "$PLUGINSDIR\modern-header.bmp"
    ; Largeur utile de la fenêtre : le signe y est posé à droite, tout en haut.
    System::Call "*(i, i, i, i) p .r1"
    System::Call "user32::GetClientRect(p $HWNDPARENT, p r1)"
    System::Call "*$1(i, i, i .r2, i)"
    System::Free $1
    IntOp $2 $2 - $ShinkuSignWidth
    System::Call "user32::SetWindowPos(p $mui.Header.Image, p 0, i r2, i 0, i $ShinkuSignWidth, i $ShinkuSignHeight, i 0x14)"
  FunctionEnd
!macroend
!insertmacro ShinkuGuiInit ""
!insertmacro ShinkuGuiInit "un."

; Page d'installation et de désinstallation : barre de progression plate, pêche
; (F0A77E) sur une piste E6E0E4, sans bordure creuse. Les couleurs sont en
; 0x00BBGGRR. Retirer le thème est nécessaire : le thème de Windows ignore
; ces couleurs.
!macro ShinkuInstFilesShow UN
  Function ${UN}ShinkuInstFilesShow
    FindWindow $0 "#32770" "" $HWNDPARENT
    GetDlgItem $1 $0 1004
    System::Call 'uxtheme::SetWindowTheme(p r1, w " ", w " ")'
    SendMessage $1 0x409 0 0x007EA7F0
    SendMessage $1 0x2001 0 0x00E4E0E6
    ; Bordures creuses : WS_BORDER (style), WS_EX_STATICEDGE et WS_EX_CLIENTEDGE (style étendu).
    System::Call "user32::GetWindowLong(p r1, i -16) i .r2"
    IntOp $2 $2 & 0xFF7FFFFF
    System::Call "user32::SetWindowLong(p r1, i -16, i r2)"
    System::Call "user32::GetWindowLong(p r1, i -20) i .r2"
    IntOp $2 $2 & 0xFFFDFDFF
    System::Call "user32::SetWindowLong(p r1, i -20, i r2)"
    ; SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED
    System::Call "user32::SetWindowPos(p r1, p 0, i 0, i 0, i 0, i 0, i 0x37)"
  FunctionEnd
!macroend
!insertmacro ShinkuInstFilesShow ""
!insertmacro ShinkuInstFilesShow "un."

; Place un contrôle de la page : x, y, largeur et hauteur en unités de dialogue,
; converties en pixels avec MapDialogRect sur la fenêtre principale. Les
; contrôles des pages Bienvenue et Terminé sont placés par rapport à la fenêtre
; entière.
!macro ShinkuPlace CONTROL X Y WIDTH HEIGHT
  !define /math SHINKU_RIGHT ${X} + ${WIDTH}
  !define /math SHINKU_BOTTOM ${Y} + ${HEIGHT}
  System::Call "*(i ${X}, i ${Y}, i ${SHINKU_RIGHT}, i ${SHINKU_BOTTOM}) p .r0"
  System::Call "user32::MapDialogRect(p $HWNDPARENT, p r0)"
  System::Call "*$0(i .r1, i .r2, i .r3, i .r4)"
  System::Free $0
  IntOp $3 $3 - $1
  IntOp $4 $4 - $2
  System::Call "user32::SetWindowPos(p ${CONTROL}, p 0, i r1, i r2, i r3, i r4, i 0x14)"
  !undef SHINKU_RIGHT
  !undef SHINKU_BOTTOM
!macroend

; Page Dossier : le bouton de droite dit « Installer » (voir habillage.nsh :
; makensis ne le fait pas, la page Menu Démarrer du modèle étant sautée).
Function ShinkuDirectoryShow
  SendMessage $mui.Button.Next ${WM_SETTEXT} 0 "STR:$(^InstallBtn)"
FunctionEnd

Function ShinkuWelcomeShow
  SendMessage $mui.WelcomePage.Title ${WM_SETFONT} $ShinkuTitleFont 0
  SetCtlColors $mui.WelcomePage.Text 5B5566 ${MUI_BGCOLOR}
  !insertmacro ShinkuPlace $mui.WelcomePage.Title 122 50 198 24
  !insertmacro ShinkuPlace $mui.WelcomePage.Text 122 82 178 100
FunctionEnd

Function ShinkuFinishShow
  SendMessage $mui.FinishPage.Title ${WM_SETFONT} $ShinkuTitleFont 0
  SetCtlColors $mui.FinishPage.Text 5B5566 ${MUI_BGCOLOR}
  !insertmacro ShinkuPlace $mui.FinishPage.Title 122 38 198 24
  !insertmacro ShinkuPlace $mui.FinishPage.Text 122 70 178 40
  !insertmacro ShinkuPlace $mui.FinishPage.Run 122 112 178 12
  !insertmacro ShinkuPlace $mui.FinishPage.ShowReadme 122 130 178 12
FunctionEnd
