; Agent du mode jeu (shinku-agent.exe) dans l'installeur NSIS de Shinku.
;
; Ce fichier ne depend que de LogicLib.nsh et ne definit que quatre macros
; publiques, que les crochets de Tauri (installer-hooks.nsh) appellent :
;
;   ShinkuAgentPreInstall     avant toute copie : l'ancien agent sort ;
;   ShinkuAgentPostInstall    l'agent sort encore, puis est copie a cote de
;                             shinku.exe ;
;   ShinkuAgentPreUninstall   l'agent remet tout en place, puis son fichier part ;
;   ShinkuAgentPostUninstall  filet de securite : le fichier et le dossier.
;
; Les autres macros (ShinkuAgentLeave, ShinkuAgentStillRunning) sont internes.
;
; Chemin de l'agent compile : `${__FILEDIR__}` vaut le dossier du fichier ou il
; est ecrit (ici apps/shinku/src-tauri/installeur), pas celui du script genere
; qui inclut ce fichier ; il doit etre lu hors de toute macro, sans quoi il
; serait relu a l'endroit ou la macro est inseree. L'agent est compile par
; `beforeBuildCommand` (cargo build --release) a la racine du depot, dans
; target/release : quatre dossiers plus haut. Le chemin peut etre impose de
; l'exterieur en definissant SHINKU_AGENT_SOURCE avant l'inclusion.
;
; La variable $UpdateMode est celle du modele NSIS de Tauri : 1 quand le
; desinstalleur est lance avec /UPDATE (mise a jour).

!include LogicLib.nsh

!ifndef SHINKU_AGENT_SOURCE
  !define SHINKU_AGENT_SOURCE "${__FILEDIR__}\..\..\..\..\target\release\shinku-agent.exe"
!endif

; Un shinku-agent.exe tourne-t-il encore, quelle que soit la session Windows ?
; Met 0 dans $1 si oui (sortie de `find`), autre chose sinon.
!macro ShinkuAgentStillRunning
  nsExec::Exec 'cmd /c tasklist /FI "IMAGENAME eq shinku-agent.exe" /NH | find /I "shinku-agent.exe"'
  Pop $1
!macroend

; Fait sortir l'agent avec l'argument donne (--quitter : il remet tout en place
; puis s'arrete ; --retirer : il remet tout en place, efface sa valeur de
; demarrage puis s'arrete), et attend sa fin.
;
; Code de sortie 0 : l'agent du compte a fini proprement. Il reste pourtant un
; cas ou le fichier est verrouille : les agents des autres sessions Windows
; ouvertes (changement rapide d'utilisateur) ne recoivent pas l'ordre, car
; l'ordre est poste a la fenetre du bureau courant. S'il en reste un en marche,
; il est arrete de force : leur journal sera repris au prochain demarrage de
; leur agent (`--demarrage` remet d'abord en place la partie restee au journal).
;
; Autre code (l'agent ne sort pas dans les dix secondes, ou ne se lance pas) :
; arret force, puis le meme ordre est redonne une fois. Plus aucun agent ne
; repond alors, et la commande reprend elle-meme le journal : l'alimentation et
; les apps en veilleuse sont remises en place, et pour --retirer la valeur de
; demarrage est effacee.
!macro ShinkuAgentLeave argument
  Push $0
  Push $1
  ${If} ${FileExists} "$INSTDIR\shinku-agent.exe"
    ClearErrors
    ExecWait '"$INSTDIR\shinku-agent.exe" ${argument}' $0
    ${If} ${Errors}
    ${OrIf} $0 <> 0
      nsExec::Exec 'taskkill /F /IM shinku-agent.exe'
      Pop $1
      Sleep 500
      ClearErrors
      ExecWait '"$INSTDIR\shinku-agent.exe" ${argument}' $0
    ${Else}
      !insertmacro ShinkuAgentStillRunning
      ${If} $1 = 0
        nsExec::Exec 'taskkill /F /IM shinku-agent.exe'
        Pop $1
        Sleep 500
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $1
  Pop $0
!macroend

; Avant de copier quoi que ce soit : l'ancien agent, s'il tourne, verrouille son fichier.
; Ce n'est pas assez : Tauri ferme shinku.exe apres ce crochet, et tant que la
; fenetre vit, elle relance l'agent (voir ShinkuAgentPostInstall).
!macro ShinkuAgentPreInstall
  !insertmacro ShinkuAgentLeave "--quitter"
!macroend

; Une fois l'application copiee : Tauri a ferme shinku.exe, plus rien ne relance
; l'agent. Il sort donc une derniere fois, juste avant la copie du nouveau
; fichier, puis l'agent est depose a cote de shinku.exe, ou celui-ci le cherche.
!macro ShinkuAgentPostInstall
  !insertmacro ShinkuAgentLeave "--quitter"
  SetOutPath "$INSTDIR"
  File "${SHINKU_AGENT_SOURCE}"
!macroend

; Avant de retirer quoi que ce soit : l'agent remet tout en place. Une mise a
; jour qui passe par le desinstalleur (/UPDATE, $UpdateMode = 1) garde la valeur
; de demarrage : --quitter. Une vraie desinstallation l'efface : --retirer.
; Le fichier de l'agent part tout de suite : Tauri retire le dossier
; d'installation avant le crochet suivant, et un dossier qui contient encore
; l'agent ne se retire pas. Pas de /REBOOTOK : une suppression remise au
; redemarrage effacerait l'agent d'une reinstallation faite entre-temps.
!macro ShinkuAgentPreUninstall
  ${If} $UpdateMode = 1
    !insertmacro ShinkuAgentLeave "--quitter"
  ${Else}
    !insertmacro ShinkuAgentLeave "--retirer"
  ${EndIf}
  Delete "$INSTDIR\shinku-agent.exe"
!macroend

; Au cas ou l'agent n'a pas pu etre supprime plus tot (processus encore en train
; de sortir). Pas de RMDir pendant une mise a jour : le dossier va resservir.
!macro ShinkuAgentPostUninstall
  Delete "$INSTDIR\shinku-agent.exe"
  ${If} $UpdateMode <> 1
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
