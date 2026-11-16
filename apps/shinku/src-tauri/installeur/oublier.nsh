; "Supprimer aussi mes reglages" : le desinstalleur n'efface plus rien dans un
; profil. Il demande a `shinku.exe --oublier`, lance avec les droits ordinaires
; de la personne qui est devant le bureau, de le faire (conception : sortie
; publique, lot 1, partie C).
;
; Pourquoi : le modele NSIS de Tauri fait `RmDir /r "$APPDATA\fr.shinku.app"` et
; `RmDir /r "$LOCALAPPDATA\fr.shinku.app"` avec les droits d'administrateur, et
; ce dossier peut etre remplace par une jonction par n'importe quel programme du
; compte : un effacement recursif, eleve, d'un dossier de son choix. Et si
; l'elevation s'est faite avec un autre compte, les reglages vises etaient ceux
; de cet autre compte. Ici, plus aucun Delete ni RMDir vers $APPDATA ou
; $LOCALAPPDATA : tests/installer.test.ts le verifie sur tous nos .nsh.
;
; Deux macros publiques, que les crochets de Tauri (habillage.nsh) appellent :
;
;   ShinkuOublierPreUninstall   si la case est cochee et hors mise a jour : garde
;                               la reponse dans notre variable, remet celle de
;                               Tauri a 0 (son bloc ne fait plus ses deux
;                               "RmDir /r"), lance `shinku.exe --oublier` par
;                               RunAsUser et attend qu'il ait fini ;
;   ShinkuOublierPostUninstall  refait seulement le menage de REGISTRE que ce bloc
;                               du modele faisait (ce ne sont pas des fichiers
;                               d'un profil).
;
; La ligne de commande de `shinku.exe --oublier` est exactement ce seul mot.
; Codes de sortie : 0 tout est parti ou rien a faire, 1 il reste quelque chose,
; 2 refuse (eleve par rapport au bureau). Le desinstalleur ne peut pas les lire :
; RunAsUser ne rend ni poignee ni code. Il attend donc par le verrou du fichier
; shinku.exe, sans greffon de plus : un programme en cours d'execution ne peut
; pas etre ouvert en ecriture. `--oublier` vit au moins une seconde, pour qu'on
; puisse le voir exister.
;
; Les variables $UpdateMode et $DeleteAppDataCheckboxState sont celles du
; modele de Tauri ; ${MAINBINARYNAME} et ${PRODUCTNAME} aussi.

!include LogicLib.nsh

; La reponse de la personne a la case, gardee ici parce que Tauri remet la sienne
; a 0 : 1 seulement si la case est cochee et hors mise a jour.
Var ShinkuOublier

; Le pas de l'attente, en millisecondes, et le nombre de pas : jusqu'a dix
; secondes pour voir `--oublier` occupe le fichier, puis jusqu'a trente secondes
; pour le voir le lacher.
!define SHINKU_OUBLIER_PAS_MS 100
!define SHINKU_OUBLIER_PAS_POUR_VOIR 100
!define SHINKU_OUBLIER_PAS_POUR_FINIR 300

; Le fichier de shinku.exe est-il verrouille ? $0 = 1 s'il l'est, 0 sinon.
; Ouverture en ecriture, fichier existant, partage nul : Windows refuse (erreur
; 32) tant que le programme tourne, ou tant qu'un autre processus l'a ouvert.
; On n'ecrit rien et on ne tronque rien (OPEN_EXISTING, pas de TRUNCATE_EXISTING,
; poignee fermee tout de suite). Un fichier absent (erreur 2 ou 3) n'est pas
; verrouille. Modifie $0, $1 et $2.
!macro ShinkuOublierVerrou
  System::Call 'kernel32::CreateFileW(w "$INSTDIR\${MAINBINARYNAME}.exe", i 0x40000000, i 0, p 0, i 3, i 0, p 0) p .r1 ?e'
  Pop $2
  ${If} $1 P<> -1
    System::Call 'kernel32::CloseHandle(p r1)'
    StrCpy $0 0
  ${ElseIf} $2 = 2
  ${OrIf} $2 = 3
    StrCpy $0 0
  ${Else}
    StrCpy $0 1
  ${EndIf}
!macroend

; Dans la section Uninstall, apres la fermeture de la fenetre et l'arret de
; l'agent et de l'assistant. Utilise $0 a $4 : la section n'y garde rien.
!macro ShinkuOublierPreUninstall
  StrCpy $ShinkuOublier 0
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    StrCpy $ShinkuOublier 1
    StrCpy $DeleteAppDataCheckboxState 0
    nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "--oublier"
    ; 1. Le voir occupe. $3 compte les pas, $4 dit s'il l'a ete.
    StrCpy $3 0
    StrCpy $4 0
    ${Do}
      !insertmacro ShinkuOublierVerrou
      ${If} $0 = 1
        StrCpy $4 1
        ${ExitDo}
      ${EndIf}
      ${If} $3 >= ${SHINKU_OUBLIER_PAS_POUR_VOIR}
        ${ExitDo}
      ${EndIf}
      Sleep ${SHINKU_OUBLIER_PAS_MS}
      IntOp $3 $3 + 1
    ${Loop}
    ; 2. Le voir libre.
    ${If} $4 = 1
      StrCpy $3 0
      ${Do}
        !insertmacro ShinkuOublierVerrou
        ${If} $0 = 0
          ${ExitDo}
        ${EndIf}
        ${If} $3 >= ${SHINKU_OUBLIER_PAS_POUR_FINIR}
          ${ExitDo}
        ${EndIf}
        Sleep ${SHINKU_OUBLIER_PAS_MS}
        IntOp $3 $3 + 1
      ${Loop}
    ${EndIf}
    ; Jamais vu occupe, ou encore occupe : jamais de repli sur un effacement
    ; eleve. S'il est encore occupe, il est arrete avant la suite : la fenetre a
    ; ete fermee plus haut, donc ce qui tient encore le fichier est ce
    ; `--oublier` bloque (il n'ecrit que par remplacement de fichier : l'arreter
    ; ne laisse au pire qu'un ".tmp") ; et la suite du desinstalleur doit pouvoir
    ; supprimer shinku.exe, ce qu'un programme en marche empeche.
    ${If} $4 = 0
    ${OrIf} $0 = 1
      ${If} $0 = 1
        nsExec::Exec 'taskkill /F /IM ${MAINBINARYNAME}.exe'
        Pop $0
        Sleep 500
      ${EndIf}
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(shinkuOublierEchec)" /SD IDOK
    ${EndIf}
  ${EndIf}
!macroend

; Le menage de registre du bloc "Delete app data" du modele de Tauri, a
; l'identique (sans ses deux "RmDir /r", remplaces par --oublier).
!macro ShinkuOublierPostUninstall
  ${If} $ShinkuOublier = 1
  ${AndIf} $UpdateMode <> 1
    ; Clear the install location $INSTDIR from registry
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"
    ; Clear the install language from registry
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"
  ${EndIf}
!macroend
