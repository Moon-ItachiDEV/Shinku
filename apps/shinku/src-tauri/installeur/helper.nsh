; Assistant eleve de Shinku (shinku-helper.exe) dans l'installeur NSIS.
;
; Ce fichier ne definit que quatre macros publiques, que les crochets de Tauri
; (installer-hooks.nsh) appellent, sur le modele d'agent.nsh :
;
;   ShinkuHelperPreInstall     avant toute copie : les assistants en marche
;                              sont arretes ;
;   ShinkuHelperPostInstall    ils sont arretes encore, puis l'assistant est
;                              copie a cote de shinku.exe ;
;   ShinkuHelperPreUninstall   les assistants sont arretes, puis son fichier
;                              part ;
;   ShinkuHelperPostUninstall  filet de securite : le fichier.
;
; L'assistant n'a ni --quitter ni journal : il ne tient aucun etat. Il pose une
; question a la personne (une boite intitulee Shinku) puis ecrit une seule valeur
; dans le registre, apres la reponse. L'arreter de force pendant sa question
; n'ecrit donc rien : taskkill /F suffit, sans ordre de sortie.
;
; Il peut tourner dans toute session Windows ouverte (changement rapide
; d'utilisateur) : taskkill agit sur le PC entier, ce qu'il faut pour que son
; fichier ne soit plus verrouille.
;
; Desinstaller Shinku ne remet PAS les entrees de demarrage du PC que
; l'assistant a coupees : elles restent coupees, comme les entrees du compte
; coupees par la page Demarrage (la personne les remet depuis le Gestionnaire
; des taches).
;
; Chemin de l'assistant compile : meme principe que pour l'agent (voir
; agent.nsh). `${__FILEDIR__}` vaut le dossier de ce fichier, lu hors de toute
; macro ; l'assistant est compile par `beforeBuildCommand` (cargo build
; --release) a la racine du depot, dans target/release : quatre dossiers plus
; haut. Le chemin peut etre impose de l'exterieur en definissant
; SHINKU_HELPER_SOURCE avant l'inclusion.
;
; La variable $UpdateMode est celle du modele NSIS de Tauri : 1 quand le
; desinstalleur est lance avec /UPDATE (mise a jour).

!ifndef SHINKU_HELPER_SOURCE
  !define SHINKU_HELPER_SOURCE "${__FILEDIR__}\..\..\..\..\target\release\shinku-helper.exe"
!endif

; Arrete de force tout shinku-helper.exe, dans toutes les sessions, puis laisse
; un instant a Windows pour lacher le fichier. Sans effet (code 128 de
; taskkill, ignore) s'il n'en tourne aucun.
!macro ShinkuHelperStop
  Push $0
  nsExec::Exec 'taskkill /F /IM shinku-helper.exe'
  Pop $0
  Sleep 500
  Pop $0
!macroend

; Avant de copier quoi que ce soit : un ancien assistant en marche verrouille
; son fichier.
!macro ShinkuHelperPreInstall
  !insertmacro ShinkuHelperStop
!macroend

; Une fois l'application copiee : un assistant a pu etre lance entre-temps par
; la fenetre de Shinku, que Tauri ne ferme qu'apres le crochet precedent. Il
; sort donc une derniere fois, juste avant la copie du nouveau fichier.
!macro ShinkuHelperPostInstall
  !insertmacro ShinkuHelperStop
  SetOutPath "$INSTDIR"
  File "${SHINKU_HELPER_SOURCE}"
!macroend

; Avant de retirer quoi que ce soit : plus d'assistant en marche, puis son
; fichier part tout de suite (Tauri retire le dossier d'installation avant le
; crochet suivant, et un dossier qui contient encore l'assistant ne se retire
; pas). Pas de /REBOOTOK : une suppression remise au redemarrage effacerait
; l'assistant d'une reinstallation faite entre-temps. Meme chose pour une mise
; a jour par le desinstalleur ($UpdateMode = 1) : le fichier est redepose par
; l'installation qui suit.
!macro ShinkuHelperPreUninstall
  !insertmacro ShinkuHelperStop
  Delete "$INSTDIR\shinku-helper.exe"
!macroend

; Au cas ou l'assistant n'a pas pu etre supprime plus tot (lance par la fenetre
; de Shinku entre les deux crochets) : plus rien ne le relance maintenant.
!macro ShinkuHelperPostUninstall
  !insertmacro ShinkuHelperStop
  Delete "$INSTDIR\shinku-helper.exe"
!macroend
