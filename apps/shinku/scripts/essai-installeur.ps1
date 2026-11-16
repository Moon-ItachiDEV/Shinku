# Essai de l'installeur : le vrai setup, parcouru comme le ferait quelqu'un.
#
# Le script ouvre l'installeur (ou le désinstalleur) de Shinku, attend chaque
# page par ses textes, lit tout ce qu'elle affiche, photographie la fenêtre,
# puis clique le bouton qu'il faut. Il échoue si un texte attendu manque, si un
# texte visible vouvoie, ou si le disque n'est pas dans l'état attendu. Il
# vérifie ainsi l'habillage de src-tauri/installeur/ tel qu'il s'affiche pour
# de bon, ce que les tests de tests/installer.test.ts ne peuvent pas voir.
#
# Trois étapes, à enchaîner dans cet ordre sur une machine jetable :
#   installer     Bienvenue, Dossier, Installation, Terminé ; puis vérifie le
#                 code de sortie, shinku.exe, uninstall.exe, l'absence de
#                 raccourci sur le bureau et le raccourci du menu Démarrer ;
#   deja-installe relance l'installeur : la page « Shinku est déjà installé »,
#                 puis Annuler. Rien ne doit changer sur le disque ;
#   desinstaller  lance uninstall.exe, coche « Supprimer aussi mes réglages »
#                 et vérifie que le dossier d'installation, %APPDATA%\Shinku
#                 et %LOCALAPPDATA%\Shinku ont disparu.
#
#   powershell -File scripts\essai-installeur.ps1 -Installeur setup.exe -Captures captures -Etape installer
#
# Avec -CapturesDansLeJournal, chaque capture est aussi écrite dans le journal,
# en JPEG encodé en base64, entre deux lignes repères :
#   === capture <nom> ===
#   (lignes de 200 caractères au plus)
#   === fin ===
#
# Ce fichier commence par une marque d'ordre des octets (BOM) : sans elle,
# Windows PowerShell 5 lirait ses lettres accentuées de travers. Il tourne
# avec Windows PowerShell 5.1 (pas d'opérateur ternaire ni de ??) et sans
# aucune dépendance. L'écran est celui de la machine : l'essai ne vaut qu'à
# 100 % de mise à l'échelle, ce que fait la machine d'intégration continue.

param(
    # Le programme d'installation à essayer (*-setup.exe).
    [Parameter(Mandatory = $true)][string]$Installeur,
    # Le dossier où écrire les captures (PNG).
    [Parameter(Mandatory = $true)][string]$Captures,
    [Parameter(Mandatory = $true)][ValidateSet('installer', 'deja-installe', 'desinstaller')][string]$Etape,
    [switch]$CapturesDansLeJournal
)

$ErrorActionPreference = 'Stop'

# Pour que les accents du journal soient lisibles.
try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch { }

Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class ShinkuControl
{
    public IntPtr Handle;
    public int Id;
    public string ClassName;
    public string Text;
    public bool Enabled;
}

public static class ShinkuWin
{
    delegate bool EnumProc(IntPtr window, IntPtr parameter);

    [StructLayout(LayoutKind.Sequential)]
    struct RECT
    {
        public int Left, Top, Right, Bottom;
    }

    const uint WM_GETTEXT = 0x000D;
    const uint BM_GETCHECK = 0x00F0;
    const uint BM_SETCHECK = 0x00F1;
    const uint WM_COMMAND = 0x0111;
    const uint SMTO_ABORTIFHUNG = 0x0002;
    const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
    const int SW_RESTORE = 9;

    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr parameter);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc proc, IntPtr parameter);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder text, int size);
    [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr GetDlgItem(IntPtr dialog, int id);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr SendMessageTimeout(IntPtr window, uint message, IntPtr wParam, StringBuilder lParam, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll", EntryPoint = "SendMessageTimeout")]
    static extern IntPtr SendMessageTimeoutPlain(IntPtr window, uint message, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out RECT rect);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr window, int attribute, out RECT rect, int size);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr window, IntPtr context, uint flags);
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);

    // Pour que les coordonnées soient celles des pixels, pas celles d'un écran « virtuel ».
    public static void BeDpiAware() { SetProcessDPIAware(); }

    public static bool Exists(IntPtr window) { return IsWindow(window); }

    // Le texte d'une fenêtre ou d'un contrôle d'un autre processus.
    public static string TextOf(IntPtr window)
    {
        StringBuilder text = new StringBuilder(4096);
        IntPtr result;
        SendMessageTimeout(window, WM_GETTEXT, (IntPtr)4096, text, SMTO_ABORTIFHUNG, 2000, out result);
        return text.ToString();
    }

    // La première fenêtre visible dont le titre (sans les espaces autour : Modern UI
    // met une légende de page " " derrière le titre) est celui-ci, ou zéro.
    public static IntPtr FindTopLevel(string title)
    {
        IntPtr found = IntPtr.Zero;
        EnumProc proc = delegate(IntPtr window, IntPtr parameter)
        {
            if (IsWindowVisible(window) && TextOf(window).Trim() == title)
            {
                found = window;
                return false;
            }
            return true;
        };
        EnumWindows(proc, IntPtr.Zero);
        GC.KeepAlive(proc);
        return found;
    }

    // Tous les contrôles visibles d'une fenêtre, de ses dialogues intérieurs compris.
    public static List<ShinkuControl> VisibleControls(IntPtr root)
    {
        List<ShinkuControl> controls = new List<ShinkuControl>();
        EnumProc proc = delegate(IntPtr window, IntPtr parameter)
        {
            if (IsWindowVisible(window))
            {
                StringBuilder name = new StringBuilder(256);
                GetClassName(window, name, 256);
                ShinkuControl control = new ShinkuControl();
                control.Handle = window;
                control.Id = GetDlgCtrlID(window);
                control.ClassName = name.ToString();
                control.Text = TextOf(window);
                control.Enabled = IsWindowEnabled(window);
                controls.Add(control);
            }
            return true;
        };
        EnumChildWindows(root, proc, IntPtr.Zero);
        GC.KeepAlive(proc);
        return controls;
    }

    public static IntPtr Item(IntPtr dialog, int id) { return GetDlgItem(dialog, id); }
    public static bool Enabled(IntPtr window) { return IsWindowEnabled(window); }
    // Un clic sur un bouton de l'assistant : WM_COMMAND (identifiant, BN_CLICKED = 0,
    // handle du bouton) envoyé à la fenêtre principale, comme le fait Windows.
    public static void Click(IntPtr dialog, int id, IntPtr button)
    {
        PostMessage(dialog, WM_COMMAND, (IntPtr)id, button);
    }

    // Coche (1) ou décoche (0) une case.
    public static void SetChecked(IntPtr button, int state)
    {
        IntPtr result;
        SendMessageTimeoutPlain(button, BM_SETCHECK, (IntPtr)state, IntPtr.Zero, SMTO_ABORTIFHUNG, 2000, out result);
    }

    // 0 : décoché, 1 : coché.
    public static int Checked(IntPtr button)
    {
        IntPtr result;
        SendMessageTimeoutPlain(button, BM_GETCHECK, IntPtr.Zero, IntPtr.Zero, SMTO_ABORTIFHUNG, 2000, out result);
        return (int)result;
    }

    // Le cadre visible de la fenêtre (sans son ombre) : gauche, haut, largeur, hauteur.
    public static int[] Bounds(IntPtr window)
    {
        RECT rect;
        if (DwmGetWindowAttribute(window, DWMWA_EXTENDED_FRAME_BOUNDS, out rect, Marshal.SizeOf(typeof(RECT))) != 0)
        {
            GetWindowRect(window, out rect);
        }
        return new int[] { rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top };
    }

    public static void Raise(IntPtr window)
    {
        ShowWindow(window, SW_RESTORE);
        SetForegroundWindow(window);
    }

    // Repli quand l'écran ne peut pas être copié : la fenêtre se dessine elle-même.
    public static bool Paint(IntPtr window, IntPtr context) { return PrintWindow(window, context, 2); }

    public static int[] ScreenSize() { return new int[] { GetSystemMetrics(0), GetSystemMetrics(1) }; }
}
'@

[ShinkuWin]::BeDpiAware()

$nb = [char]0x00A0
$debut = Get-Date
New-Item -ItemType Directory -Force -Path $Captures | Out-Null

function Dire([string]$ligne) {
    $ecoule = [int]((Get-Date) - $debut).TotalSeconds
    Write-Host ('[{0,4} s] {1}' -f $ecoule, $ligne)
}

# ── Captures ───────────────────────────────────────────────────────────────

# Écrit une image en PNG dans le dossier des captures et, sur demande, en JPEG
# base64 dans le journal.
function Enregistrer-Image([System.Drawing.Bitmap]$image, [string]$nom) {
    $png = Join-Path $Captures ($nom + '.png')
    $image.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    Dire ('capture : ' + $png)
    if ($CapturesDansLeJournal) {
        $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' } | Select-Object -First 1
        $reglages = New-Object System.Drawing.Imaging.EncoderParameters 1
        $reglages.Param[0] = [System.Drawing.Imaging.EncoderParameter]::new([System.Drawing.Imaging.Encoder]::Quality, [long]70)
        $flux = New-Object System.IO.MemoryStream
        $image.Save($flux, $codec, $reglages)
        $texte = [Convert]::ToBase64String($flux.ToArray())
        $flux.Dispose()
        Write-Host ('=== capture ' + $nom + ' ===')
        for ($i = 0; $i -lt $texte.Length; $i += 200) {
            Write-Host $texte.Substring($i, [Math]::Min(200, $texte.Length - $i))
        }
        Write-Host '=== fin ==='
    }
}

# Photographie une fenêtre. L'écran est copié tel qu'il est ; si Windows refuse
# (pas de bureau interactif), la fenêtre est dessinée par elle-même.
function Photographier([IntPtr]$fenetre, [string]$nom) {
    [ShinkuWin]::Raise($fenetre)
    Start-Sleep -Milliseconds 400
    $cadre = [ShinkuWin]::Bounds($fenetre)
    $image = New-Object System.Drawing.Bitmap $cadre[2], $cadre[3]
    $dessin = [System.Drawing.Graphics]::FromImage($image)
    try {
        try {
            $dessin.CopyFromScreen($cadre[0], $cadre[1], 0, 0, $image.Size)
        } catch {
            $contexte = $dessin.GetHdc()
            try { [void][ShinkuWin]::Paint($fenetre, $contexte) } finally { $dessin.ReleaseHdc($contexte) }
        }
        Enregistrer-Image $image $nom
    } finally {
        $dessin.Dispose()
        $image.Dispose()
    }
}

# L'écran entier, pour comprendre un échec.
function Photographier-Ecran([string]$nom) {
    try {
        $taille = [ShinkuWin]::ScreenSize()
        $image = New-Object System.Drawing.Bitmap $taille[0], $taille[1]
        $dessin = [System.Drawing.Graphics]::FromImage($image)
        $dessin.CopyFromScreen(0, 0, 0, 0, $image.Size)
        $dessin.Dispose()
        Enregistrer-Image $image $nom
        $image.Dispose()
    } catch {
        Dire ("capture de l'écran impossible : " + $_.Exception.Message)
    }
}

# ── Lecture de la fenêtre ──────────────────────────────────────────────────

# Un texte tel qu'on le compare : sans le & des raccourcis, fins de ligne unifiées.
function Normaliser([string]$texte) {
    return $texte.Replace('&', '').Replace("`r`n", "`n").Trim()
}

# Les textes visibles de la fenêtre, normalisés, sans les vides.
function Textes-Visibles([IntPtr]$fenetre) {
    $textes = @()
    foreach ($controle in [ShinkuWin]::VisibleControls($fenetre)) {
        $texte = Normaliser $controle.Text
        if ($texte -ne '') {
            $textes += $texte
        }
    }
    return $textes
}

# Un échec : une phrase en français qui dit quelle page et quel texte.
function Echouer([string]$message) {
    throw $message
}

# Aucun texte visible ne doit vouvoyer.
$vouvoiement = '(?i)(?<![\p{L}\p{N}_])(vous|votre|vos|veuillez|cliquez|choisissez|sélectionnez)(?![\p{L}\p{N}_])'

function Verifier-Tutoiement([string]$page, [string[]]$textes) {
    foreach ($texte in $textes) {
        if ($texte -match $vouvoiement) {
            Echouer ("Page « $page » : un texte vouvoie (« " + $Matches[1] + " ») : « $texte ».")
        }
    }
}

# Les textes qu'il manque. Un attendu qui commence par « re: » est un motif.
function Textes-Manquants([string[]]$visibles, [string[]]$attendus) {
    $manquants = @()
    foreach ($attendu in $attendus) {
        $trouve = $false
        foreach ($visible in $visibles) {
            if ($attendu.StartsWith('re:')) {
                if ($visible -match $attendu.Substring(3)) { $trouve = $true; break }
            } elseif ($visible -ceq $attendu) {
                $trouve = $true
                break
            }
        }
        if (-not $trouve) {
            $manquants += $attendu
        }
    }
    return $manquants
}

# Attend la fenêtre d'un titre donné.
function Attendre-Fenetre([string]$titre, [int]$delai) {
    $limite = (Get-Date).AddSeconds($delai)
    while ((Get-Date) -lt $limite) {
        $fenetre = [ShinkuWin]::FindTopLevel($titre)
        if ($fenetre -ne [IntPtr]::Zero) {
            Dire ("fenêtre « $titre » ouverte")
            return $fenetre
        }
        Start-Sleep -Milliseconds 250
    }
    Echouer ("La fenêtre « $titre » ne s'est pas ouverte en $delai s.")
}

# Attend qu'une page affiche tous les textes attendus, vérifie qu'aucun texte
# visible ne vouvoie, la photographie, et rend les textes visibles. Avec
# -Optionnel, une page qui ne vient pas (parce qu'elle est passée trop vite)
# rend $null sans échec ; -Sauf donne le texte de la page suivante, dont
# l'arrivée met fin à l'attente.
function Verifier-Page([IntPtr]$fenetre, [string]$page, [string]$capture, [string[]]$attendus, [int]$delai = 30, [switch]$Optionnel, [int]$pause = 250, [string]$Sauf = '') {
    $limite = (Get-Date).AddSeconds($delai)
    $visibles = @()
    $manquants = $attendus
    while ((Get-Date) -lt $limite) {
        if (-not [ShinkuWin]::Exists($fenetre)) {
            break
        }
        $visibles = @(Textes-Visibles $fenetre)
        $manquants = @(Textes-Manquants $visibles $attendus)
        if ($manquants.Count -eq 0) {
            break
        }
        # La page suivante est déjà là : celle-ci est passée.
        if ($Sauf -ne '' -and $visibles -ccontains $Sauf) {
            break
        }
        Start-Sleep -Milliseconds $pause
    }
    if ($manquants.Count -gt 0) {
        if ($Optionnel) {
            Dire ("page « $page » non vue (passée trop vite ?)")
            return $null
        }
        Echouer ("Page « $page » : texte attendu introuvable : « " + ($manquants -join ' » ; « ') + " ». Textes visibles : " + ($visibles -join ' | '))
    }
    Dire ("page « $page » : " + ($visibles -join ' | '))
    Verifier-Tutoiement $page $visibles
    Photographier $fenetre $capture
    return , $visibles
}

# Clique un bouton de l'assistant par son identifiant : 1 suivant, 2 annuler, 3 précédent.
function Cliquer-Bouton([IntPtr]$fenetre, [int]$id, [string]$nom) {
    $limite = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $limite) {
        $bouton = [ShinkuWin]::Item($fenetre, $id)
        if ($bouton -ne [IntPtr]::Zero -and [ShinkuWin]::Enabled($bouton)) {
            Dire ("clic : $nom")
            [ShinkuWin]::Click($fenetre, $id, $bouton)
            return
        }
        Start-Sleep -Milliseconds 200
    }
    Echouer ("Le bouton « $nom » n'est pas disponible.")
}

# La case à cocher ou le bouton radio dont le texte est celui-ci.
function Trouver-Case([IntPtr]$fenetre, [string]$page, [string]$texte) {
    foreach ($controle in [ShinkuWin]::VisibleControls($fenetre)) {
        if ($controle.ClassName -eq 'Button' -and (Normaliser $controle.Text) -ceq $texte) {
            return $controle.Handle
        }
    }
    Echouer ("Page « $page » : la case « $texte » est introuvable.")
}

# Attend la fin d'un processus et rend son code de sortie.
function Attendre-Fin([System.Diagnostics.Process]$processus, [int]$delai, [string]$nom) {
    if (-not $processus.WaitForExit($delai * 1000)) {
        Echouer ("$nom ne s'est pas terminé en $delai s.")
    }
    return $processus.ExitCode
}

# Attend qu'une fenêtre se ferme.
function Attendre-Fermeture([IntPtr]$fenetre, [int]$delai, [string]$nom) {
    $limite = (Get-Date).AddSeconds($delai)
    while ((Get-Date) -lt $limite) {
        if (-not [ShinkuWin]::Exists($fenetre)) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    Echouer ("La fenêtre « $nom » ne s'est pas fermée en $delai s.")
}

# ── Le disque ──────────────────────────────────────────────────────────────

$programmes = $env:ProgramW6432
if (-not $programmes) { $programmes = $env:ProgramFiles }
$dossier = Join-Path $programmes 'Shinku'
$reglages = Join-Path $env:APPDATA 'Shinku'
# Les journaux du mode jeu et du Démarrage (game.json, startup.json).
$journaux = Join-Path $env:LOCALAPPDATA 'Shinku'

function Verifier-Existe([string]$chemin, [string]$quoi) {
    if (-not (Test-Path -LiteralPath $chemin)) {
        Echouer ("$quoi est introuvable : $chemin")
    }
}

function Verifier-Absent([string]$chemin, [string]$quoi) {
    if (Test-Path -LiteralPath $chemin) {
        Echouer ("$quoi est encore là : $chemin")
    }
}

# Les raccourcis « Shinku.lnk » d'un dossier de raccourcis (bureau, menu Démarrer).
function Raccourcis([string[]]$dossiers) {
    $trouves = @()
    foreach ($racine in $dossiers) {
        if ($racine -and (Test-Path -LiteralPath $racine)) {
            $trouves += @(Get-ChildItem -LiteralPath $racine -Filter 'Shinku.lnk' -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
        }
    }
    return $trouves
}

$bureaux = @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('CommonDesktopDirectory'))
$menus = @([Environment]::GetFolderPath('Programs'), [Environment]::GetFolderPath('CommonPrograms'))

# L'état du dossier d'installation : chaque fichier, sa taille, sa date.
function Etat-Du-Dossier {
    if (-not (Test-Path -LiteralPath $dossier)) {
        return @()
    }
    return @(Get-ChildItem -LiteralPath $dossier -Recurse -Force | Sort-Object FullName | ForEach-Object { '{0}|{1}|{2}' -f $_.FullName, $_.Length, $_.LastWriteTimeUtc.Ticks })
}

# ── Les étapes ─────────────────────────────────────────────────────────────

$titreInstallation = 'Installation de Shinku'
$titreDesinstallation = 'Désinstallation de Shinku'

$accueil = @(
    'Bienvenue dans Shinku',
    "Shinku fait de la place sur ton PC, sans rien supprimer dans ton dos.`n`nL'installation prend quelques secondes.",
    'Suivant',
    'Annuler'
)

function Etape-Installer {
    Verifier-Absent (Join-Path $dossier 'shinku.exe') "Shinku (installé avant l'essai)"
    $processus = Start-Process -FilePath $Installeur -PassThru
    $null = $processus.Handle
    $fenetre = Attendre-Fenetre $titreInstallation 60

    $null = Verifier-Page $fenetre 'Bienvenue' '01-bienvenue' $accueil
    Cliquer-Bouton $fenetre 1 'Suivant'

    $null = Verifier-Page $fenetre 'Dossier' '02-dossier' @(
        "Où installer Shinku${nb}?",
        'Le dossier proposé convient dans la plupart des cas.',
        'Shinku sera installé dans ce dossier. Pour en choisir un autre, clique sur Parcourir.',
        "Dossier d'installation",
        'Parcourir…',
        're:(?i)\\Program Files\\Shinku$',
        'Précédent',
        'Installer',
        'Annuler'
    )
    Cliquer-Bouton $fenetre 1 'Installer'

    # La page d'installation ne dure que quelques secondes : on la guette de près,
    # sans échouer si elle passe trop vite (la page Terminé, elle, est obligatoire).
    $null = Verifier-Page $fenetre 'Installation' '03-installation' @(
        'Installation en cours',
        'Encore quelques secondes.'
    ) -delai 60 -Optionnel -pause 50 -Sauf 'Shinku est installé'

    $null = Verifier-Page $fenetre 'Terminé' '04-termine' @(
        'Shinku est installé',
        "Tout est prêt. Lance une première analyse quand tu veux${nb}: rien n'est supprimé sans ton accord.",
        'Lancer Shinku',
        'Créer un raccourci sur le bureau',
        'Terminer'
    ) -delai 180

    # « Lancer Shinku » est coché ; on le décoche, l'essai de la fenêtre lance
    # lui-même le programme. Le raccourci du bureau ne doit pas être coché.
    $lancer = Trouver-Case $fenetre 'Terminé' 'Lancer Shinku'
    if ([ShinkuWin]::Checked($lancer) -ne 1) {
        Echouer 'Page « Terminé » : « Lancer Shinku » devrait être coché par défaut.'
    }
    $raccourci = Trouver-Case $fenetre 'Terminé' 'Créer un raccourci sur le bureau'
    if ([ShinkuWin]::Checked($raccourci) -ne 0) {
        Echouer 'Page « Terminé » : « Créer un raccourci sur le bureau » ne devrait pas être coché par défaut.'
    }
    [ShinkuWin]::SetChecked($lancer, 0)
    if ([ShinkuWin]::Checked($lancer) -ne 0) {
        Echouer "Page « Terminé » : « Lancer Shinku » n'a pas pu être décoché."
    }
    Cliquer-Bouton $fenetre 1 'Terminer'

    $code = Attendre-Fin $processus 60 "L'installeur"
    if ($code -ne 0) {
        Echouer "L'installeur s'est terminé avec le code $code (0 attendu)."
    }
    Dire "installeur terminé, code $code"

    Verifier-Existe (Join-Path $dossier 'shinku.exe') 'shinku.exe'
    Verifier-Existe (Join-Path $dossier 'shinku-agent.exe') 'shinku-agent.exe'
    Verifier-Existe (Join-Path $dossier 'uninstall.exe') 'uninstall.exe'
    $surBureau = @(Raccourcis $bureaux)
    if ($surBureau.Count -gt 0) {
        Echouer ('Un raccourci a été créé sur le bureau, alors que la case était décochée : ' + ($surBureau -join ', '))
    }
    $dansMenu = @(Raccourcis $menus)
    if ($dansMenu.Count -eq 0) {
        Echouer 'Aucun raccourci « Shinku » dans le menu Démarrer.'
    }
    Dire ('raccourci du menu Démarrer : ' + ($dansMenu -join ', '))
}

function Etape-Deja-Installe {
    Verifier-Existe (Join-Path $dossier 'uninstall.exe') "Shinku (l'étape « installer » doit précéder)"
    $avant = @(Etat-Du-Dossier)
    $processus = Start-Process -FilePath $Installeur -PassThru
    $null = $processus.Handle
    $fenetre = Attendre-Fenetre $titreInstallation 60

    $null = Verifier-Page $fenetre 'Bienvenue' '05-bienvenue-reinstallation' $accueil
    Cliquer-Bouton $fenetre 1 'Suivant'

    $null = Verifier-Page $fenetre 'Shinku est déjà installé' '06-deja-installe' @(
        'Shinku est déjà installé',
        'Choisis ce que tu veux faire.',
        're:^Shinku \S+ est déjà sur ton PC\. Choisis ce que tu veux faire, puis clique sur Suivant\.$',
        'Réinstaller Shinku',
        'Désinstaller Shinku',
        'Précédent',
        'Suivant',
        'Annuler'
    )
    Cliquer-Bouton $fenetre 2 'Annuler'

    $null = Attendre-Fin $processus 60 "L'installeur"
    $apres = @(Etat-Du-Dossier)
    $difference = Compare-Object -ReferenceObject $avant -DifferenceObject $apres
    if ($difference) {
        Echouer ("Le disque a changé alors que l'installation a été annulée : " + (($difference | ForEach-Object { $_.InputObject }) -join ' ; '))
    }
    Dire "rien n'a changé dans $dossier"
}

function Etape-Desinstaller {
    $desinstalleur = Join-Path $dossier 'uninstall.exe'
    Verifier-Existe $desinstalleur "Shinku (l'étape « installer » doit précéder)"
    New-Item -ItemType Directory -Force -Path $reglages | Out-Null
    $fichier = Join-Path $reglages 'settings.json'
    if (-not (Test-Path -LiteralPath $fichier)) {
        Set-Content -LiteralPath $fichier -Value '{}' -Encoding ASCII
    }
    New-Item -ItemType Directory -Force -Path $journaux | Out-Null
    foreach ($nom in 'game.json', 'startup.json') {
        $fichier = Join-Path $journaux $nom
        if (-not (Test-Path -LiteralPath $fichier)) {
            Set-Content -LiteralPath $fichier -Value '{}' -Encoding ASCII
        }
    }

    # Le désinstalleur se recopie dans le dossier temporaire et s'y relance : le
    # processus lancé s'arrête aussitôt, d'où l'attente de la fenêtre par son titre.
    Start-Process -FilePath $desinstalleur | Out-Null
    $fenetre = Attendre-Fenetre $titreDesinstallation 60

    $null = Verifier-Page $fenetre 'Désinstaller' '07-desinstaller' @(
        'Désinstaller Shinku',
        'Shinku va être retiré de ton PC.',
        'Shinku sera désinstallé de ce dossier. Tes fichiers personnels ne sont pas touchés.',
        "Dossier de Shinku${nb}:",
        're:(?i)\\Program Files\\Shinku\\?$',
        'Supprimer aussi mes réglages',
        'Désinstaller',
        'Annuler'
    )
    $case = Trouver-Case $fenetre 'Désinstaller' 'Supprimer aussi mes réglages'
    if ([ShinkuWin]::Checked($case) -ne 0) {
        Echouer 'Page « Désinstaller » : « Supprimer aussi mes réglages » ne devrait pas être coché par défaut.'
    }
    [ShinkuWin]::SetChecked($case, 1)
    if ([ShinkuWin]::Checked($case) -ne 1) {
        Echouer 'Page « Désinstaller » : la case « Supprimer aussi mes réglages » ne se coche pas.'
    }
    Cliquer-Bouton $fenetre 1 'Désinstaller'

    # Comme l'installation, la page « en cours » est brève : guettée sans être exigée.
    $null = Verifier-Page $fenetre 'Désinstallation' '08-desinstallation' @(
        'Désinstallation en cours',
        'Encore quelques secondes.'
    ) -delai 60 -Optionnel -pause 50 -Sauf 'Shinku est désinstallé'

    $null = Verifier-Page $fenetre 'Désinstallé' '09-desinstalle' @(
        'Shinku est désinstallé',
        "Tes fichiers personnels n'ont pas été touchés.",
        'Fermer'
    ) -delai 120
    Cliquer-Bouton $fenetre 1 'Fermer'
    Attendre-Fermeture $fenetre 30 $titreDesinstallation

    # Le désinstalleur supprime son dossier après coup : on lui laisse un peu de temps.
    $limite = (Get-Date).AddSeconds(30)
    while ((Test-Path -LiteralPath $dossier) -and (Get-Date) -lt $limite) {
        Start-Sleep -Milliseconds 500
    }
    Verifier-Absent $dossier "Le dossier d'installation"
    Verifier-Absent $reglages 'Le dossier de réglages %APPDATA%\Shinku'
    Verifier-Absent $journaux 'Le dossier des journaux %LOCALAPPDATA%\Shinku'
    $restes = @(Raccourcis $menus)
    if ($restes.Count -gt 0) {
        Echouer ('Un raccourci du menu Démarrer est resté : ' + ($restes -join ', '))
    }
    Dire "dossier d'installation, réglages et raccourcis ont disparu"
}

# ── Programme ──────────────────────────────────────────────────────────────

try {
    if (-not (Test-Path -LiteralPath $Installeur)) {
        Echouer "Installeur introuvable : $Installeur"
    }
    $Installeur = (Resolve-Path -LiteralPath $Installeur).Path
    Dire ("étape « $Etape » avec $Installeur")
    switch ($Etape) {
        'installer' { Etape-Installer }
        'deja-installe' { Etape-Deja-Installe }
        'desinstaller' { Etape-Desinstaller }
    }
    Dire "✓ étape « $Etape » réussie"
    exit 0
} catch {
    Dire ("✗ ÉCHEC : " + $_.Exception.Message)
    Photographier-Ecran ('echec-' + $Etape)
    exit 1
}
