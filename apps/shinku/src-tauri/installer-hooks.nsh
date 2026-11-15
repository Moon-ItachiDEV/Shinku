; Crochets de l'installeur NSIS de Shinku (tauri.conf.json,
; bundle.windows.nsis.installerHooks). Tout ce qui concerne l'agent du mode
; jeu, shinku-agent.exe, que Tauri n'embarque pas tout seul, est dans
; installeur/agent.nsh : NSIS n'accepte qu'une definition par macro, et un
; autre fichier de crochets doit pouvoir cohabiter avec celui-ci. Ici, une
; ligne par crochet.
;
; `${__FILEDIR__}` vaut le dossier de ce fichier (apps/shinku/src-tauri) tant
; qu'il est lu a l'endroit ou il est ecrit : un chemin relatif simple serait
; resolu par rapport au script genere par Tauri, pas a ce fichier.

!include "${__FILEDIR__}\installeur\agent.nsh"

!macro NSIS_HOOK_PREINSTALL
  !insertmacro ShinkuAgentPreInstall
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro ShinkuAgentPostInstall
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro ShinkuAgentPreUninstall
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro ShinkuAgentPostUninstall
!macroend
