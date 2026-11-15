; Crochets de l'installeur NSIS de Shinku (tauri.conf.json,
; bundle.windows.nsis.installerHooks). Ils font entrer et sortir l'agent du
; mode jeu, shinku-agent.exe, qui n'est pas le programme principal : Tauri ne
; l'embarque pas tout seul.
;
; Le fichier est inclus par installer.nsi, un script généré ailleurs : NSIS
; résout donc les chemins relatifs par rapport à ce script, pas à ce fichier.
; `${__FILEDIR__}`, le dossier de ce fichier-ci, ne vaut ce qu'on attend qu'à
; l'endroit où le fichier est lu, d'où le `!define` ci-dessous, hors de toute
; macro (un `${__FILEDIR__}` écrit dans une macro serait relu à l'endroit où la
; macro est insérée). Ce dossier est apps/shinku/src-tauri : l'agent compilé
; par `beforeBuildCommand` (cargo build --release) est à la racine du dépôt,
; dans target/release.

!include LogicLib.nsh

!define SHINKU_AGENT_SOURCE "${__FILEDIR__}\..\..\..\target\release\shinku-agent.exe"

; Demande à l'agent de sortir avec l'argument donné (--quitter : il remet tout
; en place puis s'arrête ; --retirer : il remet tout en place, efface sa valeur
; de démarrage puis s'arrête), et attend sa fin. Un agent qui ne répond pas est
; arrêté de force, en silence : la reprise au prochain démarrage remettra en
; place ce qu'il aurait dû remettre.
!macro ShinkuAskAgentToLeave argument
  ${If} ${FileExists} "$INSTDIR\shinku-agent.exe"
    ExecWait '"$INSTDIR\shinku-agent.exe" ${argument}'
    Push $0
    nsExec::Exec 'taskkill /F /IM shinku-agent.exe'
    Pop $0
    Pop $0
  ${EndIf}
!macroend

; Avant de copier quoi que ce soit : l'ancien agent, s'il tourne, verrouille son fichier.
!macro NSIS_HOOK_PREINSTALL
  !insertmacro ShinkuAskAgentToLeave "--quitter"
!macroend

; Une fois l'application copiée : l'agent à côté de shinku.exe, où celui-ci le cherche.
!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  File "${SHINKU_AGENT_SOURCE}"
!macroend

; Avant de retirer quoi que ce soit : l'agent remet tout en place et efface sa
; valeur de démarrage. Son fichier part tout de suite : Tauri retire le dossier
; d'installation avant le crochet suivant, et un dossier qui contient encore
; l'agent ne se retire pas.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro ShinkuAskAgentToLeave "--retirer"
  Delete "$INSTDIR\shinku-agent.exe"
!macroend

; Au cas où l'agent n'a pas pu être supprimé plus tôt (processus encore en train de sortir).
!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\shinku-agent.exe"
  RMDir "$INSTDIR"
!macroend
