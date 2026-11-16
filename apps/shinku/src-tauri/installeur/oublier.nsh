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
; L'etat de l'attente : le nombre de pas (borne de secours), "shinku.exe a ete vu
; occupe", le nombre de reponses "on ne sait pas" DE SUITE, "on abandonne
; l'attente", et l'horloge (GetTickCount, en millisecondes) : au lancement de
; `--oublier`, au debut de l'attente en cours, et maintenant.
Var ShinkuOublierPas
Var ShinkuOublierVu
Var ShinkuOublierInconnu
Var ShinkuOublierAbandon
Var ShinkuOublierFini
Var ShinkuOublierLance
Var ShinkuOublierDebut
Var ShinkuOublierMaintenant

; Les deux attentes se comptent avec l'HORLOGE : jusqu'a dix secondes pour voir
; `--oublier` occuper le fichier, puis jusqu'a trente secondes pour le voir le
; lacher, par pas de 100 ms entre deux sondages (un sondage prend lui-meme du
; temps : compter les pas ferait durer plus). Les nombres de pas sont une borne
; de secours : ils valent au moins la duree en pas de 100 ms, aucune boucle ne
; depend de l'horloge seule. L'ecart d'horloge est une difference de 32 bits :
; il reste juste quand le compteur repasse par zero.
!define SHINKU_OUBLIER_PAS_MS 100
!define SHINKU_OUBLIER_MS_POUR_VOIR 10000
!define SHINKU_OUBLIER_MS_POUR_FINIR 30000
!define SHINKU_OUBLIER_PAS_POUR_VOIR 100
!define SHINKU_OUBLIER_PAS_POUR_FINIR 300
; Jusqu'a dix reponses "on ne sait pas" de suite sont tolerees (on resonde au pas
; suivant) ; une reponse sure remet le compte a zero. Au dela, on abandonne
; l'attente MAIS on dort jusqu'a 21 s apres le lancement de `--oublier` (son
; echeance est de 20 s : il a fini de lui-meme), pour ne pas supprimer shinku.exe
; sous lui ni faire reposer la question "Shinku est ouvert" par le modele. Puis
; le message, sans arret force, et sans autre sonde.
!define SHINKU_OUBLIER_INCONNU_MAX 10
!define SHINKU_OUBLIER_MS_ECHEANCE 21000
!define SHINKU_OUBLIER_PAS_ECHEANCE 300

; L'horloge, dans $ShinkuOublierMaintenant (n'utilise pas $0, la reponse du sondage).
!macro ShinkuOublierTemps
  System::Call 'kernel32::GetTickCount() i .s'
  Pop $ShinkuOublierMaintenant
!macroend

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
    !insertmacro ShinkuOublierTemps
    StrCpy $ShinkuOublierLance $ShinkuOublierMaintenant
    StrCpy $ShinkuOublierDebut $ShinkuOublierMaintenant
    StrCpy $ShinkuOublierPas 0
    StrCpy $ShinkuOublierVu 0
    StrCpy $ShinkuOublierInconnu 0
    StrCpy $ShinkuOublierAbandon 0
    StrCpy $ShinkuOublierFini 0
    ; 1. Le voir occupe (1 : occupe, 0 : pas encore, -1 : on ne sait pas).
    ${Do}
      !insertmacro ShinkuOublierRm
      ${If} $0 = 1
        StrCpy $ShinkuOublierVu 1
        ${ExitDo}
      ${ElseIf} $0 = -1
        IntOp $ShinkuOublierInconnu $ShinkuOublierInconnu + 1
        ${If} $ShinkuOublierInconnu > ${SHINKU_OUBLIER_INCONNU_MAX}
          StrCpy $ShinkuOublierAbandon 1
          ${ExitDo}
        ${EndIf}
      ${Else}
        StrCpy $ShinkuOublierInconnu 0
      ${EndIf}
      !insertmacro ShinkuOublierTemps
      IntOp $1 $ShinkuOublierMaintenant - $ShinkuOublierDebut
      ${If} $1 >= ${SHINKU_OUBLIER_MS_POUR_VOIR}
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
      !insertmacro ShinkuOublierTemps
      StrCpy $ShinkuOublierDebut $ShinkuOublierMaintenant
      StrCpy $ShinkuOublierPas 0
      StrCpy $ShinkuOublierInconnu 0
      ${Do}
        !insertmacro ShinkuOublierRm
        ${If} $0 = 0
          StrCpy $ShinkuOublierFini 1
          ${ExitDo}
        ${ElseIf} $0 = -1
          IntOp $ShinkuOublierInconnu $ShinkuOublierInconnu + 1
          ${If} $ShinkuOublierInconnu > ${SHINKU_OUBLIER_INCONNU_MAX}
            StrCpy $ShinkuOublierAbandon 1
            ${ExitDo}
          ${EndIf}
        ${Else}
          StrCpy $ShinkuOublierInconnu 0
        ${EndIf}
        !insertmacro ShinkuOublierTemps
        IntOp $1 $ShinkuOublierMaintenant - $ShinkuOublierDebut
        ${If} $1 >= ${SHINKU_OUBLIER_MS_POUR_FINIR}
          ${ExitDo}
        ${EndIf}
        ${If} $ShinkuOublierPas >= ${SHINKU_OUBLIER_PAS_POUR_FINIR}
          ${ExitDo}
        ${EndIf}
        Sleep ${SHINKU_OUBLIER_PAS_MS}
        IntOp $ShinkuOublierPas $ShinkuOublierPas + 1
      ${Loop}
    ${EndIf}
    ; Abandon ("on ne sait pas" trop de fois de suite) : on dort jusqu'a 21 s apres le
    ; lancement, puis le message. Pas d'arret force : on ne sait pas ce qui tient le fichier.
    ${If} $ShinkuOublierAbandon = 1
      StrCpy $ShinkuOublierPas 0
      ${Do}
        !insertmacro ShinkuOublierTemps
        IntOp $1 $ShinkuOublierMaintenant - $ShinkuOublierLance
        ${If} $1 >= ${SHINKU_OUBLIER_MS_ECHEANCE}
          ${ExitDo}
        ${EndIf}
        ${If} $ShinkuOublierPas >= ${SHINKU_OUBLIER_PAS_ECHEANCE}
          ${ExitDo}
        ${EndIf}
        Sleep ${SHINKU_OUBLIER_PAS_MS}
        IntOp $ShinkuOublierPas $ShinkuOublierPas + 1
      ${Loop}
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(shinkuOublierEchec)" /SD IDOK
    ${ElseIf} $ShinkuOublierVu = 0
      ; Jamais vu occupe : le message. Jamais de repli sur un effacement eleve.
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(shinkuOublierEchec)" /SD IDOK
    ${ElseIf} $ShinkuOublierFini = 0
      ; Vu, mais jamais vu fini au bout de 30 s : le message, dans tous les cas. La fin
      ; n'est lue que dans la variable posee par la boucle, jamais dans un registre qui
      ; aurait garde la derniere reponse. Si cette derniere reponse est sure et dit "en
      ; marche", ce qui tient le fichier est d'abord arrete par le Gestionnaire de
      ; redemarrage, sur ce chemin exact, comme le fait le modele (la fenetre a ete fermee
      ; plus haut, et la suite du desinstalleur ne peut pas supprimer un shinku.exe en
      ; marche). Si elle est "on ne sait pas", rien n'est arrete : les 30 s depassent
      ; l'echeance de --oublier, qui a fini de lui-meme.
      ${If} $0 = 1
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
