# Essai de l'installeur : le vrai setup, parcouru comme le ferait quelqu'un.
#
# Le script ouvre l'installeur (ou le désinstalleur) de Shinku, attend chaque
# page par ses textes, lit tout ce qu'elle affiche, photographie la fenêtre,
# puis clique le bouton qu'il faut. Il échoue si un texte attendu manque, si un
# texte visible vouvoie, ou si le disque n'est pas dans l'état attendu. Il
# vérifie ainsi l'habillage de src-tauri/installeur/ tel qu'il s'affiche pour
# de bon, ce que les tests de tests/installer.test.ts ne peuvent pas voir.
#
# Cinq étapes, à enchaîner dans cet ordre sur une machine jetable :
#   installer     Bienvenue, Dossier, Installation, Terminé ; puis vérifie le
#                 code de sortie, shinku.exe, shinku-agent.exe,
#                 shinku-helper.exe, uninstall.exe, LICENCES-TIERS.txt (non vide,
#                 il nomme serde),
#                 l'absence de raccourci sur le bureau et le raccourci du menu
#                 Démarrer ;
#   deja-installe relance l'installeur : la page « Shinku est déjà installé »,
#                 puis Annuler. Rien ne doit changer sur le disque ;
#   oublier       lance `shinku.exe --oublier` à la main : avec les droits
#                 ordinaires (code 0, témoins partis, au moins une seconde de
#                 vie), puis élevé face à un bureau qui ne l'est pas (code 2,
#                 rien de touché, si la machine permet ce cas ; sinon une ligne
#                 « NOTE » le dit et rien n'est prétendu) ;
#   desinstaller-jonction
#                 désinstalle avec la case « Supprimer aussi mes réglages »
#                 pendant qu'une jonction tient la place de
#                 %LOCALAPPDATA%\fr.shinku.app : un témoin posé derrière la
#                 jonction doit rester intact (l'ancien RmDir /r élevé du modèle
#                 de Tauri le suivait) ;
#   desinstaller  Shinku étant ouvert, lance uninstall.exe, coche « Supprimer
#                 aussi mes réglages » et vérifie que la question « Shinku est
#                 ouvert » n'est posée qu'une fois, puis que le dossier
#                 d'installation, %APPDATA%\Shinku, %LOCALAPPDATA%\Shinku et les
#                 deux dossiers fr.shinku.app ont disparu, sans que rien revienne.
#
# Chaque preuve écrit une ligne qui commence par « FAIT : » et dit ce qui a été
# MESURÉ. Une ligne « NOTE : » dit ce qui n'a pas pu être essayé : ce n'est
# jamais une preuve. Si le désinstalleur affiche « Shinku n'a pas pu supprimer
# tes réglages », l'essai ferme la boîte (il ne reste pas bloqué), le note,
# photographie l'écran et ÉCHOUE.
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
    [Parameter(Mandatory = $true)][ValidateSet('installer', 'deja-installe', 'oublier', 'desinstaller-jonction', 'desinstaller')][string]$Etape,
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

    // Les fenêtres visibles de premier niveau de classe « #32770 » : la fenêtre de l'assistant et
    // les boîtes de message du désinstalleur.
    public static List<IntPtr> Dialogs()
    {
        List<IntPtr> found = new List<IntPtr>();
        EnumProc proc = delegate(IntPtr window, IntPtr parameter)
        {
            if (IsWindowVisible(window))
            {
                StringBuilder name = new StringBuilder(256);
                GetClassName(window, name, 256);
                if (name.ToString() == "#32770")
                {
                    found.Add(window);
                }
            }
            return true;
        };
        EnumWindows(proc, IntPtr.Zero);
        GC.KeepAlive(proc);
        return found;
    }
}

public static class ShinkuToken
{
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("advapi32.dll")] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll")] static extern bool GetTokenInformation(IntPtr token, int information, out int value, int length, out int returned);

    // 1 : le jeton du processus est élevé ; 0 : il ne l'est pas ; -1 : illisible (accès refusé, processus absent).
    public static int Elevated(int pid)
    {
        IntPtr process = OpenProcess(0x1000, false, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if (process == IntPtr.Zero)
        {
            return -1;
        }
        try
        {
            IntPtr token;
            if (!OpenProcessToken(process, 0x0008, out token)) // TOKEN_QUERY
            {
                return -1;
            }
            try
            {
                int value;
                int returned;
                if (!GetTokenInformation(token, 20, out value, 4, out returned)) // TokenElevation
                {
                    return -1;
                }
                return value != 0 ? 1 : 0;
            }
            finally
            {
                CloseHandle(token);
            }
        }
        finally
        {
            CloseHandle(process);
        }
    }
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
        Traiter-Boites
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
        Traiter-Boites
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
    Verifier-Existe (Join-Path $dossier 'shinku-helper.exe') 'shinku-helper.exe'
    Verifier-Existe (Join-Path $dossier 'uninstall.exe') 'uninstall.exe'
    # La liste des licences des composants tiers : posée à côté de shinku.exe, non vide, et elle nomme serde.
    $fichierLicences = Join-Path $dossier 'LICENCES-TIERS.txt'
    Verifier-Existe $fichierLicences 'LICENCES-TIERS.txt'
    $texteLicences = [System.IO.File]::ReadAllText($fichierLicences, [System.Text.Encoding]::UTF8)
    if ($texteLicences.Length -eq 0) { Echouer 'LICENCES-TIERS.txt est posé mais vide.' }
    if ($texteLicences -notmatch '(?m)^serde \d') { Echouer 'LICENCES-TIERS.txt ne nomme pas le paquet serde.' }
    Fait ("LICENCES-TIERS.txt est posé à côté de shinku.exe : {0} caractères, il nomme le paquet serde" -f $texteLicences.Length)
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

# ── Le profil : ce que « Supprimer aussi mes réglages » doit faire disparaître ──
#
# Sur la machine d'intégration continue, tout tourne dans le profil d'un seul
# compte, administrateur : le script (jeton élevé), le désinstalleur (élevé, lui
# aussi), et `shinku.exe --oublier` que le désinstalleur lance par RunAsUser, ou
# que l'essai lance par lancer-en-utilisateur.ps1 (même compte, jeton filtré,
# niveau d'intégrité moyen). $env:APPDATA et $env:LOCALAPPDATA de ce script sont
# donc ceux que `--oublier` doit effacer : c'est ce profil que l'essai regarde, et
# les lignes « FAIT » disent lequel (compte et chemins). L'essai ne prouve pas
# le cas de deux comptes (compte ordinaire et mot de passe d'un administrateur) :
# une seule machine, un seul compte.

$identifiant = 'fr.shinku.app'
$dossiersProfil = @(
    (Join-Path $env:APPDATA 'Shinku'),
    (Join-Path $env:LOCALAPPDATA 'Shinku'),
    (Join-Path $env:APPDATA $identifiant),
    (Join-Path $env:LOCALAPPDATA $identifiant)
)
$travail = Join-Path $env:TEMP 'essai-installeur'
$sauvegarde = Join-Path $travail 'profil-sauvegarde'
$lanceurEnUtilisateur = Join-Path $PSScriptRoot 'lancer-en-utilisateur.ps1'

# Une preuve réussie : la ligne commence par « FAIT : » et dit ce qui a été mesuré.
function Fait([string]$ligne) { Write-Host "FAIT : $ligne" }

# Ce qui n'a pas pu être essayé, ou ce qui mérite d'être lu : jamais pris pour une preuve.
function Noter([string]$ligne) { Write-Host "NOTE : $ligne" }

function Compte-Courant { return [Security.Principal.WindowsIdentity]::GetCurrent().Name }

# Les fichiers de réglages et de journaux, avec leurs voisins .tmp.
function Chemins-Reglages {
    $roaming = Join-Path $env:APPDATA 'Shinku'
    $local = Join-Path $env:LOCALAPPDATA 'Shinku'
    $chemins = @()
    foreach ($nom in 'settings.json', 'settings.illisible.json', 'journal.json', 'settings.json.tmp', 'journal.json.tmp') {
        $chemins += (Join-Path $roaming $nom)
    }
    foreach ($nom in 'game.json', 'game.json.tmp', 'startup.json', 'startup.json.tmp') {
        $chemins += (Join-Path $local $nom)
    }
    return $chemins
}

# Un fichier au fond des deux arborescences du modèle de Tauri (le cache de WebView2 est dans la seconde).
function Chemins-Arbres {
    return @(
        (Join-Path $env:APPDATA ($identifiant + '\essai\temoin-roaming.txt')),
        (Join-Path $env:LOCALAPPDATA ($identifiant + '\EBWebView\Default\temoin-local.txt'))
    )
}

function Poser-Fichier([string]$chemin, [string]$contenu) {
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($chemin))
    [IO.File]::WriteAllText($chemin, $contenu, [Text.Encoding]::ASCII)
}

# Pose un témoin partout où `--oublier` doit passer. Un fichier déjà là (écrit par
# Shinku lui-même) est laissé tel quel.
function Poser-Temoins {
    foreach ($chemin in (@(Chemins-Reglages) + @(Chemins-Arbres))) {
        if (-not [IO.File]::Exists($chemin)) {
            Poser-Fichier $chemin '{}'
        }
    }
}

# Ceux de ces chemins qui existent encore.
function Restants([string[]]$chemins) {
    return @($chemins | Where-Object { [IO.File]::Exists($_) -or [IO.Directory]::Exists($_) })
}

function Est-Lien([string]$chemin) {
    try { return (([IO.File]::GetAttributes($chemin) -band [IO.FileAttributes]::ReparsePoint) -ne 0) } catch { return $false }
}

# Supprime un fichier, un dossier ou un lien SANS JAMAIS SUIVRE un point d'analyse :
# un lien (jonction) est retiré seul, ce qu'il vise reste.
function Supprimer-Arbre([string]$chemin) {
    $dossierExiste = [IO.Directory]::Exists($chemin)
    if (-not ($dossierExiste -or [IO.File]::Exists($chemin))) { return }
    if (Est-Lien $chemin) {
        if ($dossierExiste) { [IO.Directory]::Delete($chemin) } else { [IO.File]::Delete($chemin) }
        return
    }
    if (-not $dossierExiste) { [IO.File]::Delete($chemin); return }
    foreach ($enfant in [IO.Directory]::GetFileSystemEntries($chemin)) { Supprimer-Arbre $enfant }
    [IO.Directory]::Delete($chemin)
}

# Garde le profil de la machine tel qu'il est (ce que les essais d'avant y ont écrit pour de bon),
# pour que les essais de `--oublier` et de la jonction ne le fassent pas disparaître avant
# l'essai final de la désinstallation.
function Sauver-Profil {
    Supprimer-Arbre $sauvegarde
    [void][IO.Directory]::CreateDirectory($sauvegarde)
    for ($i = 0; $i -lt $dossiersProfil.Count; $i++) {
        $source = $dossiersProfil[$i]
        if ([IO.Directory]::Exists($source)) {
            if (Est-Lien $source) { Echouer "Un lien est en place de $source avant l'essai : l'essai ne le copie pas." }
            Copy-Item -LiteralPath $source -Destination (Join-Path $sauvegarde ([string]$i)) -Recurse -Force
        }
    }
}

function Restaurer-Profil {
    for ($i = 0; $i -lt $dossiersProfil.Count; $i++) {
        $copie = Join-Path $sauvegarde ([string]$i)
        Supprimer-Arbre $dossiersProfil[$i]
        if ([IO.Directory]::Exists($copie)) {
            Copy-Item -LiteralPath $copie -Destination $dossiersProfil[$i] -Recurse -Force
        }
    }
    Supprimer-Arbre $sauvegarde
}

# Shinku qui tourne encore (un essai d'avant) : arrêté, la machine est jetable.
function Arreter-Shinku {
    foreach ($processus in @(Get-Process -Name 'shinku' -ErrorAction SilentlyContinue)) {
        Noter "shinku.exe (PID $($processus.Id)) tournait encore : arrêté avant l'essai."
        try { $processus.Kill() } catch { }
        [void]$processus.WaitForExit(15000)
    }
}

# Lance une ligne de commande avec le jeton ordinaire (lancer-en-utilisateur.ps1), par un petit fichier .cmd :
# la valeur de -Arguments d'un script lancé par -File ne doit pas commencer par un tiret. Rend le processus
# du lanceur : son code de sortie est celui de la ligne de commande.
function Lancer-En-Utilisateur([string]$ligne, [string]$nom) {
    [void][IO.Directory]::CreateDirectory($travail)
    $cmd = Join-Path $travail "$nom.cmd"
    [IO.File]::WriteAllText($cmd, ("@echo off`r`n" + $ligne + "`r`nexit /b %errorlevel%`r`n"), [Text.Encoding]::ASCII)
    $sortie = Join-Path $travail "$nom.sortie.txt"
    Remove-Item -LiteralPath $sortie, "$sortie.err" -Force -ErrorAction SilentlyContinue
    $parametres = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $lanceurEnUtilisateur,
        '-Programme', (Join-Path $env:SystemRoot 'System32\cmd.exe'), '-Arguments', ('/c ""{0}""' -f $cmd))
    $parametres = @($parametres | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } })
    $lanceur = Start-Process -FilePath 'powershell.exe' -ArgumentList $parametres -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $sortie -RedirectStandardError "$sortie.err"
    $null = $lanceur.Handle
    return $lanceur
}

function Erreur-Du-Lanceur([string]$nom) {
    $fichier = Join-Path $travail "$nom.sortie.txt.err"
    if (Test-Path -LiteralPath $fichier) { return ((Get-Content -LiteralPath $fichier) -join ' ') }
    return ''
}

# ── Les boîtes de message du désinstalleur ─────────────────────────────────

# Pendant la désinstallation, deux boîtes peuvent s'ouvrir et bloquer le désinstalleur :
#   - « Shinku est ouvert. Clique sur OK pour le fermer et continuer. » (le modèle de Tauri,
#     appelé par nos crochets AVANT `--oublier`, puis rejoué par le modèle sans plus rien
#     trouver : elle ne doit donc être posée qu'une fois) ;
#   - « Shinku n'a pas pu supprimer tes réglages. » (oublier.nsh : `--oublier` n'a pas été vu
#     ou n'a pas fini). Sur la machine d'essai, c'est un ÉCHEC : la boîte est notée,
#     photographiée et fermée (l'essai ne reste jamais bloqué), puis l'essai échoue.
$script:Surveiller = $false
$script:ProcessVus = @{}
$script:DernierGuet = [DateTime]::MinValue
$script:BoitesVues = @{}
$script:Questions = 0
$script:Echecs = @()

# Le compte propriétaire d'un processus ou « inconnu » (lu par WMI).
function Proprietaire([int]$pid_) {
    try {
        $p = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$pid_" -ErrorAction Stop
        $o = Invoke-CimMethod -InputObject $p -MethodName GetOwner -ErrorAction Stop
        if ($o.User) { return ($o.Domain + '\' + $o.User) }
    } catch { }
    return 'inconnu'
}

# Écrit dans le journal l'Explorateur de la session : c'est lui que RunAsUser imite (PID, compte,
# jeton élevé ou non), ou « aucun » : sans lui, ou avec celui d'un autre compte, `--oublier` ne
# tourne pas comme prévu et le journal doit le dire sans que personne ait à le deviner.
function Noter-Explorateur {
    $session = [Diagnostics.Process]::GetCurrentProcess().SessionId
    $explorateur = Get-Process -Name 'explorer' -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session } | Select-Object -First 1
    if ($null -eq $explorateur) {
        Noter "Explorateur de la session $session : aucun (RunAsUser n'a peut-être personne à imiter)."
        return
    }
    Noter ("Explorateur de la session ${session} : PID $($explorateur.Id), compte $(Proprietaire $explorateur.Id), jeton " + (Decrire-Jeton ([ShinkuToken]::Elevated($explorateur.Id))) + ' ; ce script : compte ' + (Compte-Courant) + ', jeton ' + (Decrire-Jeton ([ShinkuToken]::Elevated([Diagnostics.Process]::GetCurrentProcess().Id))))
}

# Note, une fois par processus, tout shinku.exe vu pendant la désinstallation : sa ligne de commande
# et son compte (`--oublier` lancé par RunAsUser, ou la fenêtre). Au plus une lecture par seconde.
function Guetter-Shinku {
    if (((Get-Date) - $script:DernierGuet).TotalSeconds -lt 1) { return }
    $script:DernierGuet = Get-Date
    try {
        foreach ($p in @(Get-CimInstance -ClassName Win32_Process -Filter "Name='shinku.exe'" -ErrorAction Stop)) {
            $cle = [string]$p.ProcessId
            if ($script:ProcessVus.ContainsKey($cle)) { continue }
            $script:ProcessVus[$cle] = $true
            Noter ("shinku.exe vu pendant la désinstallation : PID $($p.ProcessId), ligne de commande « $($p.CommandLine) », compte $(Proprietaire $p.ProcessId)")
        }
    } catch { }
}

# Ce qui reste dans le profil, et les msedgewebview2.exe encore en vie dont la ligne de commande
# nomme fr.shinku.app (ils peuvent tenir le cache plus longtemps que le réessai de --oublier).
# Elle ne peut jamais remplacer le message d'échec d'origine : toute exception est rendue en clair, à la place de la liste.
function Decrire-Restes {
    try {
        $morceaux = @()
        foreach ($racine in $dossiersProfil) {
            try {
                if (-not [IO.Directory]::Exists($racine)) { continue }
                if (Est-Lien $racine) { $morceaux += "$racine (lien)"; continue }
                $contenu = @(Lister $racine)
                $morceaux += ("$racine : " + $contenu.Count + ' éléments' + $(if ($contenu.Count -gt 0) { ' dont ' + (($contenu | Select-Object -First 15) -join ' ; ') } else { '' }))
            } catch {
                $morceaux += "$racine (liste impossible : $($_.Exception.Message))"
            }
        }
        $web = @()
        try {
            $web = @(Get-CimInstance -ClassName Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction Stop | Where-Object { $_.CommandLine -like '*fr.shinku.app*' })
        } catch { }
        $morceaux += ("msedgewebview2.exe en vie nommant fr.shinku.app : " + $web.Count + $(if ($web.Count -gt 0) { ' (PID ' + (($web | ForEach-Object { $_.ProcessId }) -join ', ') + ')' } else { '' }))
        return ($morceaux -join ' | ')
    } catch {
        return "(liste impossible : $($_.Exception.Message))"
    }
}

$script:ProfilNonRemis = ''

# Avant chaque étape : l'état laissé par une étape précédente (qui a pu échouer) ne doit ni fausser
# celle-ci ni cacher sa vraie cause.
#   - une copie du désinstalleur (~nsu*.tmp\Un*.exe) encore en vie est arrêtée, avec une ligne NOTE ;
#     s'il en reste, l'étape échoue avec ce motif ;
#   - les boîtes restées ouvertes sont fermées et leurs poignées retenues : seules les boîtes apparues
#     après le début de l'étape seront comptées ;
#   - un appel WMI « à blanc » : le premier est lent à froid, le journal ne doit pas le payer pendant l'attente.
function Preparer-Etape {
    $script:ProfilNonRemis = ''
    $script:Questions = 0
    $script:Echecs = @()
    $script:ProcessVus = @{}
    try { $null = @(Get-CimInstance -ClassName Win32_Process -Filter "Name='shinku.exe'" -ErrorAction Stop) } catch { }
    $copies = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -like '*\~nsu*.tmp\Un*.exe' })
    foreach ($copie in $copies) {
        Noter "une copie du désinstalleur d'une étape précédente tournait encore (PID $($copie.Id), $($copie.Path)) : arrêtée."
        try { $copie.Kill() } catch { }
        [void]$copie.WaitForExit(15000)
    }
    if ($copies.Count -gt 0) {
        Start-Sleep -Seconds 1
        $restes = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -like '*\~nsu*.tmp\Un*.exe' })
        if ($restes.Count -gt 0) { Echouer "Une copie du désinstalleur d'une étape précédente tourne encore et n'a pas pu être arrêtée (PID $($restes[0].Id))." }
    }
    $script:BoitesVues = @{}
    foreach ($fenetre in [ShinkuWin]::Dialogs()) {
        $textes = @(Textes-Visibles $fenetre)
        if (@($textes | Where-Object { $_ -match '^Shinku est ouvert\.' -or $_ -match "^Shinku n'a pas pu supprimer tes réglages" }).Count -gt 0) {
            Noter ("une boîte d'une étape précédente était restée ouverte, fermée : " + ($textes -join ' | '))
            [ShinkuWin]::Click($fenetre, 1, [ShinkuWin]::Item($fenetre, 1))
            $script:BoitesVues[[string]$fenetre.ToInt64()] = 'ancienne'
        }
    }
}

function Traiter-Boites {
    if (-not $script:Surveiller) { return }
    Guetter-Shinku
    foreach ($fenetre in [ShinkuWin]::Dialogs()) {
        $cle = [string]$fenetre.ToInt64()
        if ($script:BoitesVues.ContainsKey($cle)) { continue }
        $textes = @(Textes-Visibles $fenetre)
        $nature = ''
        if (@($textes | Where-Object { $_ -match '^Shinku est ouvert\.' }).Count -gt 0) { $nature = 'question' }
        elseif (@($textes | Where-Object { $_ -match "^Shinku n'a pas pu supprimer tes réglages" }).Count -gt 0) { $nature = 'echec' }
        if ($nature -eq '') { continue }
        $script:BoitesVues[$cle] = $nature
        if ($nature -eq 'question') {
            $script:Questions++
            Dire ("boîte « Shinku est ouvert » (n° $($script:Questions)) : " + ($textes -join ' | '))
            $capture = 'question-ouvert-' + $script:Questions
        } else {
            $script:Echecs += ($textes -join ' | ')
            Write-Host ("NOTE : la boîte d'échec de --oublier est apparue : " + ($textes -join ' | '))
            $capture = 'echec-oublier'
        }
        try { Photographier $fenetre $capture } catch { }
        $bouton = [ShinkuWin]::Item($fenetre, 1)
        [ShinkuWin]::Click($fenetre, 1, $bouton)
    }
}

# Attend la fin des copies du désinstalleur (%TEMP%\~nsu<n>.tmp\Un*.exe) : il rend la main aussitôt
# et finit après (registre, dossier). L'installation suivante ne doit pas courir en même temps.
function Attendre-Desinstalleur {
    $limite = (Get-Date).AddSeconds(120)
    do {
        Traiter-Boites
        Start-Sleep -Milliseconds 500
        $copies = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -like '*\~nsu*.tmp\Un*.exe' })
    } while ($copies.Count -gt 0 -and (Get-Date) -lt $limite)
    if ($copies.Count -gt 0) { Echouer 'Le désinstalleur tourne encore après 120 s.' }
}

# L'état d'un jeton lu par ShinkuToken.Elevated, en clair.
function Decrire-Jeton([int]$etat) {
    if ($etat -eq 1) { return 'élevé' }
    if ($etat -eq 0) { return 'non élevé' }
    return 'illisible ou absent'
}

# Les chemins d'une arborescence (sans lien : la cible de la jonction n'en contient aucun), triés ;
# aucun si elle manque.
function Lister([string]$racine) {
    if (-not [IO.Directory]::Exists($racine)) { return @() }
    return @(Get-ChildItem -LiteralPath $racine -Recurse -Force | Sort-Object FullName | ForEach-Object { $_.FullName })
}

# ── L'essai de shinku.exe --oublier, lancé à la main ───────────────────────

function Etape-Oublier {
    $programme = Join-Path $dossier 'shinku.exe'
    Verifier-Existe $programme "shinku.exe (l'étape « installer » doit précéder)"
    Preparer-Etape
    Arreter-Shinku
    $compte = Compte-Courant
    Sauver-Profil
    try {
        # 1. Avec les droits ordinaires de la personne : code 0, témoins partis, au moins une seconde de vie.
        Poser-Temoins
        $poses = @(@(Chemins-Reglages) + @(Chemins-Arbres))
        $lanceur = Lancer-En-Utilisateur ('start "" /wait "{0}" --oublier' -f $programme) 'oublier'
        $vu = $null
        $debutLancement = Get-Date
        $limite = (Get-Date).AddSeconds(90)
        while (-not $lanceur.HasExited -and (Get-Date) -lt $limite) {
            if ($null -eq $vu) {
                $trouve = Get-Process -Name 'shinku' -ErrorAction SilentlyContinue | Select-Object -First 1
                if ($null -ne $trouve) { try { $null = $trouve.Handle; $vu = $trouve } catch { } }
            }
            Start-Sleep -Milliseconds 25
        }
        if (-not $lanceur.HasExited) {
            try { $lanceur.Kill() } catch { }
            Arreter-Shinku
            Echouer "shinku.exe --oublier ne s'est pas terminé en 90 s."
        }
        $ecoule = ((Get-Date) - $debutLancement).TotalSeconds
        $code = $lanceur.ExitCode
        $erreur = Erreur-Du-Lanceur 'oublier'
        if ($code -ne 0) {
            Echouer ("shinku.exe --oublier (droits ordinaires) a rendu le code $code ; 0 attendu. Restants : " + ((Restants $poses) -join ', ') + " $erreur")
        }
        if ($null -eq $vu) {
            Echouer "shinku.exe --oublier n'a jamais été vu en train de tourner : il doit vivre au moins une seconde."
        }
        [void]$vu.WaitForExit(10000)
        $vie = ($vu.ExitTime - $vu.StartTime).TotalSeconds
        if ($vie -lt 1.0) {
            Echouer ('shinku.exe --oublier a vécu {0:N2} s ; au moins 1 s attendue (le désinstalleur ne le voit que par le verrou de son fichier).' -f $vie)
        }
        $restants = @(Restants $poses)
        if ($restants.Count -gt 0) {
            Echouer ('shinku.exe --oublier a rendu le code 0 mais il reste : ' + ($restants -join ', ') + ' (' + (Decrire-Restes) + ')')
        }
        foreach ($arbre in @($dossiersProfil[2], $dossiersProfil[3])) {
            if ([IO.Directory]::Exists($arbre)) { Echouer ("shinku.exe --oublier a rendu le code 0 mais $arbre est encore là (" + (Decrire-Restes) + ')') }
        }
        Fait ("shinku.exe --oublier lancé avec les droits ordinaires (jeton filtré, compte $compte, profil $env:APPDATA et $env:LOCALAPPDATA) : code $code, vécu {0:N2} s (dates de début et de fin du processus), {1} témoins posés partis (réglages, journaux, et un fichier au fond de chacun des deux dossiers $identifiant), les deux dossiers $identifiant partis" -f $vie, $poses.Count)

        # 2. Élevé, face à un bureau qui ne l'est pas : code 2, rien de touché. Seul le jeton de ce
        #    script (élevé ou non) et celui de l'Explorateur de la session permettent de savoir si ce cas
        #    existe sur cette machine : sinon rien n'est prétendu.
        Poser-Temoins
        $poses = @(@(Chemins-Reglages) + @(Chemins-Arbres))
        $moi = [ShinkuToken]::Elevated([Diagnostics.Process]::GetCurrentProcess().Id)
        $session = [Diagnostics.Process]::GetCurrentProcess().SessionId
        $explorateur = Get-Process -Name 'explorer' -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session } | Select-Object -First 1
        $bureau = -1
        if ($null -ne $explorateur) { $bureau = [ShinkuToken]::Elevated($explorateur.Id) }
        $processusEleve = Start-Process -FilePath $programme -ArgumentList '--oublier' -Wait -PassThru -WindowStyle Hidden
        $codeEleve = $processusEleve.ExitCode
        $restants = @(Restants $poses)
        if ($moi -eq 1 -and $bureau -eq 0) {
            if ($codeEleve -ne 2) {
                Echouer ("shinku.exe --oublier lancé élevé face à un bureau non élevé (Explorateur PID $($explorateur.Id)) a rendu le code $codeEleve ; 2 attendu.")
            }
            if ($restants.Count -ne $poses.Count) {
                Echouer ('shinku.exe --oublier a refusé (code 2) mais a touché au profil : il reste ' + $restants.Count + ' témoins sur ' + $poses.Count + '.')
            }
            Fait ("shinku.exe --oublier lancé élevé face à un bureau non élevé (Explorateur PID $($explorateur.Id), jeton non élevé ; ce script : jeton élevé) : code $codeEleve, les {0} témoins sont tous restés" -f $poses.Count)
        } else {
            Noter ("le refus (code 2) n'a pas pu être essayé : jeton de ce script $(Decrire-Jeton $moi), jeton de l'Explorateur de la session $(Decrire-Jeton $bureau). Il faut un script élevé face à un bureau non élevé. Rendu : code $codeEleve, $($restants.Count) témoins restants sur $($poses.Count).")
            if ($moi -eq -1 -or $bureau -eq -1) {
                # Droits illisibles : Shinku les prend pour un refus (code 2). Rien n'est exigé du code, et rien n'est prouvé.
                Noter "droits du bureau ou du script illisibles : le code $codeEleve n'est pas exigé."
            } elseif ($codeEleve -ne 0) {
                Echouer ("shinku.exe --oublier lancé élevé, sans bureau non élevé, a rendu le code $codeEleve ; 0 attendu (le contrôle de compte coupé passe).")
            }
        }
    } finally {
        foreach ($chemin in @(@(Chemins-Reglages) + @(Chemins-Arbres))) {
            try { if ([IO.File]::Exists($chemin)) { [IO.File]::Delete($chemin) } } catch { }
        }
        # Un échec de la remise en place ne doit pas cacher l'erreur d'origine.
        try { Restaurer-Profil } catch {
            $script:ProfilNonRemis = $_.Exception.Message
            Noter "le profil n'a pas pu être remis en place : $($script:ProfilNonRemis)"
        }
    }
    # Atteint seulement sans erreur d'origine : une étape ne réussit pas avec un profil non remis en place.
    if ($script:ProfilNonRemis -ne '') { Echouer "Le profil de la machine n'a pas pu être remis en place après l'essai : $($script:ProfilNonRemis)" }
}

# ── La désinstallation, case cochée ────────────────────────────────────────

# Lance uninstall.exe, coche « Supprimer aussi mes réglages », désinstalle, ferme les boîtes qui
# s'ouvrent (voir Traiter-Boites) et attend la fin. Rend quand le dossier d'installation a disparu.
function Message-Echec-Oublier {
    return ("Le désinstalleur a affiché « Shinku n'a pas pu supprimer tes réglages » (boîte fermée par l'essai, capture echec-oublier) : " + ($script:Echecs -join ' ; ') + ". RunAsUser n'a pas lancé shinku.exe --oublier sur cette machine, ou il n'a pas fini en 40 s, ou le Gestionnaire de redémarrage n'a pas répondu.")
}

function Desinstaller-Avec-La-Case {
    Noter-Explorateur
    try {
        Desinstaller-Pas-A-Pas
    } catch {
        # La boîte d'échec est la cause la plus probable : elle est testée AVANT toute autre explication.
        $cause = $_.Exception.Message
        try { Traiter-Boites } catch { }
        if ($script:Echecs.Count -gt 0) { Echouer ((Message-Echec-Oublier) + " Erreur vue ensuite : $cause") }
        throw
    }
    if ($script:Echecs.Count -gt 0) { Echouer (Message-Echec-Oublier) }
}

function Desinstaller-Pas-A-Pas {
    $desinstalleur = Join-Path $dossier 'uninstall.exe'
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
    $script:Surveiller = $true
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
    ) -delai 180
    Cliquer-Bouton $fenetre 1 'Fermer'
    Attendre-Fermeture $fenetre 30 $titreDesinstallation

    # Le désinstalleur supprime son dossier après coup : on lui laisse un peu de temps.
    $limite = (Get-Date).AddSeconds(30)
    while ((Test-Path -LiteralPath $dossier) -and (Get-Date) -lt $limite) {
        Traiter-Boites
        Start-Sleep -Milliseconds 500
    }
    if ($script:Echecs.Count -gt 0) { return }
    Verifier-Absent $dossier "Le dossier d'installation"
    $script:Surveiller = $false
}

# ── La jonction à la place de %LOCALAPPDATA%\fr.shinku.app ─────────────────

# Le défaut le plus grave de l'ancien désinstalleur : le modèle de Tauri faisait un RmDir /r élevé de
# %LOCALAPPDATA%\fr.shinku.app, qu'un programme du compte peut remplacer par une jonction. Un témoin
# posé derrière cette jonction doit rester intact. La jonction et sa cible sont retirées quoi qu'il arrive.
#
# Ce que cette étape PROUVE : avec une jonction en place du dossier, ni le désinstalleur ni `--oublier`
# ne touchent à ce qu'elle vise (mêmes noms, mêmes contenus), et les réglages et %APPDATA%\fr.shinku.app
# sont partis.
# Ce qu'elle NE PROUVE PAS : que le retour de l'effacement élevé serait vu. Elle ne l'attrape que si le
# `RMDir /r` de NSIS suit une jonction posée à la racine même du dossier (à vérifier dans NSIS) ; un
# lien plus bas dans l'arbre n'est pas essayé ; et si le contrôle de compte est coupé, le `--oublier`
# lancé par RunAsUser a lui-même tous les droits. Le retour de l'effacement élevé est tenu par
# tests/installer.test.ts (la case de Tauri remise à 0, les deux RmDir /r du modèle gardés).
function Etape-Desinstaller-Jonction {
    $desinstalleur = Join-Path $dossier 'uninstall.exe'
    Verifier-Existe $desinstalleur "Shinku (l'étape « installer » doit précéder)"
    Preparer-Etape
    Arreter-Shinku
    $compte = Compte-Courant
    $lien = $dossiersProfil[3]
    $cible = Join-Path $travail 'jonction-cible'
    Sauver-Profil
    try {
        Supprimer-Arbre $cible
        $temoins = @(
            (Join-Path $cible 'important.txt'),
            (Join-Path $cible 'sous-dossier\autre.txt'),
            (Join-Path $cible 'EBWebView\Default\temoin-local.txt')
        )
        foreach ($temoin in $temoins) { Poser-Fichier $temoin ('a ne pas toucher : ' + $temoin) }
        $avant = @(Lister $cible)

        Supprimer-Arbre $lien
        New-Item -ItemType Junction -Path $lien -Target $cible | Out-Null
        if (-not (Est-Lien $lien)) { Echouer "La jonction $lien n'a pas été créée." }
        Poser-Temoins
        # Poser-Temoins a écrit le témoin de %LOCALAPPDATA%\fr.shinku.app\... : il est derrière la jonction.
        $reglagesPoses = @(Chemins-Reglages)
        $arbreRoaming = Join-Path $env:APPDATA $identifiant

        Desinstaller-Avec-La-Case

        $apres = @(Lister $cible)
        if (($avant -join '|') -cne ($apres -join '|')) {
            Echouer ("Derrière la jonction $lien -> $cible, le désinstalleur a changé les éléments : avant (" + $avant.Count + ") " + ($avant -join ' ; ') + " ; après (" + $apres.Count + ") " + ($apres -join ' ; '))
        }
        foreach ($temoin in $temoins) {
            if (-not [IO.File]::Exists($temoin)) { Echouer "Le témoin derrière la jonction a disparu : $temoin" }
            if ([IO.File]::ReadAllText($temoin) -cne ('a ne pas toucher : ' + $temoin)) { Echouer "Le témoin derrière la jonction a changé : $temoin" }
        }
        $restants = @(Restants ($reglagesPoses + @($arbreRoaming)))
        if ($restants.Count -gt 0) { Echouer ('Il reste après la désinstallation : ' + ($restants -join ', ') + ' (' + (Decrire-Restes) + ')') }
        $sort = 'la jonction a été laissée en place'
        if (-not (Est-Lien $lien)) { $sort = 'la jonction a été retirée seule (sa cible est intacte)' }
        Noter "sort de la jonction après la désinstallation : $sort (la conception veut qu'un point d'analyse soit laissé en place ; ce n'est pas vérifié, seul le contenu derrière l'est)."
        Fait ("désinstallation avec la case, compte $compte, avec une jonction $lien -> $cible à la place de %LOCALAPPDATA%\$identifiant : les {0} éléments derrière la jonction sont intacts (mêmes noms, mêmes contenus), les {1} fichiers de réglages et %APPDATA%\$identifiant ont disparu ; $sort" -f $apres.Count, $reglagesPoses.Count)
    } finally {
        $script:Surveiller = $false
        try { Supprimer-Arbre $lien } catch { Noter "la jonction $lien n'a pas pu être retirée : $($_.Exception.Message)" }
        try { Supprimer-Arbre $cible } catch { Noter "$cible n'a pas pu être retiré : $($_.Exception.Message)" }
        try { Restaurer-Profil } catch {
            $script:ProfilNonRemis = $_.Exception.Message
            Noter "le profil n'a pas pu être remis en place : $($script:ProfilNonRemis)"
        }
    }
    if ($script:ProfilNonRemis -ne '') { Echouer "Le profil de la machine n'a pas pu être remis en place après l'essai : $($script:ProfilNonRemis)" }
    Attendre-Desinstalleur
}

# ── La désinstallation, case cochée, fenêtre de Shinku ouverte ─────────────

# Ouvre la vraie fenêtre de Shinku avec les droits ordinaires et attend qu'elle existe.
function Ouvrir-Shinku {
    $programme = Join-Path $dossier 'shinku.exe'
    $null = Lancer-En-Utilisateur ('start "" "{0}"' -f $programme) 'ouvrir'
    $limite = (Get-Date).AddSeconds(90)
    while ((Get-Date) -lt $limite) {
        $fenetre = Get-Process -Name 'shinku' -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } | Select-Object -First 1
        if ($null -ne $fenetre) {
            Dire "la fenêtre de Shinku est ouverte (PID $($fenetre.Id))"
            Start-Sleep -Seconds 3
            return $fenetre.Id
        }
        Start-Sleep -Milliseconds 500
    }
    Echouer "La fenêtre de Shinku ne s'est pas ouverte en 90 s."
}

function Etape-Desinstaller {
    $desinstalleur = Join-Path $dossier 'uninstall.exe'
    Verifier-Existe $desinstalleur "Shinku (l'étape « installer » doit précéder)"
    Preparer-Etape
    Arreter-Shinku
    $compte = Compte-Courant
    $ouvert = Ouvrir-Shinku
    # Les fichiers que Shinku vient d'écrire lui-même restent ; ceux qui manquent sont posés.
    Poser-Temoins
    $reglagesPoses = @(Chemins-Reglages | Where-Object { [IO.File]::Exists($_) })
    $arbresPoses = @(Chemins-Arbres)

    Desinstaller-Avec-La-Case

    if ($script:Questions -ne 1) {
        Echouer ("La question « Shinku est ouvert » a été posée $($script:Questions) fois, alors que la fenêtre était ouverte au départ : une seule fois attendue (nos crochets la posent avant --oublier, le modèle ne doit plus la rejouer).")
    }
    $restes = @(Get-Process -Name 'shinku' -ErrorAction SilentlyContinue)
    if ($restes.Count -gt 0) { Echouer "shinku.exe tourne encore après la désinstallation (PID $($restes[0].Id))." }
    $restants = @(Restants (@($dossiersProfil) + @(Chemins-Reglages) + $arbresPoses))
    if ($restants.Count -gt 0) {
        Echouer ('Il reste après la désinstallation : ' + ($restants -join ', ') + ". État du profil : " + (Decrire-Restes))
    }
    $restes = @(Raccourcis $menus)
    if ($restes.Count -gt 0) {
        Echouer ('Un raccourci du menu Démarrer est resté : ' + ($restes -join ', '))
    }
    # Aucun fichier ne doit revenir (la fenêtre fermée par le désinstalleur ne doit pas réécrire ses
    # réglages après coup) : on regarde encore après un délai.
    Start-Sleep -Seconds 5
    $revenus = @(Restants (@(Chemins-Reglages) + $arbresPoses + @($dossiersProfil)))
    if ($revenus.Count -gt 0) { Echouer ('Des fichiers sont revenus après la désinstallation : ' + ($revenus -join ', ')) }
    Fait ("désinstallation avec la case, compte $compte, fenêtre de Shinku ouverte au départ (PID $ouvert) : la question « Shinku est ouvert » est posée {0} fois ; %APPDATA%\Shinku, %LOCALAPPDATA%\Shinku, %APPDATA%\$identifiant et %LOCALAPPDATA%\$identifiant ont disparu, avec {1} fichiers de réglages et de journaux ; rien n'est revenu 5 s plus tard ; le dossier d'installation et le raccourci du menu Démarrer ont disparu" -f $script:Questions, $reglagesPoses.Count)
    Attendre-Desinstalleur
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
        'oublier' { Etape-Oublier }
        'desinstaller-jonction' { Etape-Desinstaller-Jonction }
        'desinstaller' { Etape-Desinstaller }
    }
    Dire "✓ étape « $Etape » réussie"
    exit 0
} catch {
    Dire ("✗ ÉCHEC : " + $_.Exception.Message)
    Photographier-Ecran ('echec-' + $Etape)
    exit 1
}
