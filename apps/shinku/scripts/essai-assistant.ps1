# Essai de l'assistant élevé du Démarrage, shinku-helper.exe, installé.
#
# L'assistant règle, pour tous les comptes du PC, l'approbation d'une entrée de
# HKLM\...\Run : il pose une question à la personne (une boîte « Shinku »),
# puis n'écrit qu'une valeur sous
# HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run.
# Cet essai l'exerce pour de bon sur « Essai Shinku PC », l'entrée que
# essai-demarrage.ps1 -Preparer pose dans HKLM\...\Run, et prouve, dans l'ordre :
#
#   - la machine et l'exécutable : droits (EnableLUA, ConsentPromptBehavior*,
#     icacls), manifeste `requireAdministrator`, sous-système graphique ;
#   - le drapeau de chargement des bibliothèques du binaire installé
#     (DependentLoadFlags = 0x0800, lu dans le fichier PE) ;
#   - demandes refusées (code 2, aucune fenêtre), nom inconnu ou autre vue
#     (code 4), copie hors de Program Files (code 6) ;
#   - un refus d'emplacement causé par une entrée de droits : une copie dans un
#     dossier créé sous Program Files est acceptée (code 4), refusée dès que
#     Utilisateurs peut écrire dans ce dossier (code 6), acceptée de nouveau
#     quand l'entrée est retirée (code 4) ;
#   - la boîte, trouvée par le numéro du processus (classe #32770), son texte
#     contient le nom ; Annuler (WM_COMMAND, IDCANCEL) : code 3, rien ne change ;
#   - l'essai négatif : un processus sans droits (lancer-en-utilisateur.ps1)
#     envoie le même message à la boîte, Windows le refuse ; l'assistant attend
#     toujours, HKLM n'a pas changé ;
#   - OK (IDOK) : code 0, les douze octets de « coupée » ; puis « déjà coupée » :
#     code 9, sans fenêtre ; remettre : code 0, `02` et onze zéros ; remettre
#     encore : code 9 (le code 1 est celui d'un processus tué : l'assistant ne le
#     rend jamais) ;
#   - aucune fenêtre de console ;
#   - l'échéance : un assistant lancé au début sur une seconde entrée d'essai
#     que ce script pose lui-même, sans jamais lui répondre, sort avec le code 3
#     entre 118 et 150 secondes après son lancement, sans rien écrire.
#
# Prudence : chaque attente a un délai ; le script sort avec un code non nul à
# la première preuve manquée, en disant laquelle ; chaque preuve réussie écrit
# une ligne qui commence par « FAIT : ». Quoi qu'il arrive (bloc finally), tout
# shinku-helper.exe encore en marche est arrêté, l'approbation de « Essai Shinku
# PC » retrouve son état d'avant, et la seconde entrée d'essai disparaît.
#
# Les machines de l'intégration continue tournent en administrateur (jeton
# complet) : l'assistant se lance donc sans demande d'accord de Windows. Sur un
# PC ordinaire, ce script refuse de tourner (il faut un jeton élevé).
#
#   powershell -File scripts/essai-assistant.ps1
#   powershell -File scripts/essai-assistant.ps1 -Nettoyer
#
# -Nettoyer : arrête tout assistant et retire la seconde entrée d'essai (pour
# l'étape « toujours » de l'intégration continue) ; l'approbation de « Essai
# Shinku PC » est retirée par essai-demarrage.ps1 -Nettoyer.
#
# Ce fichier commence par une marque d'ordre des octets (BOM) : sans elle,
# Windows PowerShell 5 lirait ses lettres accentuées de travers.

param(
    # L'assistant installé.
    [string]$Assistant = '',
    # L'entrée du PC posée par essai-demarrage.ps1 -Preparer.
    [string]$Entree = 'Essai Shinku PC',
    # La seconde entrée, posée et retirée par ce script, pour l'échéance.
    [string]$EntreeEcheance = 'Essai Shinku Echeance',
    # Temps laissé à l'ouverture d'une boîte ou à la fin d'un assistant, en secondes.
    [int]$Attente = 30,
    [switch]$Nettoyer
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }

$ProgramFiles = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
if (-not $Assistant) { $Assistant = Join-Path $ProgramFiles 'Shinku\shinku-helper.exe' }

$RunKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Run'
$Run32Key = 'SOFTWARE\Wow6432Node\Microsoft\Windows\CurrentVersion\Run'
$ApprovedRunKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
$ApprovedRun32Key = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'
$ApprovedFolderKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder'
$PoliciesKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
$EchoCommand = '"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -Command exit'
$LauncherScript = Join-Path $PSScriptRoot 'lancer-en-utilisateur.ps1'
$Work = Join-Path $env:TEMP 'essai-assistant'
$CopyDir = Join-Path $Work 'copie'
# L'essai négatif tourne avec un jeton filtré : il lit et écrit dans le dossier
# temporaire du compte, pas dans celui de l'intégration continue.
$ProbeDir = Join-Path $env:TEMP 'essai-assistant-sonde'
# Un dossier créé sous le vrai Program Files, pour l'essai des droits.
$RightsDir = Join-Path $ProgramFiles 'Shinku Essai Droits'

$Started = Get-Date
function Say([string]$Line) {
    $seconds = ((Get-Date) - $Started).TotalSeconds
    Write-Host ('[{0,6:N1} s] {1}' -f $seconds, $Line)
}
function Done([string]$Line) { Say "OK $Line" }
# Une preuve réussie : la ligne commence par « FAIT : », sans autre préfixe.
function Proof([string]$Line) { Write-Host "FAIT : $Line" }
function Fail([string]$Message) { throw $Message }

# ── Windows : fenêtres et messages ─────────────────────────────────────────

# Le même texte est compilé ici et dans le processus sans droits (par un
# fichier : il ne peut pas être inclus dans un second bloc de texte).
$FenetresSource = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class Reponse
{
    public bool Succes;
    public int Erreur;
    public long Valeur;
}

public class Fenetre
{
    public long Handle;
    public string Classe;
    public string Titre;
    public bool Visible;
    public string[] Textes;
    public string Enfants;
    public long Ok;
    public long Annuler;
}

public static class Fenetres
{
    delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);

    [DllImport("user32.dll")]
    static extern bool EnumWindows(EnumProc proc, IntPtr lparam);

    [DllImport("user32.dll")]
    static extern bool EnumChildWindows(IntPtr parent, EnumProc proc, IntPtr lparam);

    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassName(IntPtr hwnd, StringBuilder text, int max);

    [DllImport("user32.dll")]
    static extern bool IsWindowVisible(IntPtr hwnd);

    [DllImport("user32.dll")]
    static extern bool IsWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    static extern IntPtr GetDlgItem(IntPtr hwnd, int id);

    // Le texte d'une fenetre d'un autre processus se demande par WM_GETTEXT :
    // GetWindowText ne le rend pas pour les controles.
    [DllImport("user32.dll", EntryPoint = "SendMessageTimeoutW", CharSet = CharSet.Unicode)]
    static extern IntPtr SendText(IntPtr hwnd, uint msg, IntPtr wparam, StringBuilder lparam,
        uint flags, uint timeout, out IntPtr result);

    [DllImport("user32.dll", EntryPoint = "SendMessageTimeoutW", SetLastError = true)]
    static extern IntPtr SendCommand(IntPtr hwnd, uint msg, IntPtr wparam, IntPtr lparam,
        uint flags, uint timeout, out IntPtr result);

    [DllImport("user32.dll", EntryPoint = "PostMessageW", SetLastError = true)]
    static extern bool PostCommand(IntPtr hwnd, uint msg, IntPtr wparam, IntPtr lparam);

    const uint WM_GETTEXT = 0x000D;
    const uint SMTO_ABORTIFHUNG = 0x0002;

    static string ClassOf(IntPtr hwnd)
    {
        StringBuilder text = new StringBuilder(256);
        GetClassName(hwnd, text, text.Capacity);
        return text.ToString();
    }

    static string TextOf(IntPtr hwnd)
    {
        StringBuilder text = new StringBuilder(4096);
        IntPtr result;
        IntPtr ok = SendText(hwnd, WM_GETTEXT, (IntPtr)text.Capacity, text, SMTO_ABORTIFHUNG, 1000, out result);
        return ok == IntPtr.Zero ? "" : text.ToString();
    }

    // Les fenetres de premier niveau d'un processus, avec le texte de leurs
    // controles Static (celui d'une boite de message) et leurs boutons 1 et 2.
    public static List<Fenetre> Of(uint pid)
    {
        List<Fenetre> found = new List<Fenetre>();
        EnumWindows(delegate (IntPtr hwnd, IntPtr unused)
        {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            if (owner != pid)
            {
                return true;
            }
            Fenetre window = new Fenetre();
            window.Handle = hwnd.ToInt64();
            window.Classe = ClassOf(hwnd);
            window.Titre = TextOf(hwnd);
            window.Visible = IsWindowVisible(hwnd);
            List<string> texts = new List<string>();
            List<string> children = new List<string>();
            EnumChildWindows(hwnd, delegate (IntPtr child, IntPtr unused2)
            {
                string kind = ClassOf(child);
                children.Add(kind);
                if (kind == "Static")
                {
                    string text = TextOf(child);
                    if (text.Length > 0)
                    {
                        texts.Add(text);
                    }
                }
                return true;
            }, IntPtr.Zero);
            window.Textes = texts.ToArray();
            window.Enfants = string.Join(",", children.ToArray());
            window.Ok = GetDlgItem(hwnd, 1).ToInt64();
            window.Annuler = GetDlgItem(hwnd, 2).ToInt64();
            found.Add(window);
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static bool Exists(long hwnd)
    {
        return IsWindow((IntPtr)hwnd);
    }

    // Envoie un message et attend sa prise en compte (cinq secondes au plus).
    public static Reponse Send(long hwnd, uint msg, long wparam, long lparam)
    {
        IntPtr result;
        IntPtr ok = SendCommand((IntPtr)hwnd, msg, (IntPtr)wparam, (IntPtr)lparam, SMTO_ABORTIFHUNG, 5000, out result);
        Reponse answer = new Reponse();
        answer.Erreur = Marshal.GetLastWin32Error();
        answer.Succes = ok != IntPtr.Zero;
        answer.Valeur = result.ToInt64();
        return answer;
    }

    // Poste un message sans attendre.
    public static Reponse Post(long hwnd, uint msg, long wparam, long lparam)
    {
        bool ok = PostCommand((IntPtr)hwnd, msg, (IntPtr)wparam, (IntPtr)lparam);
        Reponse answer = new Reponse();
        answer.Erreur = Marshal.GetLastWin32Error();
        answer.Succes = ok;
        return answer;
    }
}
'@

# Ce que fait, sans droits, le processus de l'essai négatif : il envoie à la
# boîte le message que l'essai lui envoie avec les droits (WM_COMMAND, IDCANCEL),
# de deux façons, plus WM_CLOSE, et écrit ce que Windows en a rendu.
$ProbeScript = @'
param([long]$Fenetre, [long]$Bouton, [string]$Source, [string]$Sortie)
$ErrorActionPreference = 'Stop'
$lines = New-Object System.Collections.ArrayList
try {
    Add-Type -TypeDefinition ([IO.File]::ReadAllText($Source))
    $who = (& whoami.exe /groups | Out-String)
    $level = 'inconnu'
    if ($who -match 'S-1-16-8192') { $level = 'moyen' }
    elseif ($who -match 'S-1-16-12288') { $level = 'haut' }
    [void]$lines.Add("INTEGRITE=$level")
    $send = [Fenetres]::Send($Fenetre, 0x111, 2, $Bouton)
    [void]$lines.Add(('ENVOI_SUCCES={0}' -f $send.Succes))
    [void]$lines.Add(('ENVOI_ERREUR={0}' -f $send.Erreur))
    $post = [Fenetres]::Post($Fenetre, 0x111, 2, $Bouton)
    [void]$lines.Add(('POSTE_SUCCES={0}' -f $post.Succes))
    [void]$lines.Add(('POSTE_ERREUR={0}' -f $post.Erreur))
    $close = [Fenetres]::Post($Fenetre, 0x10, 0, 0)
    [void]$lines.Add(('FERMER_SUCCES={0}' -f $close.Succes))
    [void]$lines.Add(('FERMER_ERREUR={0}' -f $close.Erreur))
    [void]$lines.Add('FINI=oui')
} catch {
    [void]$lines.Add('EXCEPTION=' + $_.Exception.Message)
}
[IO.File]::WriteAllLines($Sortie, [string[]]$lines)
'@

# ── Le registre, toujours dans la vue 64 bits de HKLM ──────────────────────

function Open-Hklm([string]$SubKey, [bool]$Writable) {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, [Microsoft.Win32.RegistryView]::Registry64)
    try { return $base.OpenSubKey($SubKey, $Writable) } finally { $base.Close() }
}

function Format-Value($Value) {
    if ($null -eq $Value) { return '' }
    if ($Value -is [byte[]]) { return [BitConverter]::ToString($Value) }
    if ($Value -is [string[]]) { return ($Value -join ';') }
    return [string]$Value
}

# Une valeur : $null si elle n'existe pas, sinon son type, sa donnée et son texte.
function Read-Value([string]$SubKey, [string]$Name) {
    $key = Open-Hklm $SubKey $false
    if ($null -eq $key) { return $null }
    try {
        if (@($key.GetValueNames()) -notcontains $Name) { return $null }
        $kind = $key.GetValueKind($Name)
        $value = $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        return [pscustomobject]@{ Kind = $kind; Value = $value; Text = ('{0}:{1}' -f $kind, (Format-Value $value)) }
    } finally { $key.Close() }
}

function Write-Value([string]$SubKey, [string]$Name, $Value, $Kind) {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, [Microsoft.Win32.RegistryView]::Registry64)
    try {
        $key = $base.CreateSubKey($SubKey)
        try { $key.SetValue($Name, $Value, $Kind) } finally { $key.Close() }
    } finally { $base.Close() }
}

function Remove-HklmValue([string]$SubKey, [string]$Name) {
    $key = Open-Hklm $SubKey $true
    if ($null -eq $key) { return }
    try {
        if (@($key.GetValueNames()) -contains $Name) { $key.DeleteValue($Name, $false) }
    } finally { $key.Close() }
}

# Tout ce que les cinq clés de Démarrage contiennent, pour prouver que rien
# d'autre n'a bougé : « place|nom » vers « type:données ».
function Get-Snapshot {
    $result = @{}
    $places = @(
        @('run', $RunKey), @('run32', $Run32Key),
        @('approuve-run', $ApprovedRunKey), @('approuve-run32', $ApprovedRun32Key),
        @('approuve-dossier', $ApprovedFolderKey))
    foreach ($place in $places) {
        $key = Open-Hklm $place[1] $false
        if ($null -eq $key) { continue }
        try {
            foreach ($name in @($key.GetValueNames())) {
                $kind = $key.GetValueKind($name)
                $value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                $result[('{0}|{1}' -f $place[0], $name)] = ('{0}:{1}' -f $kind, (Format-Value $value))
            }
        } finally { $key.Close() }
    }
    return $result
}

# Les différences entre deux instantanés, hors celles que l'étape permet.
function Compare-Snapshots($Before, $After, [string[]]$Allowed) {
    $differences = New-Object System.Collections.ArrayList
    $names = @($Before.Keys) + @($After.Keys) | Sort-Object -Unique
    foreach ($name in $names) {
        if ($Allowed -contains $name) { continue }
        $was = '(absente)'
        $is = '(absente)'
        if ($Before.ContainsKey($name)) { $was = $Before[$name] }
        if ($After.ContainsKey($name)) { $is = $After[$name] }
        if ($was -ne $is) { [void]$differences.Add("$name : $was devenue $is") }
    }
    return @($differences)
}

function Assert-Unchanged([string]$What, $Before, [string[]]$Allowed) {
    $now = Get-Snapshot
    $differences = @(Compare-Snapshots $Before $now $Allowed)
    if ($differences.Count -gt 0) { Fail "$What : HKLM a changé ($($differences -join ' ; '))" }
}

# Le nom d'une entrée tel que la demande le porte : UTF-16, quatre chiffres
# hexadécimaux minuscules par unité.
function ConvertTo-HexName([string]$Name) {
    $parts = foreach ($unit in $Name.ToCharArray()) { '{0:x4}' -f [int]$unit }
    return ($parts -join '')
}

# ── Les processus ──────────────────────────────────────────────────────────

$Processes = New-Object System.Collections.ArrayList

function Stop-Helpers {
    foreach ($process in @($Processes)) {
        try { if (-not $process.HasExited) { $process.Kill() } } catch { }
    }
    # Tout shinku-helper.exe de la machine, quel qu'en soit le lanceur.
    foreach ($process in @(Get-Process -Name 'shinku-helper' -ErrorAction SilentlyContinue)) {
        try { $process.Kill() } catch { }
    }
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline -and @(Get-Process -Name 'shinku-helper' -ErrorAction SilentlyContinue).Count -gt 0) {
        Start-Sleep -Milliseconds 200
    }
}

function Start-Helper([string]$Arguments, [string]$Program = '') {
    if (-not $Program) { $Program = $Assistant }
    if ($Arguments) {
        $process = Start-Process -FilePath $Program -ArgumentList $Arguments -PassThru
    } else {
        $process = Start-Process -FilePath $Program -PassThru
    }
    # Sans cela, ExitCode peut rester vide après la fin d'un processus lancé avec -PassThru.
    $null = $process.Handle
    [void]$Processes.Add($process)
    return $process
}

function Wait-Exit($Process, [int]$Seconds, [string]$What) {
    if (-not $Process.WaitForExit($Seconds * 1000)) {
        Fail "$What : l'assistant (processus $($Process.Id)) ne sort pas en $Seconds s"
    }
    if ($null -eq $Process.ExitCode) { Fail "$What : le code de sortie de l'assistant est illisible" }
    return [int]$Process.ExitCode
}

# Lance l'assistant pour une demande qui ne doit ouvrir aucune boîte, et rend son
# code de sortie. Tant qu'il tourne, ses fenêtres visibles sont relevées : il ne
# doit en avoir aucune.
function Invoke-Quick([string]$Arguments, [string]$Program = '') {
    $process = Start-Helper $Arguments $Program
    $seen = New-Object System.Collections.ArrayList
    $deadline = (Get-Date).AddSeconds($Attente)
    while (-not $process.HasExited) {
        if ((Get-Date) -gt $deadline) {
            try { $process.Kill() } catch { }
            $what = 'aucune fenêtre vue'
            if ($seen.Count -gt 0) { $what = "fenêtre vue : $((@($seen) | Select-Object -Unique) -join ' ; ')" }
            Fail "la demande « $Arguments » ne rend pas la main en $Attente s ($what)"
        }
        foreach ($window in @([Fenetres]::Of([uint32]$process.Id))) {
            if ($window.Visible) { [void]$seen.Add(('classe {0} titre « {1} »' -f $window.Classe, $window.Titre)) }
        }
        Start-Sleep -Milliseconds 40
    }
    $null = $process.WaitForExit()
    if ($null -eq $process.ExitCode) { Fail "le code de sortie de « $Arguments » est illisible" }
    return [pscustomobject]@{ Code = [int]$process.ExitCode; Windows = @($seen) }
}

function Assert-Quick([string]$Arguments, [int[]]$Codes, [string]$What, [string]$Program = '') {
    $run = Invoke-Quick $Arguments $Program
    if ($Codes -notcontains $run.Code) {
        Fail "$What : code $($run.Code) au lieu de $($Codes -join ' ou ') (demande « $Arguments »)"
    }
    if ($run.Windows.Count -gt 0) {
        Fail "$What : une fenêtre s'est ouverte ($($run.Windows -join ' ; ')) alors qu'aucune question n'avait lieu d'être"
    }
    return $run.Code
}

# Attend la boîte de l'assistant : fenêtre visible de classe #32770 du processus.
function Find-Dialog($Process, [int]$Seconds, [string]$What) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        foreach ($window in @([Fenetres]::Of([uint32]$Process.Id))) {
            if ($window.Visible -and $window.Classe -eq '#32770') { return $window }
        }
        if ($Process.HasExited) {
            Fail "$What : l'assistant est sorti avec le code $($Process.ExitCode) sans ouvrir de boîte"
        }
        Start-Sleep -Milliseconds 100
    }
    Fail "$What : aucune boîte (classe #32770) du processus $($Process.Id) en $Seconds s"
}

# La boîte est celle qu'on attend : titre, début du texte (le verbe et le nom),
# place, deux boutons.
function Assert-Dialog($Window, [string]$Verb, [string]$Name, [string]$What) {
    $text = ($Window.Textes -join ' / ')
    Say "  boîte : titre « $($Window.Titre) », texte « $text », enfants $($Window.Enfants)"
    if ($Window.Titre -ne 'Shinku') { Fail "$What : le titre de la boîte est « $($Window.Titre) » au lieu de « Shinku »" }
    if (-not $text.Contains($Name)) { Fail "$What : le texte de la boîte ne contient pas le nom « $Name » : « $text »" }
    # Les textes sont ceux de question_text et place_text (helper/startup.rs).
    $start = "$Verb « $Name »"
    if (-not $text.Contains($start)) { Fail "$What : le texte de la boîte ne contient pas « $start » : « $text »" }
    if (-not $text.Contains('Run (64 bits)')) { Fail "$What : le texte de la boîte ne nomme pas la place « Run (64 bits) » : « $text »" }
    if ($Window.Ok -eq 0 -or $Window.Annuler -eq 0) { Fail "$What : la boîte n'a pas ses boutons OK et Annuler" }
}

# Répond à la boîte comme le ferait un clic : WM_COMMAND, avec l'identifiant du bouton.
function Answer-Dialog($Window, [int]$Id, [string]$What) {
    $button = $Window.Annuler
    if ($Id -eq 1) { $button = $Window.Ok }
    $answer = [Fenetres]::Send($Window.Handle, 0x111, $Id, $button)
    if (-not $answer.Succes) { Fail "$What : WM_COMMAND $Id refusé à la boîte (GetLastError $($answer.Erreur))" }
}

# ── Ce que la machine dit, pour le compte rendu ────────────────────────────

function Show-Machine {
    Say 'UAC (HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System) :'
    foreach ($name in @('EnableLUA', 'ConsentPromptBehaviorAdmin', 'ConsentPromptBehaviorUser', 'PromptOnSecureDesktop')) {
        $read = Read-Value $PoliciesKey $name
        $shown = '(absente)'
        if ($null -ne $read) { $shown = Format-Value $read.Value }
        Say "  $name = $shown"
    }
    $lsa = Read-Value 'SYSTEM\CurrentControlSet\Control\Lsa' 'nodefaultadminowner'
    $lsaShown = '(absente)'
    if ($null -ne $lsa) { $lsaShown = Format-Value $lsa.Value }
    Say "  HKLM\SYSTEM\CurrentControlSet\Control\Lsa\nodefaultadminowner = $lsaShown"
    $paths = @(
        $ProgramFiles,
        (Split-Path -Parent $Assistant),
        $Assistant,
        [Environment]::GetFolderPath('CommonStartup'))
    foreach ($path in $paths) {
        Say "icacls $path :"
        try { Say "  propriétaire : $((Get-Acl -LiteralPath $path).Owner)" } catch { Say "  propriétaire illisible : $($_.Exception.Message)" }
        try {
            foreach ($line in @(& icacls.exe $path)) { if ("$line".Trim()) { Say "  $line" } }
        } catch { Say "  icacls impossible : $($_.Exception.Message)" }
    }
}

function Test-Elevated {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { return $false }
    return ((& whoami.exe /groups | Out-String) -match 'S-1-16-12288')
}

# ── Nettoyage ──────────────────────────────────────────────────────────────

# Les deux états d'origine de « Essai Shinku PC », notés avant tout changement.
$Original = $null
$OriginalNoted = $false

# Ce que dit la machine des droits d'un objet : propriétaire, puis icacls.
function Get-RightsText([string]$Path) {
    $parts = New-Object System.Collections.ArrayList
    try { [void]$parts.Add("propriétaire : $((Get-Acl -LiteralPath $Path).Owner)") } catch { [void]$parts.Add("propriétaire illisible : $($_.Exception.Message)") }
    try {
        foreach ($line in @(& icacls.exe $Path)) { if ("$line".Trim()) { [void]$parts.Add("$line".Trim().Replace($Path, '<objet>')) } }
    } catch { [void]$parts.Add("icacls impossible : $($_.Exception.Message)") }
    return ($parts -join ' | ')
}

# Les entrées de droits d'un objet, sans son chemin ni le bilan d'icacls, triées.
function Get-AclLines([string]$Path) {
    $lines = New-Object System.Collections.ArrayList
    foreach ($line in @(& icacls.exe $Path)) {
        $text = "$line".Replace($Path, '').Trim()
        if ($text -and $text -notmatch '^(Successfully|Failed processing|Traitement)') { [void]$lines.Add($text) }
    }
    return @($lines | Sort-Object)
}

# Lance l'assistant depuis la copie du dossier d'essai des droits.
function Assert-RightsCase([int]$Expected, [string]$Step, [string]$Name, [string]$Program) {
    $run = Invoke-Quick "demarrage1 couper run $(ConvertTo-HexName $Name)" $Program
    if ($run.Windows.Count -gt 0) {
        Fail "droits, étape $Step : une fenêtre s'est ouverte ($($run.Windows -join ' ; '))"
    }
    if ($run.Code -ne $Expected) {
        Fail ("droits, étape $Step : code $($run.Code) au lieu de $Expected. Dossier : $(Get-RightsText $RightsDir). Fichier : $(Get-RightsText $Program)")
    }
}

function Remove-RightsDir {
    if (Test-Path -LiteralPath $RightsDir) { Remove-Item -LiteralPath $RightsDir -Recurse -Force -ErrorAction Stop }
}

function Remove-Echo {
    Remove-HklmValue $RunKey $EntreeEcheance
    Remove-HklmValue $ApprovedRunKey $EntreeEcheance
}

# Chaque geste a son propre essai : un geste raté ne retient pas les suivants. La
# première erreur est relancée à la fin, pour que le script sorte en échec.
function Clean-Up {
    $first = $null
    try { Stop-Helpers } catch { if ($null -eq $first) { $first = $_ }; Say "  arrêt des assistants impossible : $($_.Exception.Message)" }
    try { Remove-Echo } catch { if ($null -eq $first) { $first = $_ }; Say "  retrait de « $EntreeEcheance » impossible : $($_.Exception.Message)" }
    if ($OriginalNoted) {
        try {
            if ($null -eq $Original) {
                Remove-HklmValue $ApprovedRunKey $Entree
            } else {
                Write-Value $ApprovedRunKey $Entree $Original.Value $Original.Kind
            }
            $back = Read-Value $ApprovedRunKey $Entree
            $backText = '(absente)'
            if ($null -ne $back) { $backText = $back.Text }
            $wasText = '(absente)'
            if ($null -ne $Original) { $wasText = $Original.Text }
            if ($backText -ne $wasText) { Fail "l'approbation de « $Entree » n'a pas été remise : $backText au lieu de $wasText" }
            Say "approbation de « $Entree » remise dans son état d'avant : $wasText"
        } catch { if ($null -eq $first) { $first = $_ }; Say "  remise de l'approbation impossible : $($_.Exception.Message)" }
    }
    foreach ($dir in @($Work, $ProbeDir, $RightsDir)) {
        try { if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction Stop } } catch {
            Say "  impossible de retirer $dir : $($_.Exception.Message)"
        }
    }
    if ($null -ne $first) { throw $first }
}

if ($Nettoyer) {
    Stop-Helpers
    Remove-Echo
    try { Remove-RightsDir } catch { Say "  impossible de retirer $RightsDir : $($_.Exception.Message)" }
    Say "assistants arrêtés, entrée « $EntreeEcheance » retirée, dossier $RightsDir retiré"
    exit 0
}

# Pas avant : le nettoyage n'en a pas besoin, et une compilation ratée ne doit
# pas l'empêcher.
Add-Type -TypeDefinition $FenetresSource

# ── L'essai négatif : un processus sans droits ─────────────────────────────

# Lance un petit script PowerShell avec le jeton filtré (lancer-en-utilisateur.ps1)
# et attend ce qu'il a écrit.
function Invoke-Unprivileged([long]$Window, [long]$Button) {
    New-Item -ItemType Directory -Force -Path $ProbeDir | Out-Null
    $scriptPath = Join-Path $ProbeDir 'sonde.ps1'
    $sourcePath = Join-Path $ProbeDir 'fenetres.cs'
    $outPath = Join-Path $ProbeDir 'sonde.txt'
    $launcherOut = Join-Path $ProbeDir 'lanceur.txt'
    Remove-Item -Force -ErrorAction SilentlyContinue $outPath, $launcherOut, "$launcherOut.err"
    $ascii = New-Object System.Text.UTF8Encoding $false
    [IO.File]::WriteAllText($scriptPath, $ProbeScript, $ascii)
    [IO.File]::WriteAllText($sourcePath, $FenetresSource, $ascii)

    # Par un fichier .cmd lancé par `cmd /c`, comme Invoke-AsUser de
    # essai-mode-jeu.ps1 : la valeur de -Arguments ne commence pas par un tiret
    # (« powershell -File » la prendrait pour un nom de paramètre).
    $cmdPath = Join-Path $ProbeDir 'sonde.cmd'
    $psExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $lines = @('@echo off',
        ('"{0}" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0sonde.ps1" -Fenetre {1} -Bouton {2} -Source "%~dp0fenetres.cs" -Sortie "%~dp0sonde.txt"' -f $psExe, $Window, $Button),
        'exit /b %errorlevel%')
    [IO.File]::WriteAllText($cmdPath, (($lines -join "`r`n") + "`r`n"), $ascii)
    $cmdExe = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $parameters = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $LauncherScript,
        '-Programme', $cmdExe, '-Arguments', ('/c ""{0}""' -f $cmdPath))
    # Start-Process recolle la liste avec des espaces, sans guillemets : un
    # argument qui en contient porte les siens, et ses guillemets sont précédés
    # d'une barre oblique inverse.
    $parameters = @($parameters | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } })
    $launcher = Start-Process -FilePath 'powershell' -ArgumentList $parameters -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $launcherOut -RedirectStandardError "$launcherOut.err"
    $null = $launcher.Handle
    [void]$Processes.Add($launcher)
    if (-not $launcher.WaitForExit(90000)) {
        # Le programme lancé (cmd, puis la sonde) est l'enfant du lanceur : son
        # numéro est la première ligne écrite, « PID <numéro> ».
        try {
            foreach ($line in @(Get-Content -LiteralPath $launcherOut -ErrorAction SilentlyContinue)) {
                if ("$line" -match '^PID (\d+)') { Get-CimInstance Win32_Process -Filter "ParentProcessId=$($Matches[1])" -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; Stop-Process -Id ([int]$Matches[1]) -Force -ErrorAction SilentlyContinue }
            }
        } catch { }
        try { $launcher.Kill() } catch { }
        Fail "l'essai négatif : le processus sans droits ne finit pas en 90 s"
    }
    if (-not (Test-Path -LiteralPath $outPath)) {
        $why = ''
        if (Test-Path "$launcherOut.err") { $why = (Get-Content "$launcherOut.err") -join ' ' }
        Fail "l'essai négatif : le processus sans droits n'a rien écrit (lanceur : code $($launcher.ExitCode) $why)"
    }
    $result = @{}
    foreach ($line in @(Get-Content -LiteralPath $outPath)) {
        $at = "$line".IndexOf('=')
        if ($at -gt 0) { $result["$line".Substring(0, $at)] = "$line".Substring($at + 1) }
    }
    return $result
}

# ── Les vérifications du fichier ───────────────────────────────────────────

function Assert-Executable {
    if (-not (Test-Path -LiteralPath $Assistant)) { Fail "l'assistant n'est pas installé : $Assistant est introuvable" }
    $bytes = [IO.File]::ReadAllBytes($Assistant)
    $peAt = [BitConverter]::ToInt32($bytes, 0x3C)
    if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A -or $bytes[$peAt] -ne 0x50 -or $bytes[$peAt + 1] -ne 0x45) {
        Fail "$Assistant n'est pas un exécutable Windows"
    }
    # Le sous-système est à 68 octets du début de l'en-tête optionnel (PE32 et PE32+).
    $subsystem = [BitConverter]::ToUInt16($bytes, $peAt + 24 + 68)
    if ($subsystem -ne 2) { Fail "le sous-système de l'assistant est $subsystem au lieu de 2 (graphique) : il ouvrirait une console" }
    $text = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    if (-not $text.Contains('level="requireAdministrator"')) { Fail "le manifeste de l'assistant ne contient pas level=\"requireAdministrator\"" }
    Proof "le manifeste de l'assistant contient requireAdministrator et son sous-système est graphique (2), $($bytes.Length) octets"
    Assert-LoadFlags $bytes $peAt
}

# De l'adresse relative d'un exécutable à sa place dans le fichier, par la table
# des sections.
function ConvertTo-FileOffset([byte[]]$Bytes, [long]$SectionsAt, [int]$Count, [long]$Rva) {
    for ($i = 0; $i -lt $Count; $i++) {
        $at = $SectionsAt + 40 * $i
        $virtualSize = [long][BitConverter]::ToUInt32($Bytes, $at + 8)
        $virtualAddress = [long][BitConverter]::ToUInt32($Bytes, $at + 12)
        $rawSize = [long][BitConverter]::ToUInt32($Bytes, $at + 16)
        $rawAt = [long][BitConverter]::ToUInt32($Bytes, $at + 20)
        $span = [Math]::Max($virtualSize, $rawSize)
        if ($Rva -ge $virtualAddress -and $Rva -lt ($virtualAddress + $span)) { return ($Rva - $virtualAddress + $rawAt) }
    }
    Fail ("l'adresse relative 0x{0:X} n'est dans aucune section du fichier" -f $Rva)
}

# Le drapeau de chargement des bibliothèques (éditeur de liens :
# /DEPENDENTLOADFLAG:0x800) est dans la table « Load Config » : répertoire de
# données n° 10, mot de 16 bits au décalage 0x4E de IMAGE_LOAD_CONFIG_DIRECTORY64.
function Assert-LoadFlags([byte[]]$Bytes, [int]$PeAt) {
    $optional = $PeAt + 24
    $magic = [BitConverter]::ToUInt16($Bytes, $optional)
    if ($magic -ne 0x20B) { Fail ("l'en-tête optionnel de l'assistant a le magic 0x{0:X} au lieu de 0x20B (PE32+)" -f $magic) }
    $sectionCount = [int][BitConverter]::ToUInt16($Bytes, $PeAt + 6)
    $optionalSize = [long][BitConverter]::ToUInt16($Bytes, $PeAt + 20)
    $sectionsAt = $optional + $optionalSize
    $directories = [long][BitConverter]::ToUInt32($Bytes, $optional + 108)
    if ($directories -lt 11) { Fail "l'assistant n'a que $directories répertoires de données : pas de table Load Config" }
    $loadRva = [long][BitConverter]::ToUInt32($Bytes, $optional + 112 + 8 * 10)
    $loadSize = [long][BitConverter]::ToUInt32($Bytes, $optional + 112 + 8 * 10 + 4)
    if ($loadRva -eq 0 -or $loadSize -eq 0) { Fail 'le répertoire Load Config de l''assistant est absent : aucun DependentLoadFlags' }
    $loadAt = ConvertTo-FileOffset $Bytes $sectionsAt $sectionCount $loadRva
    $structureSize = [long][BitConverter]::ToUInt32($Bytes, $loadAt)
    if ($structureSize -lt 0x50) { Fail ("la table Load Config de l'assistant fait {0} octets : elle n'atteint pas DependentLoadFlags (décalage 0x4E)" -f $structureSize) }
    $flags = [BitConverter]::ToUInt16($Bytes, $loadAt + 0x4E)
    if ($flags -ne 0x0800) { Fail ("DependentLoadFlags de l'assistant vaut 0x{0:X4} au lieu de 0x0800" -f $flags) }
    Proof "l'assistant installé porte DependentLoadFlags = 0x0800 : ses bibliothèques liées ne se chargent que depuis System32"

    # Pour information, pas une preuve : les bibliothèques de sa table d'importation.
    try {
        $importRva = [long][BitConverter]::ToUInt32($Bytes, $optional + 112 + 8)
        $names = New-Object System.Collections.ArrayList
        if ($importRva -ne 0) {
            $entry = ConvertTo-FileOffset $Bytes $sectionsAt $sectionCount $importRva
            for ($i = 0; $i -lt 500; $i++) {
                $at = $entry + 20 * $i
                $nameRva = [long][BitConverter]::ToUInt32($Bytes, $at + 12)
                $thunk = [long][BitConverter]::ToUInt32($Bytes, $at + 16)
                if ($nameRva -eq 0 -and $thunk -eq 0) { break }
                $nameAt = ConvertTo-FileOffset $Bytes $sectionsAt $sectionCount $nameRva
                $end = $nameAt
                while ($end -lt $Bytes.Length -and $Bytes[$end] -ne 0) { $end++ }
                [void]$names.Add([Text.Encoding]::ASCII.GetString($Bytes, $nameAt, $end - $nameAt))
            }
        }
        Say "  information (pas une preuve) : bibliothèques importées par l'assistant : $($names -join ', ')"
    } catch { Say "  information : table d'importation illisible ($($_.Exception.Message))" }
}

# ── L'essai ────────────────────────────────────────────────────────────────

$exitCode = 0
$echo = $null
$echoSince = $null
try {
    Say "Windows $([Environment]::OSVersion.Version), assistant $Assistant"
    if (-not (Test-Elevated)) { Fail "ce script doit tourner avec un jeton élevé (administrateur, niveau d'intégrité haut)" }
    if (-not (Test-Path -LiteralPath $LauncherScript)) { Fail "lancer-en-utilisateur.ps1 est introuvable ($LauncherScript)" }
    $left = @(Get-Process -Name 'shinku-helper' -ErrorAction SilentlyContinue)
    if ($left.Count -gt 0) {
        Say "un assistant tourne déjà (processus $(($left | ForEach-Object { $_.Id }) -join ', ')) : laissé par un essai d'avant, arrêté"
        Stop-Helpers
    }

    Show-Machine
    Assert-Executable
    if ($null -eq (Read-Value $RunKey $Entree)) {
        Fail "l'entrée d'essai « $Entree » est absente de HKLM\$RunKey : lance d'abord essai-demarrage.ps1 -Preparer"
    }

    # L'état d'origine de l'approbation, noté avant tout changement ; puis l'état
    # « active » écrit en toutes lettres (un essai d'avant, la fenêtre par
    # exemple, peut avoir coupé l'entrée).
    $Original = Read-Value $ApprovedRunKey $Entree
    $OriginalNoted = $true
    $originalText = '(absente)'
    if ($null -ne $Original) { $originalText = $Original.Text }
    Say "approbation de « $Entree » au départ : $originalText"
    $enabled = [byte[]](2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    Write-Value $ApprovedRunKey $Entree $enabled ([Microsoft.Win32.RegistryValueKind]::Binary)
    Done "« $Entree » est mise à l'état actif (02 et onze zéros)"

    # La seconde entrée, pour l'échéance. Une valeur laissée par un essai
    # interrompu est retirée d'abord.
    Remove-Echo
    Write-Value $RunKey $EntreeEcheance $EchoCommand ([Microsoft.Win32.RegistryValueKind]::String)
    Done "entrée « $EntreeEcheance » posée dans HKLM\$RunKey"

    $hex = ConvertTo-HexName $Entree
    $hexEcho = ConvertTo-HexName $EntreeEcheance
    $before = Get-Snapshot

    # ── L'échéance : lancée tout de suite, jamais répondue ─────────────────
    $echo = Start-Helper "demarrage1 couper run $hexEcho"
    $echoSince = Get-Date
    Say "assistant de l'échéance lancé (processus $($echo.Id)) sur « $EntreeEcheance » : on ne lui répondra pas"
    $echoDialog = Find-Dialog $echo $Attente "l'échéance"
    Assert-Dialog $echoDialog 'Couper' $EntreeEcheance "l'échéance"
    Proof "la boîte de l'assistant de l'échéance est ouverte (#32770, « Shinku ») et son texte contient « $EntreeEcheance »"

    # ── Demandes refusées : code 2, aucune fenêtre ─────────────────────────
    Say 'demandes refusées'
    $refused = @(
        @('aucun mot', ''),
        @('un seul mot', 'demarrage1'),
        @('trois mots', "demarrage1 couper run"),
        @('cinq mots', "demarrage1 couper run $hex en-trop"),
        @('une autre version', "demarrage2 couper run $hex"),
        @('un verbe inconnu', "demarrage1 supprimer run $hex"),
        @('un verbe en majuscule', "demarrage1 Couper run $hex"),
        @('une place inconnue', "demarrage1 couper machine $hex"),
        @('un nom qui n''est pas en hexadécimal', "demarrage1 couper run zzzz"),
        @('un nom de longueur impaire', 'demarrage1 couper run 004'),
        @('un nom avec U+0000', "demarrage1 couper run 00450000"),
        @('un nom avec une moitié de paire de substitution', "demarrage1 couper run d800"))
    foreach ($case in $refused) {
        $code = Assert-Quick $case[1] @(2) "demande refusée ($($case[0]))"
        Proof "demande refusée : $($case[0]) : code $code, aucune fenêtre"
    }
    Assert-Unchanged 'après les demandes refusées' $before @()

    # ── Nom inconnu, autre vue, autre place : code 4 ───────────────────────
    $unknown = ConvertTo-HexName 'Essai Shinku Inconnue'
    $code = Assert-Quick "demarrage1 couper run $unknown" @(4) 'nom inconnu'
    Proof "nom inconnu (run) : code $code, aucune fenêtre"
    $code = Assert-Quick "demarrage1 couper run32 $hex" @(4) 'entrée du PC demandée dans la vue 32 bits'
    Proof "« $Entree » demandée dans la vue 32 bits : code $code (elle n'y est pas), aucune fenêtre"
    $lower = $Entree.ToLowerInvariant()
    $hexLower = ConvertTo-HexName $lower
    $code = Assert-Quick "demarrage1 couper run $hexLower" @(4) 'nom écrit autrement'
    Proof "nom en minuscules : code $code (la comparaison est exacte), aucune fenêtre"
    # Le dossier Démarrage commun : 4 s'il est sûr, 6 si Windows lui a donné des
    # droits que l'assistant refuse. Les deux sont sans fenêtre ; le journal dit lequel.
    $code = Assert-Quick "demarrage1 couper dossier $unknown" @(4, 6) 'dossier Démarrage commun'
    Proof "dossier Démarrage commun, nom inconnu : code $code ($(if ($code -eq 4) { 'le dossier est jugé sûr' } else { 'le dossier est jugé non sûr' })), aucune fenêtre"
    Assert-Unchanged 'après les noms inconnus' $before @()

    # ── Une copie hors de Program Files : code 6 ───────────────────────────
    New-Item -ItemType Directory -Force -Path $CopyDir | Out-Null
    $copy = Join-Path $CopyDir 'shinku-helper.exe'
    Copy-Item -LiteralPath $Assistant -Destination $copy -Force
    $code = Assert-Quick "demarrage1 couper run $hex" @(6) 'copie dans %TEMP%' $copy
    Proof "copie de l'assistant dans $CopyDir : code $code (emplacement non sûr), aucune fenêtre"
    Assert-Unchanged 'après la copie' $before @()

    # ── Un refus d'emplacement causé par une entrée de droits ──────────────
    # Le propriétaire de la copie est celui qu'il faut (Administrateurs) : seule
    # une entrée de droits donnant l'écriture à Utilisateurs sur le dossier peut
    # la faire refuser. Le nom demandé n'existe pas : si l'emplacement est
    # accepté, l'assistant rend 4 et n'ouvre aucune boîte.
    Say "emplacement et droits : dossier $RightsDir"
    Remove-RightsDir
    New-Item -ItemType Directory -Path $RightsDir | Out-Null
    $rightsCopy = Join-Path $RightsDir 'shinku-helper.exe'
    Copy-Item -LiteralPath $Assistant -Destination $rightsCopy -Force
    $rightsBefore = @(Get-AclLines $RightsDir)
    Say "  dossier : $(Get-RightsText $RightsDir)"
    Say "  fichier : $(Get-RightsText $rightsCopy)"
    Assert-RightsCase 4 'A (droits de départ)' 'Essai Shinku Inconnue' $rightsCopy
    Proof "copie dans $RightsDir (droits de départ) : code 4, l'emplacement est accepté, aucune fenêtre"

    $grant = '*S-1-5-32-545:(W)'
    & icacls.exe $RightsDir /grant $grant | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "icacls /grant $grant sur $RightsDir rend le code $LASTEXITCODE" }
    $rightsGranted = @(Get-AclLines $RightsDir)
    if (($rightsGranted -join "`n") -eq ($rightsBefore -join "`n")) { Fail "l'entrée d'écriture pour Utilisateurs n'apparaît pas dans les droits de $RightsDir" }
    Say "  dossier, écriture donnée à Utilisateurs : $(Get-RightsText $RightsDir)"
    Assert-RightsCase 6 'B (écriture pour Utilisateurs sur le dossier)' 'Essai Shinku Inconnue' $rightsCopy
    Proof "copie dans $RightsDir avec l'écriture donnée à Utilisateurs sur le dossier : code 6, l'emplacement est refusé à cause de l'entrée de droits, aucune fenêtre"

    & icacls.exe $RightsDir /remove:g '*S-1-5-32-545' | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "icacls /remove:g sur $RightsDir rend le code $LASTEXITCODE" }
    $rightsAfter = @(Get-AclLines $RightsDir)
    if (($rightsAfter -join "`n") -ne ($rightsBefore -join "`n")) {
        Fail "après le retrait, les droits de $RightsDir ne sont pas ceux de départ : avant [$($rightsBefore -join ' ; ')] après [$($rightsAfter -join ' ; ')]"
    }
    Assert-RightsCase 4 'C (entrée retirée)' 'Essai Shinku Inconnue' $rightsCopy
    Proof "copie dans $RightsDir une fois l'entrée retirée (droits revenus à ceux de départ) : code 4, aucune fenêtre"
    Assert-Unchanged 'après l''essai des droits' $before @()

    # ── Déjà dans l'état : code 9 ──────────────────────────────────────────
    $code = Assert-Quick "demarrage1 remettre run $hex" @(9) "remettre une entrée déjà active"
    Proof "remettre « $Entree » déjà active : code $code, aucune fenêtre, rien d'écrit"
    Assert-Unchanged 'après « déjà active »' $before @()

    # ── Annuler ────────────────────────────────────────────────────────────
    Say 'la boîte, réponse Annuler'
    $cancel = Start-Helper "demarrage1 couper run $hex"
    $dialog = Find-Dialog $cancel $Attente 'Annuler'
    Assert-Dialog $dialog 'Couper' $Entree 'Annuler'
    Proof "la boîte est ouverte, de classe #32770, trouvée par le processus $($cancel.Id) ; son texte contient « $Entree »"

    # La console : sous-système graphique (vérifié plus haut), aucune fenêtre de
    # console et aucun conhost enfant, aucune autre fenêtre visible que la boîte.
    $visible = @([Fenetres]::Of([uint32]$cancel.Id) | Where-Object { $_.Visible })
    $others = @($visible | Where-Object { $_.Classe -ne '#32770' })
    if ($others.Count -gt 0) { Fail "l'assistant a une autre fenêtre visible que sa boîte : $(($others | ForEach-Object { $_.Classe }) -join ', ')" }
    $consoles = @([Fenetres]::Of([uint32]$cancel.Id) | Where-Object { $_.Classe -eq 'ConsoleWindowClass' -or $_.Classe -eq 'CASCADIA_HOSTING_WINDOW_CLASS' })
    if ($consoles.Count -gt 0) { Fail "une fenêtre de console appartient à l'assistant" }
    $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($cancel.Id)" -ErrorAction SilentlyContinue)
    $console = @($children | Where-Object { $_.Name -eq 'conhost.exe' -or $_.Name -eq 'OpenConsole.exe' })
    if ($console.Count -gt 0) { Fail "l'assistant a lancé une console (conhost, processus $($console[0].ProcessId))" }
    Proof "aucune fenêtre de console n'appartient à l'assistant (la boîte est sa seule fenêtre visible, aucun conhost enfant)"
    Assert-Unchanged 'boîte ouverte, avant toute réponse' $before @()
    Proof "la boîte est ouverte et HKLM n'a pas changé : rien n'est écrit avant la réponse"

    # L'essai négatif, celui qui compte : le même message, sans droits.
    Say 'l''essai négatif : le même message, envoyé sans droits'
    $probe = Invoke-Unprivileged $dialog.Handle $dialog.Annuler
    foreach ($name in @($probe.Keys | Sort-Object)) { Say "  sans droits : $name = $($probe[$name])" }
    if ($probe['EXCEPTION']) { Fail "l'essai négatif : le processus sans droits a échoué : $($probe['EXCEPTION'])" }
    if ($probe['FINI'] -ne 'oui') { Fail "l'essai négatif : le processus sans droits n'est pas allé au bout" }
    if ($probe['INTEGRITE'] -ne 'moyen') { Fail "l'essai négatif : le processus sans droits tourne au niveau d'intégrité « $($probe['INTEGRITE']) » et non « moyen » : l'essai ne prouverait rien" }
    foreach ($attempt in @('ENVOI', 'POSTE', 'FERMER')) {
        $succeeded = $probe[$attempt + '_SUCCES']
        $lastError = $probe[$attempt + '_ERREUR']
        if ($succeeded -eq 'True') { Fail "l'essai négatif : Windows a laissé passer le message ($attempt) venant du processus sans droits" }
        Say "  $attempt : succès $succeeded, GetLastError $lastError"
        if ($lastError -ne '5') {
            if ($attempt -eq 'FERMER') { Say "  ATTENTION : $attempt refusé avec GetLastError $lastError et non 5 (ERROR_ACCESS_DENIED)" }
            else { Fail "l'essai négatif : $attempt refusé avec GetLastError $lastError et non 5 (ERROR_ACCESS_DENIED), le refus de l'UIPI" }
        }
    }
    Start-Sleep -Seconds 3
    if ($cancel.HasExited) { Fail "l'essai négatif : l'assistant est sorti (code $($cancel.ExitCode)) après le message du processus sans droits" }
    if (-not [Fenetres]::Exists($dialog.Handle)) { Fail "l'essai négatif : la boîte a disparu après le message du processus sans droits" }
    Assert-Unchanged "l'essai négatif" $before @()
    Proof ("depuis un processus sans droits (intégrité moyenne), WM_COMMAND IDCANCEL est refusé (SendMessageTimeout : " +
        "GetLastError $($probe['ENVOI_ERREUR']) ; PostMessage : GetLastError $($probe['POSTE_ERREUR']) ; WM_CLOSE : GetLastError $($probe['FERMER_ERREUR'])) ; " +
        "l'assistant attend toujours et HKLM n'a pas changé")

    Answer-Dialog $dialog 2 'Annuler'
    $code = Wait-Exit $cancel $Attente 'Annuler'
    if ($code -ne 3) { Fail "Annuler : code $code au lieu de 3" }
    Assert-Unchanged 'après Annuler' $before @()
    Proof "Annuler (WM_COMMAND, IDCANCEL) : code $code, HKLM n'a pas changé"

    # ── OK : couper ────────────────────────────────────────────────────────
    Say 'la boîte, réponse OK (couper)'
    $cut = Start-Helper "demarrage1 couper run $hex"
    $dialog = Find-Dialog $cut $Attente 'couper'
    Assert-Dialog $dialog 'Couper' $Entree 'couper'
    Answer-Dialog $dialog 1 'couper'
    $code = Wait-Exit $cut $Attente 'couper'
    if ($code -ne 0) { Fail "couper : code $code au lieu de 0" }
    $read = Read-Value $ApprovedRunKey $Entree
    if ($null -eq $read) { Fail "couper : code 0 mais l'approbation de « $Entree » n'existe pas" }
    if ($read.Kind -ne [Microsoft.Win32.RegistryValueKind]::Binary) { Fail "couper : l'approbation est de type $($read.Kind) et non Binary" }
    $bytes = [byte[]]$read.Value
    if ($bytes.Length -ne 12) { Fail "couper : l'approbation fait $($bytes.Length) octets au lieu de 12 ($($read.Text))" }
    if ($bytes[0] -ne 3 -or $bytes[1] -ne 0 -or $bytes[2] -ne 0 -or $bytes[3] -ne 0) { Fail "couper : les quatre premiers octets sont $([BitConverter]::ToString($bytes, 0, 4)) au lieu de 03-00-00-00" }
    $stamp = [BitConverter]::ToInt64($bytes, 4)
    if ($stamp -le 0) { Fail "couper : la date (huit octets) est vide" }
    $when = [DateTime]::FromFileTimeUtc($stamp)
    $gap = [Math]::Abs(([DateTime]::UtcNow - $when).TotalMinutes)
    if ($gap -gt 10) { Fail "couper : la date écrite est $when UTC, à $([int]$gap) minutes d'ici" }
    Assert-Unchanged 'après OK (couper)' $before @("approuve-run|$Entree")
    Proof "OK (WM_COMMAND, IDOK) : code $code, 12 octets $([BitConverter]::ToString($bytes)) (03 00 00 00 puis la date $when UTC), rien d'autre n'a changé dans HKLM"

    # ── Déjà coupée : code 9, rien n'est écrit ─────────────────────────────
    $cutText = (Read-Value $ApprovedRunKey $Entree).Text
    $code = Assert-Quick "demarrage1 couper run $hex" @(9) 'couper une entrée déjà coupée'
    if ((Read-Value $ApprovedRunKey $Entree).Text -ne $cutText) { Fail "couper une entrée déjà coupée : l'approbation a été réécrite" }
    Proof "couper une entrée déjà coupée : code $code, aucune fenêtre, l'approbation n'est pas réécrite"

    # ── Remettre ───────────────────────────────────────────────────────────
    Say 'la boîte, réponse OK (remettre)'
    $restore = Start-Helper "demarrage1 remettre run $hex"
    $dialog = Find-Dialog $restore $Attente 'remettre'
    Assert-Dialog $dialog 'Remettre' $Entree 'remettre'
    Answer-Dialog $dialog 1 'remettre'
    $code = Wait-Exit $restore $Attente 'remettre'
    if ($code -ne 0) { Fail "remettre : code $code au lieu de 0" }
    $read = Read-Value $ApprovedRunKey $Entree
    if ($null -eq $read) { Fail "remettre : code 0 mais l'approbation de « $Entree » n'existe pas" }
    if ($read.Text -ne 'Binary:02-00-00-00-00-00-00-00-00-00-00-00') { Fail "remettre : l'approbation est $($read.Text) au lieu de Binary:02-00-00-00-00-00-00-00-00-00-00-00" }
    Assert-Unchanged 'après OK (remettre)' $before @()
    Proof "remettre : code $code, 12 octets 02 puis onze zéros, rien d'autre n'a changé dans HKLM"

    $code = Assert-Quick "demarrage1 remettre run $hex" @(9) 'remettre une entrée déjà remise'
    Proof "remettre une entrée déjà remise : code $code, aucune fenêtre"

    # ── L'échéance ─────────────────────────────────────────────────────────
    Say "l'échéance : on attend la fin de l'assistant lancé il y a $([int]((Get-Date) - $echoSince).TotalSeconds) s"
    if ($echo.HasExited) { Say "  il est déjà sorti, code $($echo.ExitCode)" }
    $limit = [Math]::Max(1, 185 - ((Get-Date) - $echoSince).TotalSeconds)
    $code = Wait-Exit $echo ([int][Math]::Ceiling($limit)) "l'échéance"
    $lived = $null
    try { $lived = ($echo.ExitTime - $echo.StartTime).TotalSeconds } catch { }
    if ($null -eq $lived) { Fail "l'échéance : la durée de vie de l'assistant est illisible" }
    Say ("  l'assistant de l'échéance a vécu {0:N1} s et rendu le code {1}" -f $lived, $code)
    if ($code -ne 3) { Fail "l'échéance : code $code au lieu de 3 (après $([int]$lived) s)" }
    if ($lived -lt 118 -or $lived -gt 150) { Fail ("l'échéance : l'assistant est sorti après {0:N1} s, hors de l'intervalle 118 à 150 s" -f $lived) }
    Assert-Unchanged "après l'échéance" $before @()
    if ($null -ne (Read-Value $ApprovedRunKey $EntreeEcheance)) { Fail "l'échéance : une approbation a été écrite pour « $EntreeEcheance »" }
    Proof ("l'échéance : l'assistant sans réponse est sorti avec le code 3 après {0:N1} s (entre 118 et 150 s), sans rien écrire pour « {1} »" -f $lived, $EntreeEcheance)

    Say 'Essai réussi.'
} catch {
    $exitCode = 1
    Say "ECHEC : $($_.Exception.Message)"
    if ($_.InvocationInfo -and $_.InvocationInfo.PositionMessage) {
        foreach ($where in ("$($_.InvocationInfo.PositionMessage)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
    if ($_.ScriptStackTrace) {
        foreach ($where in ("$($_.ScriptStackTrace)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
} finally {
    try { Clean-Up } catch { Say "remise en place incomplète : $($_.Exception.Message)"; $exitCode = 1 }
}
exit $exitCode
