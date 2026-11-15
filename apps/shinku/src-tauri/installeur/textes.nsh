; Les textes de l'assistant de Shinku (UTF-8), inclus par habillage.nsh.
;
; Le fichier de langue de Modern UI ne définit une chaîne que si elle ne l'est
; pas déjà (LANGFILE_SETSTRING) : une pré-définition ici l'emporte donc sur le
; texte français d'origine. Cela vaut parce que l'installeur n'a qu'une langue.
; Les textes qui se règlent par une commande (boutons, DirText, UninstallText,
; FileErrorText) sont dans francais.nsh.
;
; Règles : on tutoie, on reste court et direct, sans point d'exclamation ; une
; espace insécable (U+00A0) précède « : », « ? » et « ! » dans le texte affiché.

; --- Bienvenue ---
!define MUI_WELCOMEPAGE_TITLE "Bienvenue dans Shinku"
!define MUI_WELCOMEPAGE_TEXT "Shinku fait de la place sur ton PC, sans rien supprimer dans ton dos.$\r$\n$\r$\nL'installation prend quelques secondes."

; --- Dossier ---
!define MUI_TEXT_DIRECTORY_TITLE "Où installer Shinku ?"
!define MUI_TEXT_DIRECTORY_SUBTITLE "Le dossier proposé convient dans la plupart des cas."

; --- Installation ---
!define MUI_TEXT_INSTALLING_TITLE "Installation en cours"
!define MUI_TEXT_INSTALLING_SUBTITLE "Encore quelques secondes."
!define MUI_TEXT_FINISH_TITLE "Installation terminée"
!define MUI_TEXT_FINISH_SUBTITLE "Shinku est installé."
!define MUI_TEXT_ABORT_TITLE "Installation interrompue"
!define MUI_TEXT_ABORT_SUBTITLE "Shinku n'a pas été installé."

; --- Terminé ---
!define MUI_FINISHPAGE_TITLE "Shinku est installé"
!define MUI_FINISHPAGE_TEXT "Tout est prêt. Lance une première analyse quand tu veux : rien n'est supprimé sans ton accord."
!define MUI_FINISHPAGE_RUN_TEXT "Lancer Shinku"
; La case « Créer un raccourci sur le bureau » prend le texte que Tauri lui
; donne : la chaîne createDesktop, dans francais.nsh.
!define MUI_BUTTONTEXT_FINISH "&Terminer"

; Ces deux-là n'apparaissent que si Windows demande un redémarrage, ou si
; l'abandon est confirmé : les versions d'origine vouvoient.
!define MUI_TEXT_FINISH_INFO_REBOOT "Ton PC doit redémarrer pour terminer l'installation de Shinku. Redémarrer maintenant ?"
!define MUI_TEXT_ABORTWARNING "Quitter l'installation de Shinku ?"

; --- Désinstallation ---
!define MUI_UNTEXT_CONFIRM_TITLE "Désinstaller Shinku"
!define MUI_UNTEXT_CONFIRM_SUBTITLE "Shinku va être retiré de ton PC."
!define MUI_UNTEXT_UNINSTALLING_TITLE "Désinstallation en cours"
!define MUI_UNTEXT_UNINSTALLING_SUBTITLE "Encore quelques secondes."
!define MUI_UNTEXT_FINISH_TITLE "Shinku est désinstallé"
!define MUI_UNTEXT_FINISH_SUBTITLE "Tes fichiers personnels n'ont pas été touchés."
!define MUI_UNTEXT_ABORT_TITLE "Désinstallation interrompue"
!define MUI_UNTEXT_ABORT_SUBTITLE "Shinku n'a pas été désinstallé."
!define MUI_UNTEXT_FINISH_INFO_REBOOT "Ton PC doit redémarrer pour terminer la désinstallation de Shinku. Redémarrer maintenant ?"
!define MUI_UNTEXT_ABORTWARNING "Quitter la désinstallation de Shinku ?"
