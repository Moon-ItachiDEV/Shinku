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
; RunAsUser ne rend ni poignee ni code. Il attend donc par le Gestionnaire de
; redemarrage de Windows (Restart Manager), sans greffon de plus, sur le chemin
; EXACT $INSTDIR\shinku.exe, avec les macros que le modele definit deja
; (RestartManager_StartSession, _RegisterFile, _EndSession) et l'appel RmGetList
; de CheckIfAppIsRunning : "occupe" = RmGetList repond ERROR_MORE_DATA (234),
; "libre" = 0. TOUTE AUTRE REPONSE (session impossible a ouvrir, autre code) est
; "on ne sait pas" : un echec de l'attente (le message), jamais "libre" ni "vu".
; Le Gestionnaire de redemarrage ne pose aucun verrou sur le fichier : aucune
; sonde ne peut gener le lancement de shinku.exe. `--oublier` vit au moins une
; seconde, pour qu'on puisse le voir exister.
;
; Une session est ouverte et fermee a CHAQUE pas (ShinkuOublierRm) : je ne suis
; pas certain que RmGetList, rappele dans la meme session, revoie les processus
; lances depuis l'enregistrement du fichier. A VERIFIER ; une session par
; passage serait alors permise. La session est fermee sur tous les chemins.
;
; La pile et RunAsUser : on ne sait pas si le greffon pousse un resultat (le
; modele ne fait pas de Pop apres l'appel). A VERIFIER. Les deux cas sont sans
; effet ici : aucun registre n'est sauve par Push/Pop a cheval sur l'appel, l'etat
; de l'attente est dans des variables a nous, et le resultat n'est jamais lu.
;
; Les variables $UpdateMode et $DeleteAppDataCheckboxState sont celles du
; modele de Tauri ; ${MAINBINARYNAME} et ${PRODUCTNAME} aussi.

!include LogicLib.nsh

; ERROR_MORE_DATA : "il y a des processus" (le modele le definit aussi).
!define /ifndef ERROR_MORE_DATA 234

; La reponse de la personne a la case, gardee ici parce que Tauri remet la sienne
; a 0 : 1 seulement si la case est cochee et hors mise a jour.
Var ShinkuOublier
; L'etat de l'attente : le nombre de pas, "shinku.exe a ete vu occupe", "on ne sait
; pas" (une reponse du Gestionnaire autre que 0 et 234).
Var ShinkuOublierPas
Var ShinkuOublierVu
Var ShinkuOublierInconnu

; Le pas de l'attente, en millisecondes, et le nombre de pas : jusqu'a dix
; secondes pour voir `--oublier` occuper le fichier, puis jusqu'a trente secondes
; pour le voir le lacher.
!define SHINKU_OUBLIER_PAS_MS 100
!define SHINKU_OUBLIER_PAS_POUR_VOIR 100
!define SHINKU_OUBLIER_PAS_POUR_FINIR 300

; shinku.exe tourne-t-il ? $0 = 1 oui (ERROR_MORE_DATA), 0 non, -1 on ne sait pas.
; Modifie $0 a $3 et $R0.
!macro ShinkuOublierRm
  StrCpy $0 -1
  !insertmacro RestartManager_StartSession $R0
  ${If} $R0 != ""
    !insertmacro RestartManager_RegisterFile $R0 "$INSTDIR\${MAINBINARYNAME}.exe"
    ${If} $0 = 0
      System::Call 'RSTRTMGR::RmGetList(p R0, *i .r1, *i .r2, p 0, *i .r3) i .r0'
      ${If} $0 = ${ERROR_MORE_DATA}
        StrCpy $0 1
      ${ElseIf} $0 = 0
        StrCpy $0 0
      ${Else}
        StrCpy $0 -1
      ${EndIf}
    ${Else}
      StrCpy $0 -1
    ${EndIf}
    !insertmacro RestartManager_EndSession $R0
  ${EndIf}
!macroend

; Arrete ce qui tient $INSTDIR\shinku.exe, comme le fait le modele
; (RmShutdown avec RmForceShutdown), sur ce chemin et pas un autre. Modifie $0 a
; $3 et $R0. La session est fermee sur tous les chemins.
!macro ShinkuOublierArreter
  !insertmacro RestartManager_StartSession $R0
  ${If} $R0 != ""
    !insertmacro RestartManager_RegisterFile $R0 "$INSTDIR\${MAINBINARYNAME}.exe"
    ${If} $0 = 0
      System::Call 'RSTRTMGR::RmShutdown(p R0, i ${RmForceShutdown}, p 0) i .r0'
    ${EndIf}
    !insertmacro RestartManager_EndSession $R0
  ${EndIf}
!macroend

; Dans la section Uninstall, apres la fermeture de la fenetre et l'arret de
; l'agent et de l'assistant.
!macro ShinkuOublierPreUninstall
  StrCpy $ShinkuOublier 0
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    StrCpy $ShinkuOublier 1
    StrCpy $DeleteAppDataCheckboxState 0
    nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "--oublier"
    StrCpy $ShinkuOublierPas 0
    StrCpy $ShinkuOublierVu 0
    StrCpy $ShinkuOublierInconnu 0
    ; 1. Le voir occupe (0 : pas encore, 1 : occupe, -1 : on ne sait pas).
    ${Do}
      !insertmacro ShinkuOublierRm
      ${If} $0 = 1
        StrCpy $ShinkuOublierVu 1
        ${ExitDo}
      ${EndIf}
      ${If} $0 = -1
        StrCpy $ShinkuOublierInconnu 1
        ${ExitDo}
      ${EndIf}
      ${If} $ShinkuOublierPas >= ${SHINKU_OUBLIER_PAS_POUR_VOIR}
        ${ExitDo}
      ${EndIf}
      Sleep ${SHINKU_OUBLIER_PAS_MS}
      IntOp $ShinkuOublierPas $ShinkuOublierPas + 1
    ${Loop}
    ; 2. Le voir libre.
    ${If} $ShinkuOublierVu = 1
      StrCpy $ShinkuOublierPas 0
      ${Do}
        !insertmacro ShinkuOublierRm
        ${If} $0 = 0
          ${ExitDo}
        ${EndIf}
        ${If} $0 = -1
          StrCpy $ShinkuOublierInconnu 1
          ${ExitDo}
        ${EndIf}
        ${If} $ShinkuOublierPas >= ${SHINKU_OUBLIER_PAS_POUR_FINIR}
          ${ExitDo}
        ${EndIf}
        Sleep ${SHINKU_OUBLIER_PAS_MS}
        IntOp $ShinkuOublierPas $ShinkuOublierPas + 1
      ${Loop}
    ${EndIf}
    ; Jamais vu occupe, "on ne sait pas", ou encore occupe : le message. Jamais de
    ; repli sur un effacement eleve. Seulement s'il est encore occupe (vu, puis pas
    ; libre au bout de 30 s, reponse sure), ce qui tient le fichier est arrete par le
    ; Gestionnaire de redemarrage, sur ce chemin exact, comme le fait le modele : la
    ; fenetre a ete fermee plus haut, et la suite du desinstalleur ne peut pas
    ; supprimer un shinku.exe en marche.
    ${If} $ShinkuOublierVu = 0
    ${OrIf} $ShinkuOublierInconnu = 1
    ${OrIf} $0 = 1
      ${If} $ShinkuOublierInconnu = 0
      ${AndIf} $0 = 1
        !insertmacro ShinkuOublierArreter
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
