; Crochets de l'installeur NSIS de Shinku (tauri.conf.json,
; bundle.windows.nsis.installerHooks). Tout ce qui concerne l'agent du mode
; jeu, shinku-agent.exe, que Tauri n'embarque pas tout seul, est dans
; installeur/agent.nsh ; tout ce qui concerne l'assistant eleve,
; shinku-helper.exe, est dans installeur/helper.nsh : NSIS n'accepte qu'une
; definition par macro, et un autre fichier de crochets doit pouvoir cohabiter
; avec celui-ci. Ici, une ligne par fichier et par crochet. A la desinstallation,
; l'assistant passe avant l'agent : le dossier d'installation doit etre vide
; quand l'agent le retire.
;
; `${__FILEDIR__}` vaut le dossier de ce fichier (apps/shinku/src-tauri) tant
; qu'il est lu a l'endroit ou il est ecrit : un chemin relatif simple serait
; resolu par rapport au script genere par Tauri, pas a ce fichier.

!include "${__FILEDIR__}\installeur\agent.nsh"
!include "${__FILEDIR__}\installeur\helper.nsh"

!macro NSIS_HOOK_PREINSTALL
  !insertmacro ShinkuAgentPreInstall
  !insertmacro ShinkuHelperPreInstall
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro ShinkuAgentPostInstall
  !insertmacro ShinkuHelperPostInstall
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro ShinkuHelperPreUninstall
  !insertmacro ShinkuAgentPreUninstall
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro ShinkuHelperPostUninstall
  !insertmacro ShinkuAgentPostUninstall
!macroend
