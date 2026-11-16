# Prépare et vérifie l'état d'une machine d'essai pour la page « Confidentialité » de Shinku.
#
# CET ESSAI EFFACE VRAIMENT. Il est fait pour une machine jetable (celle de
# l'intégration continue), jamais pour le PC de quelqu'un : il met de côté le
# dossier `User Data` de Chrome, vide trois clés de HKCU, le dossier « Récents »
# et le presse-papiers. Il ne tourne que si la variable d'environnement
# SHINKU_ESSAI_CONFIDENTIALITE vaut 1 (la CI seule la pose) ; sinon il s'arrête,
# sans rien toucher.
#
# Ce script ne touche jamais à la fenêtre : c'est `essai-confidentialite.mjs`
# qui la pilote, et qui lance chaque étape de ce script avant et après. Chaque
# étape est une action et sa vérification :
#
#   -Etape <nom>-preparer    pose ce que la fenêtre va voir ;
#   -Etape <nom>-verifier    relit la machine après l'effacement ;
#   -Etape nettoyer          remet tout en place (à lancer quoi qu'il arrive) : le User Data d'origine
#                            est remis, l'état et le dossier de travail sont supprimés ;
#   -Etape nettoyer-scenario remet en place ce qu'un scénario a posé (registre, récents, épinglé, fenêtre,
#                            User Data d'essai) mais garde le User Data d'origine de côté, le modèle de
#                            Chrome et l'état : le pilote l'appelle après chaque scénario ;
#   -Constat                 avec une étape -verifier : relève ce qui a disparu ou changé sans décider du
#                            verdict (le pilote l'utilise quand la fenêtre a échoué, pour savoir ce qui est parti).
#
# Un nettoyage incomplet garde l'état (etat.txt) et sort avec le code 1 en listant ce qui n'a pas pu être
# remis : le suivant sait encore ce qu'il reste à remettre.
#
# Les étapes (une par preuve de la conception, section « Essais sur le vrai
# Windows ») :
#   profil                1. faux profil Chrome « Profile 77 », vraies bases SQLite
#   verrou-fichier        2. un fichier du lot tenu ouvert : tout le lot intact
#   verrou-lockfile       2. `lockfile` tenu sans processus Chrome (-Partage Chromium|Aucun)
#   jonction              3. une jonction à la place du dossier du profil
#   lien-physique         3. un lien physique sur `History`
#   lien-registre         3. un lien de registre sur `RunMRU`
#   profil-remplace       4. profil supprimé puis recréé (-remplacer, entre la vue et le clic)
#   registre              5. RunMRU, TypedPaths, WordWheelQuery vidés
#   registre-explorateur  5. la même chose, une fenêtre de l'Explorateur ouverte
#   recents               6. un récent posé, un dossier épinglé à l'Accès rapide
#   presse-papiers        7. un texte dans le presse-papiers
#   chrome-creer, chrome-ouvrir, chrome-fermer, chrome-avant, chrome-apres
#                         8. le vrai Chrome, s'il est installé (code de sortie 10 : absent)
#
# Chaque preuve imprime une ligne qui commence par « FAIT : » et dit ce qui a
# été mesuré. Un écart sort avec le code 1, en disant ce qui était attendu et
# ce qui a été vu. Codes : 0 fait, 1 écart, 2 garde-fou (variable absente),
# 10 Chrome absent (étape chrome-creer seulement).
#
# Les fichiers d'essai sont écrits par le compte qui lance ce script : sur la
# machine de l'intégration continue, c'est celui de la fenêtre (même HKCU, même
# %LOCALAPPDATA%). Rien ici n'est élevé : un script lancé en administrateur
# agit comme le compte, et Chrome est lancé avec un jeton ordinaire
# (lancer-en-utilisateur.ps1).
#
#   $env:SHINKU_ESSAI_CONFIDENTIALITE = '1'
#   powershell -File scripts/essai-confidentialite.ps1 -Etape profil-preparer
#
# Ce fichier commence par une marque d'ordre des octets (BOM) : sans elle,
# Windows PowerShell 5 lirait ses lettres accentuées de travers.

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        'nettoyer', 'nettoyer-scenario',
        'profil-preparer', 'profil-verifier',
        'verrou-fichier-preparer', 'verrou-fichier-verifier',
        'verrou-lockfile-preparer', 'verrou-lockfile-verifier',
        'jonction-preparer', 'jonction-verifier',
        'lien-physique-preparer', 'lien-physique-verifier',
        'lien-registre-preparer', 'lien-registre-verifier',
        'profil-remplace-preparer', 'profil-remplace-remplacer', 'profil-remplace-verifier',
        'registre-preparer', 'registre-verifier',
        'registre-explorateur-preparer', 'registre-explorateur-verifier',
        'recents-preparer', 'recents-verifier',
        'presse-papiers-preparer', 'presse-papiers-verifier',
        'chrome-creer', 'chrome-ouvrir', 'chrome-fermer', 'chrome-avant', 'chrome-apres')]
    [string]$Etape,
    # Étape verrou-lockfile : comment le lockfile est tenu. « Chromium » : en écriture, partage en
    # lecture, supprimé à la fermeture (comme Chromium). « Aucun » : sans aucun partage.
    [ValidateSet('Chromium', 'Aucun')]
    [string]$Partage = 'Chromium',
    # Avec une étape -verifier : journalise les écarts (« CONSTAT ») sans faire échouer l'étape.
    [switch]$Constat
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }

# Le garde-fou, avant toute autre chose.
if ($env:SHINKU_ESSAI_CONFIDENTIALITE -ne '1') {
    Write-Host 'Cet essai efface vraiment (historique de Chrome mis de côté, clés du registre, récents, presse-papiers) :'
    Write-Host 'il est fait pour une machine jetable. Il ne tourne que si SHINKU_ESSAI_CONFIDENTIALITE vaut 1 ; rien n''a été touché.'
    exit 2
}

$Started = Get-Date
function Say([string]$Line) {
    $seconds = ((Get-Date) - $Started).TotalSeconds
    Write-Host ('[{0,6:N1} s] {1}' -f $seconds, $Line)
}
# Une preuve : la ligne commence par « FAIT : » et dit ce qui a été mesuré.
$script:Gaps = 0
function Proof([string]$Line) {
    if (-not $Constat) { Write-Host "FAIT : $Line"; return }
    if ($script:Gaps -eq 0) { Write-Host "CONSTAT : $Line" } else { Write-Host "CONSTAT : (non affirmé : $($script:Gaps) écart(s) déjà relevé(s)) $Line" }
}
function Fail([string]$Message) { throw $Message }
# Un écart relevé par une vérification : il fait échouer l'étape ; avec -Constat il est seulement journalisé.
function Mismatch([string]$Message) {
    if (-not $Constat) { throw $Message }
    $script:Gaps += 1
    Say "  CONSTAT : écart : $Message"
}

# ── Où tout se passe ───────────────────────────────────────────────────────

$ChromeRoot = Join-Path $env:LOCALAPPDATA 'Google\Chrome'
$UserData = Join-Path $ChromeRoot 'User Data'
$Aside = Join-Path $ChromeRoot 'User Data.avant-essai-shinku'
$Profile77 = Join-Path $UserData 'Profile 77'
$DefaultProfile = Join-Path $UserData 'Default'
$LauncherScript = Join-Path $PSScriptRoot 'lancer-en-utilisateur.ps1'

# La règle de l'historique d'un navigateur Chromium (rules.rs) : chaque nom avec ses annexes.
$LotMain = @('History', 'Archived History', 'Visited Links', 'Favicons', 'Shortcuts', 'Network Action Predictor', 'Top Sites')
$Lot = New-Object System.Collections.ArrayList
foreach ($mainName in $LotMain) {
    [void]$Lot.Add($mainName)
    foreach ($suffix in @('-journal', '-wal', '-shm')) { [void]$Lot.Add($mainName + $suffix) }
}

# Les clés du registre que « Ce que tu as tapé dans Windows » vide.
$ExplorerKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer'
$TypedKeys = @('RunMRU', 'TypedPaths', 'WordWheelQuery')
$LinkTarget = 'Software\Essai Shinku Lien Cible'
$ViewLeaf = 'Essai Shinku Vue Explorateur'

$LocalState = '{"os_crypt":{"encrypted_key":"ZmF1eC1lc3NhaS1zaGlua3U="},"profile":{"info_cache":{}}}'
$Utf8 = New-Object System.Text.UTF8Encoding $false

# ── Windows : liens de registre et récents ─────────────────────────────────

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class EssaiFenetres
{
    delegate bool EnumProc(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")]
    static extern bool EnumWindows(EnumProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr window, StringBuilder text, int max);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetWindowTextW(IntPtr window, StringBuilder text, int max);

    [DllImport("user32.dll")]
    static extern bool PostMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    // Les fenetres de l'Explorateur (classe CabinetWClass) dont le titre contient `part`.
    static List<IntPtr> Find(string part)
    {
        List<IntPtr> found = new List<IntPtr>();
        EnumProc callback = delegate (IntPtr window, IntPtr parameter)
        {
            StringBuilder name = new StringBuilder(256);
            GetClassNameW(window, name, name.Capacity);
            if (name.ToString() == "CabinetWClass")
            {
                StringBuilder title = new StringBuilder(512);
                GetWindowTextW(window, title, title.Capacity);
                if (title.ToString().IndexOf(part, StringComparison.OrdinalIgnoreCase) >= 0)
                {
                    found.Add(window);
                }
            }
            return true;
        };
        EnumWindows(callback, IntPtr.Zero);
        return found;
    }

    public static int Count(string part)
    {
        return Find(part).Count;
    }

    // WM_CLOSE : marche d'un niveau eleve vers l'Explorateur, qui tourne en niveau moyen.
    public static int Close(string part)
    {
        List<IntPtr> windows = Find(part);
        foreach (IntPtr window in windows)
        {
            PostMessageW(window, 0x0010, IntPtr.Zero, IntPtr.Zero);
        }
        return windows.Count;
    }
}

public static class EssaiShell
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern uint GetLongPathNameW(string shortPath, StringBuilder longPath, uint size);

    // Le chemin long d'un dossier qui existe (le Shell rend toujours des noms longs).
    public static string LongPath(string path)
    {
        StringBuilder result = new StringBuilder(1024);
        uint length = GetLongPathNameW(path, result, (uint)result.Capacity);
        return (length == 0 || length >= result.Capacity) ? path : result.ToString();
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern void SHAddToRecentDocs(uint flags, string path);

    // SHARD_PATHW : un chemin, comme le ferait un " Ouvrir " d'une app.
    public static void AddRecent(string path)
    {
        SHAddToRecentDocs(3, path);
    }
}

public static class EssaiRegistre
{
    static readonly UIntPtr CurrentUser = new UIntPtr(0x80000001u);
    const uint REG_OPTION_CREATE_LINK = 2;
    const uint REG_OPTION_OPEN_LINK = 8;
    const uint KEY_ALL_ACCESS = 0xF003F;
    const uint KEY_READ = 0x20019;
    const uint DELETE = 0x10000;
    const uint REG_LINK = 6;

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegCreateKeyExW(UIntPtr key, string subKey, uint reserved, string kind, uint options,
        uint access, IntPtr security, out UIntPtr result, out uint disposition);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegSetValueExW(UIntPtr key, string name, uint reserved, uint type, byte[] data, uint size);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegOpenKeyExW(UIntPtr key, string subKey, uint options, uint access, out UIntPtr result);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegQueryValueExW(UIntPtr key, string name, IntPtr reserved, out uint type, byte[] data, ref uint size);

    [DllImport("advapi32.dll")]
    static extern int RegCloseKey(UIntPtr key);

    [DllImport("ntdll.dll")]
    static extern int NtDeleteKey(UIntPtr key);

    // Cree, sous HKCU, une cle qui est un lien de registre vers `target` (chemin natif :
    // \Registry\User\<SID>\...). La cle ne doit pas exister.
    public static void CreateLink(string subKey, string target)
    {
        UIntPtr key;
        uint disposition;
        int status = RegCreateKeyExW(CurrentUser, subKey, 0, null, REG_OPTION_CREATE_LINK, KEY_ALL_ACCESS, IntPtr.Zero, out key, out disposition);
        if (status != 0)
        {
            throw new Win32Exception(status);
        }
        try
        {
            if (disposition != 1)
            {
                throw new InvalidOperationException("la cle existait deja : elle n'est pas devenue un lien");
            }
            byte[] data = Encoding.Unicode.GetBytes(target);
            status = RegSetValueExW(key, "SymbolicLinkValue", 0, REG_LINK, data, (uint)data.Length);
            if (status != 0)
            {
                throw new Win32Exception(status);
            }
        }
        finally
        {
            RegCloseKey(key);
        }
    }

    // Vrai si la cle, ouverte sans suivre les liens, porte une valeur SymbolicLinkValue de type REG_LINK.
    public static bool IsLink(string subKey)
    {
        UIntPtr key;
        int status = RegOpenKeyExW(CurrentUser, subKey, REG_OPTION_OPEN_LINK, KEY_READ, out key);
        if (status != 0)
        {
            return false;
        }
        try
        {
            uint type;
            uint size = 0;
            int found = RegQueryValueExW(key, "SymbolicLinkValue", IntPtr.Zero, out type, null, ref size);
            return (found == 0 || found == 234) && type == REG_LINK;
        }
        finally
        {
            RegCloseKey(key);
        }
    }

    // Retire le lien lui-meme, jamais sa cible : ouvert sans suivre les liens, puis supprime par son poignee.
    public static void DeleteLink(string subKey)
    {
        UIntPtr key;
        int status = RegOpenKeyExW(CurrentUser, subKey, REG_OPTION_OPEN_LINK, DELETE, out key);
        if (status != 0)
        {
            throw new Win32Exception(status);
        }
        try
        {
            int nt = NtDeleteKey(key);
            if (nt != 0)
            {
                throw new InvalidOperationException("NtDeleteKey : 0x" + nt.ToString("X"));
            }
        }
        finally
        {
            RegCloseKey(key);
        }
    }
}
'@

# Les dossiers d'essai sont sous le profil du compte (chemin long), pas sous TEMP : la forme courte de TEMP
# (RUNNER~1) n'est pas celle que le Shell rend pour l'Accès rapide.
$WorkParent = [EssaiShell]::LongPath($env:USERPROFILE)
$Work = Join-Path $WorkParent 'Essai Shinku Confidentialite'
$ViewFolder = Join-Path $WorkParent $ViewLeaf
$StateFile = Join-Path $Work 'etat.txt'
$Scratch = Join-Path $Work 'scratch'

# ── L'état entre deux étapes ───────────────────────────────────────────────

function Get-State {
    $state = @{}
    if (Test-Path -LiteralPath $StateFile) {
        foreach ($line in @(Get-Content -LiteralPath $StateFile -Encoding UTF8)) {
            $at = "$line".IndexOf('=')
            if ($at -gt 0) { $state["$line".Substring(0, $at)] = "$line".Substring($at + 1) }
        }
    }
    return $state
}

function Set-StateValue([string]$Key, [string]$Value) {
    [void][IO.Directory]::CreateDirectory($Work)
    $state = Get-State
    $state[$Key] = $Value
    $lines = New-Object System.Collections.ArrayList
    foreach ($name in @($state.Keys | Sort-Object)) { [void]$lines.Add("$name=$($state[$name])") }
    [IO.File]::WriteAllLines($StateFile, [string[]]@($lines), $Utf8)
}

function Clear-StateValue([string]$Key) {
    if (-not (Test-Path -LiteralPath $StateFile)) { return }
    $state = Get-State
    if (-not $state.ContainsKey($Key)) { return }
    $state.Remove($Key)
    $lines = New-Object System.Collections.ArrayList
    foreach ($name in @($state.Keys | Sort-Object)) { [void]$lines.Add("$name=$($state[$name])") }
    [IO.File]::WriteAllLines($StateFile, [string[]]@($lines), $Utf8)
}

# ── Fichiers : suppression sûre, empreintes ────────────────────────────────

# Supprime un arbre sans jamais suivre un lien : un lien (jonction, lien symbolique) est retiré tout seul,
# jamais ce qu'il désigne. Remove-Item -Recurse de Windows PowerShell 5 n'offre pas cette garantie.
function Remove-TreeSafe([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($item.PSIsContainer) { [IO.Directory]::Delete($Path) } else { [IO.File]::Delete($Path) }
        return
    }
    if ($item.PSIsContainer) {
        foreach ($child in @(Get-ChildItem -LiteralPath $Path -Force)) { Remove-TreeSafe $child.FullName }
        [IO.Directory]::Delete($Path)
    } else {
        [IO.File]::SetAttributes($Path, [IO.FileAttributes]::Normal)
        [IO.File]::Delete($Path)
    }
}

function Get-FileSha([string]$Path) {
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') } finally { $sha.Dispose() }
    } finally {
        $stream.Dispose()
    }
}

# Ce que contient un dossier : chemin relatif -> « taille:SHA-256 », « DOSSIER » ou « LIEN ». Un lien n'est pas suivi.
function Add-Tree([string]$Dir, [string]$Root, $Result) {
    foreach ($child in @(Get-ChildItem -LiteralPath $Dir -Force)) {
        $relative = $child.FullName.Substring($Root.Length).TrimStart('\')
        if ($child.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            $Result[$relative] = 'LIEN'
        } elseif ($child.PSIsContainer) {
            $Result[$relative] = 'DOSSIER'
            Add-Tree $child.FullName $Root $Result
        } else {
            try { $Result[$relative] = ('{0}:{1}' -f $child.Length, (Get-FileSha $child.FullName)) } catch { $Result[$relative] = 'ILLISIBLE' }
        }
    }
}

function Get-Tree([string]$Root) {
    $result = @{}
    $trimmed = $Root.TrimEnd('\')
    if (Test-Path -LiteralPath $trimmed) { Add-Tree $trimmed $trimmed $result }
    return $result
}

function Save-Tree($Tree, [string]$Name) {
    [void][IO.Directory]::CreateDirectory($Work)
    $lines = New-Object System.Collections.ArrayList
    foreach ($key in @($Tree.Keys | Sort-Object)) { [void]$lines.Add("$key`t$($Tree[$key])") }
    [IO.File]::WriteAllLines((Join-Path $Work "arbre-$Name.tsv"), [string[]]@($lines), $Utf8)
}

function Load-Tree([string]$Name) {
    $file = Join-Path $Work "arbre-$Name.tsv"
    if (-not (Test-Path -LiteralPath $file)) { Fail "l'état « $Name » n'a pas été préparé (pas de $file) : lance d'abord l'étape de préparation" }
    $tree = @{}
    foreach ($line in @(Get-Content -LiteralPath $file -Encoding UTF8)) {
        $at = "$line".IndexOf("`t")
        if ($at -gt 0) { $tree["$line".Substring(0, $at)] = "$line".Substring($at + 1) }
    }
    return $tree
}

# Les différences entre deux arbres. Les chemins de `Gone` devaient disparaître, tout le reste devait rester identique.
function Compare-Trees($Before, $After, [string[]]$Gone) {
    $errors = New-Object System.Collections.ArrayList
    foreach ($key in @($Before.Keys | Sort-Object)) {
        $shouldGo = $Gone -contains $key
        if (-not $After.ContainsKey($key)) {
            if (-not $shouldGo) { [void]$errors.Add("disparu : $key") }
        } elseif ($shouldGo) {
            [void]$errors.Add("toujours là : $key")
        } elseif ($After[$key] -ne $Before[$key]) {
            [void]$errors.Add("changé : $key (avant $($Before[$key]), après $($After[$key]))")
        }
    }
    foreach ($key in @($After.Keys | Sort-Object)) {
        if (-not $Before.ContainsKey($key)) { [void]$errors.Add("apparu : $key") }
    }
    return @($errors)
}

function Assert-Tree([string]$What, $Before, $After, [string[]]$Gone, [string]$Expected) {
    $errors = @(Compare-Trees $Before $After $Gone)
    if ($errors.Count -eq 0) { return }
    if ($Constat) {
        foreach ($one in @($errors | Select-Object -First 40)) { Mismatch "$What : $one" }
        if ($errors.Count -gt 40) { Mismatch "$What : … et $($errors.Count - 40) de plus" }
        return
    }
    $shown = @($errors | Select-Object -First 12) -join ' ; '
    $more = ''
    if ($errors.Count -gt 12) { $more = " ; … et $($errors.Count - 12) de plus" }
    Fail "$What : attendu : $Expected ; vu : $($errors.Count) écart(s) : $shown$more"
}

# Les fichiers de la règle présents dans un dossier de profil, avec leur taille : « History 53248 octets, … ».
function Describe-Lot([string]$Dir) {
    $parts = New-Object System.Collections.ArrayList
    foreach ($name in $Lot) {
        $path = Join-Path $Dir $name
        if (Test-Path -LiteralPath $path -PathType Leaf) { [void]$parts.Add(('{0} {1} octets' -f $name, (Get-Item -LiteralPath $path -Force).Length)) }
    }
    return ($parts -join ', ')
}

# ── Un faux profil Chrome, avec de vraies bases SQLite ─────────────────────

# Écrit en ASCII : le fichier est exécuté par le Python de la machine.
$PythonSource = @'
import os
import shutil
import sqlite3
import sys


def rows(count):
    return [("essai shinku %d " % i + "x" * 120,) for i in range(count)]


def plain(path, count=120):
    con = sqlite3.connect(path)
    con.execute("create table t (id integer primary key, v text)")
    con.executemany("insert into t (v) values (?)", rows(count))
    con.commit()
    con.close()


def with_wal(work, destination):
    # Un -wal que personne n'a repris : les fichiers sont copies tant que la connexion est ouverte,
    # avant le point de reprise qu'elle fait en se fermant.
    path = os.path.join(work, "wal.db")
    con = sqlite3.connect(path)
    con.execute("pragma journal_mode=wal")
    con.execute("pragma wal_autocheckpoint=0")
    con.execute("create table t (id integer primary key, v text)")
    con.executemany("insert into t (v) values (?)", rows(120))
    con.commit()
    for suffix in ("", "-wal", "-shm"):
        if os.path.exists(path + suffix):
            shutil.copyfile(path + suffix, destination + suffix)
    con.close()
    if not os.path.exists(destination + "-wal") or os.path.getsize(destination + "-wal") == 0:
        raise SystemExit("le -wal n'a pas ete obtenu")


def with_journal(work, destination):
    # Un -journal que personne n'a rejoue : copie en pleine transaction, cache minuscule pour que
    # les pages d'origine soient deja ecrites dans le journal.
    path = os.path.join(work, "journal.db")
    plain(path, 400)
    con = sqlite3.connect(path, isolation_level=None)
    con.execute("pragma cache_size=2")
    con.execute("begin")
    con.execute("update t set v = v || 'yyyyyyyyyyyyyyyyyyyyyyyyyyyyyy'")
    for suffix in ("", "-journal"):
        if os.path.exists(path + suffix):
            shutil.copyfile(path + suffix, destination + suffix)
    con.execute("rollback")
    con.close()
    if not os.path.exists(destination + "-journal") or os.path.getsize(destination + "-journal") == 0:
        raise SystemExit("le -journal n'a pas ete obtenu")


def text(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as handle:
        handle.write(content)


def blob(path, size, byte):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as handle:
        handle.write(bytes([byte]) * size)


profile = sys.argv[1]
work = sys.argv[2]
os.makedirs(profile, exist_ok=True)
os.makedirs(work, exist_ok=True)

# Les fichiers de la regle : des bases SQLite, dont une avec un -wal et une avec un -journal.
with_journal(work, os.path.join(profile, "History"))
with_wal(work, os.path.join(profile, "Favicons"))
for name in ("Archived History", "Visited Links", "Shortcuts", "Network Action Predictor", "Top Sites"):
    plain(os.path.join(profile, name))

# Ce qui ne doit jamais partir : des bases SQLite aussi, des fichiers de texte, des dossiers.
for name in ("Login Data", "Web Data"):
    plain(os.path.join(profile, name))
os.makedirs(os.path.join(profile, "Network"), exist_ok=True)
plain(os.path.join(profile, "Network", "Cookies"))
text(os.path.join(profile, "Bookmarks"), '{"roots":{"bookmark_bar":{"children":[{"name":"essai","type":"url","url":"https://example.org/"}]}}}')
text(os.path.join(profile, "Preferences"), '{"profile":{"name":"Essai"}}')
blob(os.path.join(profile, "Local Storage", "leveldb", "000003.log"), 4096, 7)
blob(os.path.join(profile, "Sessions", "Session_13300000000"), 2048, 9)
text(os.path.join(profile, "Extensions", "essai", "1.0", "manifest.json"), '{"name":"essai","version":"1.0"}')
# Des noms qui ressemblent a ceux de la regle sans en etre.
blob(os.path.join(profile, "History Provider Cache"), 1024, 3)
blob(os.path.join(profile, "History.bak"), 1024, 4)
blob(os.path.join(profile, "Favicons.old"), 1024, 5)

# Chaque base se relit.
for name in ("Archived History", "Visited Links", "Shortcuts", "Network Action Predictor", "Top Sites", "Login Data", "Web Data"):
    con = sqlite3.connect(os.path.join(profile, name))
    count = con.execute("select count(*) from t").fetchone()[0]
    con.close()
    if count == 0:
        raise SystemExit(name + " est vide")
print("bases SQLite : plusieurs fichiers de la regle, dont History (+ -journal) et Favicons (+ -wal, -shm)")
'@

function Get-PythonCommand {
    foreach ($name in @('python.exe', 'python3.exe')) {
        $command = Get-Command $name -ErrorAction SilentlyContinue | Select-Object -First 1
        # L'alias du Microsoft Store n'est pas un Python.
        if ($null -ne $command -and $command.Source -notmatch 'WindowsApps') { return [pscustomobject]@{ Path = $command.Source; Prefix = @() } }
    }
    $launcher = Get-Command 'py.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $launcher) { return [pscustomobject]@{ Path = $launcher.Source; Prefix = @('-3') } }
    Fail 'attendu : un Python (python.exe ou py.exe) sur la machine, pour fabriquer de vraies bases SQLite ; vu : aucun'
}

function New-FakeProfile([string]$Dir) {
    $python = Get-PythonCommand
    [void][IO.Directory]::CreateDirectory($Work)
    $script = Join-Path $Work 'fabriquer.py'
    [IO.File]::WriteAllText($script, $PythonSource, $Utf8)
    Remove-TreeSafe $Scratch
    $prefix = @($python.Prefix)
    $output = & $python.Path @prefix $script $Dir $Scratch
    if ($LASTEXITCODE -ne 0) { Fail "attendu : le Python fabrique le faux profil dans $Dir ; vu : code de sortie $LASTEXITCODE" }
    foreach ($line in @($output)) { Say "  python : $line" }
    Remove-TreeSafe $Scratch
}

# Met de côté le User Data d'origine (s'il y en a un et que ce n'est pas le nôtre), puis en crée un vide, avec Local State.
function Initialize-UserData {
    $state = Get-State
    if ((Test-Path -LiteralPath $Aside) -and $state['aside'] -ne '1') {
        Fail "attendu : pas de reste d'un essai précédent ; vu : $Aside existe déjà (lance l'étape nettoyer)"
    }
    if ($state['userDataOurs'] -eq '1') {
        Remove-TreeSafe $UserData
    } elseif (Test-Path -LiteralPath $UserData) {
        try { [IO.Directory]::Move($UserData, $Aside) } catch { Fail "attendu : le User Data d'origine est mis de côté ; vu : $($_.Exception.Message) (Chrome tourne-t-il ?)" }
        Set-StateValue 'aside' '1'
        Say "  le User Data d'origine est mis de côté : $Aside"
    }
    [void][IO.Directory]::CreateDirectory($UserData)
    Set-StateValue 'userDataOurs' '1'
    Install-BrowserState
}

# Copie un User Data fait par Chrome (sans lien) : les fichiers, octet pour octet.
function Copy-UserData([string]$From, [string]$To) {
    [void][IO.Directory]::CreateDirectory($To)
    Copy-Item -Path (Join-Path $From '*') -Destination $To -Recurse -Force
}

# Si Chrome est installé, c'est lui qui crée le User Data (une fois pour tout l'essai : le modèle est copié ensuite) ;
# le faux Local State n'est écrit que sans Chrome, ou si Chrome n'a pas su le créer.
function Install-BrowserState {
    $state = Get-State
    if ($null -ne (Get-ChromePath) -and $state['chromeModele'] -ne 'echec') {
        $model = Join-Path $Work 'modele-chrome'
        try {
            if (Test-Path -LiteralPath $model) {
                Copy-UserData $model $UserData
            } else {
                Say "  Chrome crée son User Data (une fois pour tout l'essai)"
                Start-Chrome 'chrome-modele' { Test-Path -LiteralPath (Join-Path $DefaultProfile 'History') } 'Chrome crée son User Data (Default\History)'
                Start-Sleep -Seconds 3
                Stop-Chrome
                # Chrome écrit parfois son Local State seulement à sa fermeture ; s'il n'y est pas, c'est le faux.
                if (-not (Test-Path -LiteralPath (Join-Path $UserData 'Local State'))) {
                    Say '  Chrome n''a pas écrit de Local State : le faux est posé'
                    [IO.File]::WriteAllText((Join-Path $UserData 'Local State'), $LocalState, $Utf8)
                }
                Copy-UserData $UserData $model
            }
            Set-StateValue 'chromeModele' 'oui'
            return
        } catch {
            Say "  Chrome n'a pas pu créer son User Data ($($_.Exception.Message)) : le faux Local State est écrit à la place"
            try { Stop-Chrome } catch { }
            Remove-TreeSafe $UserData
            Remove-TreeSafe $model
            [void][IO.Directory]::CreateDirectory($UserData)
            Set-StateValue 'chromeModele' 'echec'
        }
    }
    [IO.File]::WriteAllText((Join-Path $UserData 'Local State'), $LocalState, $Utf8)
}

# Le faux Chrome : User Data neuf, un seul profil, « Profile 77 ».
function Initialize-FakeBrowser {
    if (@(Get-Process -Name 'chrome' -ErrorAction SilentlyContinue).Count -gt 0) {
        Fail 'attendu : Chrome est fermé ; vu : un processus chrome.exe tourne'
    }
    Initialize-UserData
    New-FakeProfile $Profile77
}

# ── Un fichier tenu ouvert par un autre processus ──────────────────────────

# Écrit en ASCII, exécuté par un second PowerShell qui tient le fichier jusqu'à ce qu'on le lui dise.
$HoldSource = @'
param([string]$Chemin, [string]$Partage, [string]$Signal, [string]$Arret, [int]$Duree)
$ErrorActionPreference = 'Stop'
$flux = $null
try {
    if ($Partage -eq 'Chromium') {
        # Comme Chromium : en ecriture, partage en lecture, supprime a la fermeture.
        $flux = New-Object System.IO.FileStream($Chemin, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read, 4096, [System.IO.FileOptions]::DeleteOnClose)
    } elseif ($Partage -eq 'Aucun') {
        $flux = New-Object System.IO.FileStream($Chemin, [System.IO.FileMode]::Create, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None, 4096, [System.IO.FileOptions]::DeleteOnClose)
    } else {
        $flux = New-Object System.IO.FileStream($Chemin, [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    }
    [System.IO.File]::WriteAllText($Signal, 'PRET ' + $PID)
    $fin = (Get-Date).AddSeconds($Duree)
    while ((Get-Date) -lt $fin -and -not (Test-Path -LiteralPath $Arret)) { Start-Sleep -Milliseconds 200 }
} catch {
    [System.IO.File]::WriteAllText($Signal, 'ERREUR ' + $_.Exception.Message)
} finally {
    if ($null -ne $flux) { $flux.Dispose() }
}
'@

# Lance le processus qui tient `Path` ; `Mode` : Chromium, Aucun (un lockfile) ou Fichier (un fichier existant, sans partage).
function Start-Holder([string]$Path, [string]$Mode) {
    [void][IO.Directory]::CreateDirectory($Work)
    $script = Join-Path $Work 'tenir.ps1'
    [IO.File]::WriteAllText($script, $HoldSource, $Utf8)
    $signal = Join-Path $Work 'tenir-signal.txt'
    $stop = Join-Path $Work 'tenir-arret.txt'
    Remove-Item -LiteralPath $signal, $stop -Force -ErrorAction SilentlyContinue
    $parameters = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $script,
        '-Chemin', $Path, '-Partage', $Mode, '-Signal', $signal, '-Arret', $stop, '-Duree', '600')
    # Start-Process recolle la liste avec des espaces, sans guillemets : un argument qui en contient porte les siens.
    $parameters = @($parameters | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } })
    $process = Start-Process -FilePath 'powershell.exe' -ArgumentList $parameters -PassThru -WindowStyle Hidden
    $null = $process.Handle
    Set-StateValue 'holderPid' ([string]$process.Id)
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path -LiteralPath $signal) {
            $said = ([IO.File]::ReadAllText($signal)).Trim()
            if ($said.StartsWith('PRET')) { Say "  $Path tenu ouvert par le processus $($process.Id) (mode $Mode)"; return }
            Fail "attendu : le processus tient $Path ; vu : $said"
        }
        if ($process.HasExited) { Fail "attendu : le processus tient $Path ; vu : il s'est arrêté (code $($process.ExitCode)) sans le dire" }
        Start-Sleep -Milliseconds 200
    }
    Fail "attendu : le processus tient $Path en 30 s ; vu : aucune réponse"
}

function Stop-Holder {
    $state = Get-State
    if (-not $state.ContainsKey('holderPid')) { return }
    $id = [int]$state['holderPid']
    $stop = Join-Path $Work 'tenir-arret.txt'
    [void][IO.Directory]::CreateDirectory($Work)
    [IO.File]::WriteAllText($stop, 'stop')
    $process = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($null -ne $process) {
        if (-not $process.WaitForExit(15000)) {
            Say "  le processus $id ne lâche pas son fichier en 15 s : arrêté de force"
            try { $process.Kill() } catch { }
            [void]$process.WaitForExit(10000)
        }
    }
    Clear-StateValue 'holderPid'
    Say "  le processus $id a lâché son fichier"
}

# Ouvre `lockfile` comme le fait le cœur de Shinku (lecture et écriture, partage total) : « ouvert » ou la raison du refus.
function Test-LockfileOpen([string]$Path) {
    try {
        $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        $stream.Dispose()
        return 'ouvert (le cœur ne le verrait pas tenu)'
    } catch [System.IO.FileNotFoundException] {
        return 'absent'
    } catch [System.IO.DirectoryNotFoundException] {
        return 'absent'
    } catch {
        return "refusé : $($_.Exception.Message)"
    }
}

# ── Le registre ────────────────────────────────────────────────────────────

function Get-ValueCount([string]$Name) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("$ExplorerKey\$Name", $false)
    if ($null -eq $key) { return -1 }
    try { return @($key.GetValueNames()).Count } finally { $key.Close() }
}

# Pose des valeurs dans les trois clés (en vidant d'abord ce qu'elles contenaient).
function Set-TypedValues {
    if ([EssaiRegistre]::IsLink("$ExplorerKey\RunMRU")) {
        Fail 'attendu : RunMRU est une clé ordinaire avant de poser les valeurs ; vu : un lien de registre resté d''un essai précédent (lance l''étape nettoyer)'
    }
    $utf16 = [Text.Encoding]::Unicode
    foreach ($name in $TypedKeys) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey("$ExplorerKey\$name")
        try {
            foreach ($old in @($key.GetValueNames())) { $key.DeleteValue($old, $false) }
            if ($name -eq 'RunMRU') {
                $key.SetValue('a', 'notepad\1', [Microsoft.Win32.RegistryValueKind]::String)
                $key.SetValue('b', 'calc\1', [Microsoft.Win32.RegistryValueKind]::String)
                $key.SetValue('MRUList', 'ba', [Microsoft.Win32.RegistryValueKind]::String)
            } elseif ($name -eq 'TypedPaths') {
                $key.SetValue('url1', 'C:\Windows', [Microsoft.Win32.RegistryValueKind]::String)
                $key.SetValue('url2', 'C:\Temp', [Microsoft.Win32.RegistryValueKind]::String)
            } else {
                $word = $utf16.GetBytes('essai shinku') + [byte[]](0, 0)
                $key.SetValue('0', [byte[]]$word, [Microsoft.Win32.RegistryValueKind]::Binary)
                $key.SetValue('MRUListEx', [byte[]](0, 0, 0, 0, 255, 255, 255, 255), [Microsoft.Win32.RegistryValueKind]::Binary)
            }
        } finally { $key.Close() }
    }
    Set-StateValue 'registreTouche' '1'
}

function Show-TypedCounts([string]$Title) {
    $parts = foreach ($name in $TypedKeys) { '{0} : {1}' -f $name, (Get-ValueCount $name) }
    return "$Title " + ($parts -join ' ; ')
}

# ── Récents, Accès rapide, Explorateur ─────────────────────────────────────

$QuickAccess = 'shell:::{679f85cb-0220-4080-b29b-5540cc05aab6}'
$RecentFolder = Join-Path $env:APPDATA 'Microsoft\Windows\Recent'

function Get-RecentCount {
    return @(Get-ChildItem -LiteralPath $RecentFolder -File -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*.lnk' }).Count
}

# Les contenus (noms et tailles, jamais le contenu des fichiers) de Recent, AutomaticDestinations et CustomDestinations.
function Show-RecentFolders([string]$Title) {
    $places = @(
        @('Recent', $RecentFolder),
        @('AutomaticDestinations', (Join-Path $RecentFolder 'AutomaticDestinations')),
        @('CustomDestinations', (Join-Path $RecentFolder 'CustomDestinations')))
    foreach ($place in $places) {
        $files = @(Get-ChildItem -LiteralPath $place[1] -File -Force -ErrorAction SilentlyContinue)
        $total = 0
        foreach ($file in $files) { $total += $file.Length }
        Say ('  {0} : {1} : {2} fichier(s), {3} octets' -f $Title, $place[0], $files.Count, $total)
        foreach ($file in @($files | Select-Object -First 30)) { Say ('      {0} ({1} octets)' -f $file.Name, $file.Length) }
        if ($files.Count -gt 30) { Say ('      … et {0} de plus' -f ($files.Count - 30)) }
    }
}

function Get-QuickAccessPaths {
    $shell = New-Object -ComObject Shell.Application
    $folder = $shell.Namespace($QuickAccess)
    if ($null -eq $folder) { return $null }
    $paths = New-Object System.Collections.ArrayList
    foreach ($item in @($folder.Items())) { [void]$paths.Add([string]$item.Path) }
    return ,@($paths)
}

# Le nom que le Shell donne au dossier (chemin long) : c'est avec lui qu'on compare, sans tenir compte de la casse.
function Get-ShellPath([string]$Folder) {
    $shell = New-Object -ComObject Shell.Application
    $namespace = $shell.Namespace($Folder)
    if ($null -eq $namespace) { return $Folder }
    return [string]$namespace.Self.Path
}

function Test-Listed($Paths, [string[]]$Wanted) {
    foreach ($path in @($Paths)) {
        foreach ($one in $Wanted) { if ($path -ieq $one) { return $true } }
    }
    return $false
}

function Wait-ExplorerClosed([int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline -and [EssaiFenetres]::Count($ViewLeaf) -gt 0) { Start-Sleep -Milliseconds 300 }
}

# ── Chrome, le vrai ────────────────────────────────────────────────────────

function Get-ChromePath {
    $roots = @($env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:LOCALAPPDATA)
    foreach ($root in $roots) {
        if (-not $root) { continue }
        $path = Join-Path $root 'Google\Chrome\Application\chrome.exe'
        if (Test-Path -LiteralPath $path) { return $path }
    }
    return $null
}

function Get-ChromeProcesses { return @(Get-Process -Name 'chrome' -ErrorAction SilentlyContinue) }

# Lance une ligne de commande avec un jeton ordinaire (lancer-en-utilisateur.ps1), par un petit fichier .cmd :
# la valeur de -Arguments d'un script lancé par -File ne doit pas commencer par un tiret.
function Start-AsUser([string]$Line, [string]$Name) {
    [void][IO.Directory]::CreateDirectory($Work)
    $cmd = Join-Path $Work "$Name.cmd"
    [IO.File]::WriteAllText($cmd, ("@echo off`r`n" + $Line + "`r`nexit /b %errorlevel%`r`n"), [Text.Encoding]::ASCII)
    $out = Join-Path $Work "$Name.sortie.txt"
    Remove-Item -LiteralPath $out, "$out.err" -Force -ErrorAction SilentlyContinue
    $parameters = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $LauncherScript,
        '-Programme', (Join-Path $env:SystemRoot 'System32\cmd.exe'), '-Arguments', ('/c ""{0}""' -f $cmd))
    $parameters = @($parameters | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } })
    $launcher = Start-Process -FilePath 'powershell.exe' -ArgumentList $parameters -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $out -RedirectStandardError "$out.err"
    $null = $launcher.Handle
    return $launcher
}

# Lance Chrome sans fenêtre (headless), le laisse tourner, et attend que `Condition` soit vraie.
function Start-Chrome([string]$Name, [scriptblock]$Condition, [string]$What) {
    $chrome = Get-ChromePath
    if ($null -eq $chrome) { Fail 'attendu : Chrome est installé ; vu : chrome.exe introuvable' }
    [void][IO.Directory]::CreateDirectory($Work)
    $page = Join-Path $Work 'page.html'
    [IO.File]::WriteAllText($page, '<html><head><title>essai shinku</title></head><body>essai shinku</body></html>', $Utf8)
    $url = 'file:///' + $page.Replace('\', '/')
    # --user-data-dir en toutes lettres : l'essai ne dépend pas du dossier que Chrome choisirait sans fenêtre.
    $line = 'start "" "{0}" --headless=new --disable-gpu --no-first-run --no-default-browser-check "--user-data-dir={1}" "{2}"' -f $chrome, $UserData, $url
    $launcher = Start-AsUser $line $Name
    Set-StateValue 'chromeUsed' '1'
    if (-not $launcher.WaitForExit(30000)) { try { $launcher.Kill() } catch { } }
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline) {
        if ((Get-ChromeProcesses).Count -gt 0 -and (& $Condition)) { return }
        Start-Sleep -Milliseconds 500
    }
    $seen = "$((Get-ChromeProcesses).Count) processus chrome.exe"
    $err = ''
    if (Test-Path -LiteralPath (Join-Path $Work "$Name.sortie.txt.err")) { $err = (Get-Content -LiteralPath (Join-Path $Work "$Name.sortie.txt.err")) -join ' ' }
    Fail "attendu : $What dans les 60 s ; vu : $seen $err"
}

# Ferme tout Chrome (arrêt de force) et attend la fin de tous ses processus.
function Stop-Chrome {
    foreach ($process in @(Get-ChromeProcesses)) { try { $process.Kill() } catch { } }
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline -and (Get-ChromeProcesses).Count -gt 0) { Start-Sleep -Milliseconds 300 }
    $left = (Get-ChromeProcesses).Count
    if ($left -gt 0) { Fail "attendu : plus aucun processus chrome.exe ; vu : $left en 30 s" }
}

# Les fichiers que Chrome ne doit jamais perdre à l'effacement de l'historique.
function Get-ChromeProtected {
    $result = @{}
    $names = @('Local State', 'Default\Bookmarks', 'Default\Login Data', 'Default\Web Data', 'Default\Preferences',
        'Default\Secure Preferences', 'Default\Network\Cookies', 'Default\Account Web Data', 'Default\Login Data For Account')
    foreach ($name in $names) {
        $path = Join-Path $UserData $name
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $result[$name] = ('{0}:{1}' -f (Get-Item -LiteralPath $path -Force).Length, (Get-FileSha $path))
        }
    }
    return $result
}

# ── Les étapes : 1. le faux profil ─────────────────────────────────────────

function Step-ProfilPreparer {
    Initialize-FakeBrowser
    $tree = Get-Tree $UserData
    Save-Tree $tree 'profil'
    $needed = @('Bookmarks', 'Login Data', 'Web Data', 'Network\Cookies', 'Preferences', 'Local Storage\leveldb\000003.log',
        'Sessions\Session_13300000000', 'Extensions\essai\1.0\manifest.json', 'History-journal', 'Favicons-wal', 'Favicons-shm')
    foreach ($name in $needed) {
        if (-not $tree.ContainsKey("Profile 77\$name")) { Fail "attendu : le faux profil a $name ; vu : absent" }
    }
    if (-not $tree.ContainsKey('Local State')) { Fail 'attendu : User Data a un Local State ; vu : absent' }
    $inLot = @($Lot | Where-Object { $tree.ContainsKey("Profile 77\$_") })
    $origin = 'faux Local State'
    if ((Get-State)['chromeModele'] -eq 'oui') { $origin = 'User Data créé par le vrai Chrome, avec ses propres fichiers' }
    Proof "faux profil Chrome « Profile 77 » préparé dans User Data ($origin) : $($tree.Count) éléments, dont $($inLot.Count) fichiers de la règle ($(Describe-Lot $Profile77))"
    Proof 'vérifiés présents à côté des fichiers de la règle : Bookmarks, Login Data, Web Data, Network\Cookies, Preferences, Local Storage\leveldb, Sessions, Extensions, et Local State dans User Data'
}

function Step-ProfilVerifier {
    $before = Load-Tree 'profil'
    $after = Get-Tree $UserData
    $gone = @($Lot | Where-Object { $before.ContainsKey("Profile 77\$_") } | ForEach-Object { "Profile 77\$_" })
    if ($gone.Count -eq 0) { Fail 'attendu : le profil préparé avait des fichiers de la règle ; vu : aucun' }
    Assert-Tree 'après l''effacement du faux profil' $before $after $gone 'seuls les fichiers de la règle ont disparu, tout le reste est identique octet pour octet'
    Proof "les $($gone.Count) fichiers de la règle ont disparu de Profile 77 et les $($after.Count) autres éléments de User Data sont identiques octet pour octet (SHA-256) : $((($gone | ForEach-Object { $_.Substring('Profile 77\'.Length) }) -join ', '))"
    $annexes = foreach ($name in @('Profile 77\History-journal', 'Profile 77\Favicons-wal', 'Profile 77\Favicons-shm')) {
        '{0} : {1}' -f $name.Substring('Profile 77\'.Length), $(if ($after.ContainsKey($name)) { 'reste' } else { 'parti' })
    }
    Proof "les annexes SQLite (le -journal non rejoué de History, le -wal non repris de Favicons) : $($annexes -join ' ; ')"
}

# ── 2. Un fichier tenu ouvert ──────────────────────────────────────────────

function Step-VerrouFichierPreparer {
    Initialize-FakeBrowser
    $before = Get-Tree $UserData
    Save-Tree $before 'verrou-fichier'
    # Le dernier fichier de la règle dans l'ordre du cœur : si l'effacement n'était pas « tout ou rien », les autres seraient déjà partis.
    $held = Join-Path $Profile77 'Top Sites'
    Start-Holder $held 'Fichier'
    Proof "« Top Sites » (dernier fichier de la règle) est tenu ouvert sans partage par un autre processus ; $(@($Lot | Where-Object { $before.ContainsKey("Profile 77\$_") }).Count) fichiers de la règle au total"
}

function Step-VerrouFichierVerifier {
    Stop-Holder
    $before = Load-Tree 'verrou-fichier'
    $after = Get-Tree $UserData
    Assert-Tree 'après un effacement refusé par un fichier tenu' $before $after @() 'tout le lot est intact (aucun fichier de la règle n''a disparu, rien n''a changé)'
    Proof "tout le lot est intact : $($after.Count) éléments de User Data ont la même empreinte SHA-256 qu'avant ; un seul fichier tenu ouvert n'a laissé partir aucun autre"
}

# ── 2. Le lockfile ─────────────────────────────────────────────────────────

function Step-VerrouLockfilePreparer {
    Initialize-FakeBrowser
    $before = Get-Tree $UserData
    Save-Tree $before 'verrou-lockfile'
    Set-StateValue 'lockfileMode' $Partage
    $lock = Join-Path $UserData 'lockfile'
    Start-Holder $lock $Partage
    $mode = 'en écriture, partage en lecture, supprimé à la fermeture (comme Chromium)'
    if ($Partage -eq 'Aucun') { $mode = 'en lecture et écriture, sans aucun partage' }
    Proof "lockfile tenu $mode, sans aucun processus Chrome ; ouverture lecture+écriture (partage total) comme le cœur la fait : $(Test-LockfileOpen $lock)"
}

function Step-VerrouLockfileVerifier {
    Stop-Holder
    $before = Load-Tree 'verrou-lockfile'
    $after = Get-Tree $UserData
    Assert-Tree 'après la lecture de la page avec un lockfile tenu' $before $after @() 'rien n''a changé (la ligne était inerte, le lockfile a disparu à la fermeture)'
    Proof "lockfile lâché : User Data est identique à l'état d'avant ($($after.Count) éléments), rien n'a été touché pendant que le lockfile était tenu"
}

# ── 3. Les pièges ──────────────────────────────────────────────────────────

function Step-JonctionPreparer {
    Initialize-UserData
    $decoy = Join-Path $Work 'cible-jonction'
    Remove-TreeSafe $decoy
    New-FakeProfile $decoy
    Save-Tree (Get-Tree $decoy) 'jonction-cible'
    [void](New-Item -ItemType Junction -Path $Profile77 -Value $decoy)
    $item = Get-Item -LiteralPath $Profile77 -Force
    if (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Fail "attendu : Profile 77 est une jonction ; vu : un dossier ordinaire (attributs $($item.Attributes))" }
    Save-Tree (Get-Tree $UserData) 'jonction'
    Proof "« Profile 77 » est une jonction (attributs $($item.Attributes)) vers $decoy, qui porte un vrai profil : $(Describe-Lot $decoy)"
}

function Step-JonctionVerifier {
    $item = Get-Item -LiteralPath $Profile77 -Force -ErrorAction SilentlyContinue
    if ($null -eq $item) {
        Mismatch 'attendu : la jonction « Profile 77 » est toujours là ; vu : absente'
    } elseif (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        Mismatch 'attendu : « Profile 77 » est toujours une jonction ; vu : un dossier ordinaire'
    }
    $decoy = Join-Path $Work 'cible-jonction'
    $before = Load-Tree 'jonction-cible'
    $after = Get-Tree $decoy
    Assert-Tree 'de l''autre côté de la jonction' $before $after @() 'rien n''est touché de l''autre côté (aucun fichier disparu ni changé)'
    Proof "rien n'est touché de l'autre côté de la jonction : $($after.Count) éléments identiques octet pour octet ; la jonction est toujours là"
}

function Step-LienPhysiquePreparer {
    Initialize-FakeBrowser
    $decoyDir = Join-Path $Work 'cible-lien'
    Remove-TreeSafe $decoyDir
    [void][IO.Directory]::CreateDirectory($decoyDir)
    $decoy = Join-Path $decoyDir 'History'
    $history = Join-Path $Profile77 'History'
    Copy-Item -LiteralPath $history -Destination $decoy
    Remove-Item -LiteralPath $history -Force
    try {
        [void](New-Item -ItemType HardLink -Path $history -Value $decoy)
    } catch {
        Say "  New-Item -ItemType HardLink a échoué ($($_.Exception.Message)) : fsutil"
        & fsutil.exe hardlink create $history $decoy | Out-Null
        if ($LASTEXITCODE -ne 0) { Fail "attendu : un lien physique sur History ; vu : fsutil rend le code $LASTEXITCODE" }
    }
    $names = @(& fsutil.exe hardlink list $history)
    $count = @($names | Where-Object { "$_".Trim() }).Count
    if ($count -lt 2) { Fail "attendu : History a au moins deux noms ; vu : $count (fsutil hardlink list : $($names -join ' | '))" }
    Save-Tree (Get-Tree $UserData) 'lien-physique'
    Save-Tree (Get-Tree $decoyDir) 'lien-physique-cible'
    Proof "« History » a $count noms (NumberOfLinks = $count d'après fsutil hardlink list) : l'autre nom est $decoy"
}

function Step-LienPhysiqueVerifier {
    $history = Join-Path $Profile77 'History'
    $decoyDir = Join-Path $Work 'cible-lien'
    Assert-Tree 'après l''effacement refusé par un lien physique (User Data)' (Load-Tree 'lien-physique') (Get-Tree $UserData) @() 'tout le lot est intact (aucun fichier de la règle n''a disparu, History compris)'
    Assert-Tree 'de l''autre côté du lien physique' (Load-Tree 'lien-physique-cible') (Get-Tree $decoyDir) @() 'l''autre nom de History est intact'
    $count = 0
    if (Test-Path -LiteralPath $history) { $count = @(@(& fsutil.exe hardlink list $history) | Where-Object { "$_".Trim() }).Count }
    if ($count -lt 2) { Mismatch "attendu : History a toujours deux noms ; vu : $count" }
    Proof "le lien physique sur History a $count noms, et tout le lot est resté en place : un fichier à plusieurs noms n'a rien fait partir"
}

function Step-LienRegistrePreparer {
    Initialize-FakeBrowser
    $target = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($LinkTarget)
    try {
        foreach ($old in @($target.GetValueNames())) { $target.DeleteValue($old, $false) }
        $target.SetValue('a', 'cible-1', [Microsoft.Win32.RegistryValueKind]::String)
        $target.SetValue('b', 'cible-2', [Microsoft.Win32.RegistryValueKind]::String)
    } finally { $target.Close() }
    # TypedPaths et WordWheelQuery sont des clés ordinaires, avec des valeurs : la ligne « Ce que tu as tapé » existe.
    Set-TypedValues
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree("$ExplorerKey\RunMRU", $false)
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $nativeTarget = "\Registry\User\$sid\$LinkTarget"
    [EssaiRegistre]::CreateLink("$ExplorerKey\RunMRU", $nativeTarget)
    if (-not [EssaiRegistre]::IsLink("$ExplorerKey\RunMRU")) { Fail 'attendu : RunMRU est un lien de registre (SymbolicLinkValue, REG_LINK) ; vu : ce n''est pas un lien' }
    $seen = Get-ValueCount 'RunMRU'
    if ($seen -ne 2) { Fail "attendu : ouvert en suivant le lien, RunMRU montre les 2 valeurs de la cible ; vu : $seen" }
    Set-StateValue 'lienRegistre' '1'
    Proof "RunMRU est un lien de registre vers HKCU\$LinkTarget : ouvert en suivant le lien, il montre les $seen valeurs de la cible ; $(Show-TypedCounts 'valeurs avant :')"
}

# Défait le piège : le lien lui-même (jamais sa cible) puis la cible ; sans lien resté, la preuve 5 lit une vraie clé.
function Remove-RegistryTrap {
    if ([EssaiRegistre]::IsLink("$ExplorerKey\RunMRU")) { [EssaiRegistre]::DeleteLink("$ExplorerKey\RunMRU") }
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($LinkTarget, $false)
    if ([EssaiRegistre]::IsLink("$ExplorerKey\RunMRU")) { Fail 'attendu : le lien de registre RunMRU est retiré ; vu : c''est encore un lien' }
    Clear-StateValue 'lienRegistre'
}

function Step-LienRegistreVerifier {
    $counts = ''
    $original = $null
    try {
        if (-not [EssaiRegistre]::IsLink("$ExplorerKey\RunMRU")) { Mismatch 'attendu : RunMRU est toujours un lien de registre ; vu : ce n''est plus un lien' }
        $target = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($LinkTarget, $false)
        if ($null -eq $target) {
            Mismatch "attendu : la cible HKCU\$LinkTarget existe encore ; vu : absente"
        } else {
            try {
                $names = @($target.GetValueNames() | Sort-Object)
                if (($names -join ',') -ne 'a,b') { Mismatch "attendu : la cible garde ses valeurs a et b ; vu : $($names -join ',')" }
                elseif ($target.GetValue('a') -ne 'cible-1' -or $target.GetValue('b') -ne 'cible-2') { Mismatch 'attendu : les données de a et b sont inchangées ; vu : elles ont changé' }
            } finally { $target.Close() }
        }
        $counts = Show-TypedCounts 'valeurs après :'
    } catch {
        $original = $_
    }
    # Le piège ne reste pas : ni lui ni sa cible ne gênent l'essai suivant. Une erreur ici ne masque pas l'écart d'origine.
    try {
        Remove-RegistryTrap
    } catch {
        Say "  le piège du registre n'a pas pu être défait : $($_.Exception.Message)"
        if ($null -eq $original) { throw }
    }
    if ($null -ne $original) { throw $original }
    Proof "rien n'est touché de l'autre côté du lien de registre : la cible avait encore a=cible-1 et b=cible-2, et RunMRU était encore un lien ; $counts ; le lien et sa cible sont maintenant retirés"
}

# ── 4. Un profil remplacé ──────────────────────────────────────────────────

function Get-FileIdText([string]$Path) {
    try { return (@(& fsutil.exe file queryfileid $Path) -join ' ').Trim() } catch { return "illisible ($($_.Exception.Message))" }
}

function Step-ProfilRemplacePreparer {
    Initialize-FakeBrowser
    Save-Tree (Get-Tree $UserData) 'remplace-avant'
    $id = Get-FileIdText $Profile77
    Set-StateValue 'remplaceId' $id
    Proof "faux profil préparé ; identifiant du dossier « Profile 77 » : $id"
}

function Step-ProfilRemplaceRemplacer {
    $before = Get-State
    Remove-TreeSafe $Profile77
    if (Test-Path -LiteralPath $Profile77) { Fail 'attendu : Profile 77 supprimé ; vu : il existe encore' }
    New-FakeProfile $Profile77
    Save-Tree (Get-Tree $UserData) 'remplace-apres'
    $oldId = $before['remplaceId']
    $newId = Get-FileIdText $Profile77
    $verdict = 'différent : le profil est reconnu comme un autre'
    if ($oldId -eq $newId) { $verdict = 'IDENTIQUE : le disque a réutilisé l''identifiant, le cœur ne peut pas voir le remplacement par l''identité' }
    Proof "« Profile 77 » supprimé puis recréé avec les mêmes noms de fichiers ; identifiant avant : $oldId ; après : $newId ; $verdict"
}

function Step-ProfilRemplaceVerifier {
    Assert-Tree 'après un effacement demandé pour l''ancien profil' (Load-Tree 'remplace-apres') (Get-Tree $UserData) @() 'le profil recréé est intact (rien n''a été effacé : Shinku a refusé, le profil a changé)'
    Proof 'le profil recréé est intact : aucun de ses fichiers n''a été touché par la demande faite pour l''ancien'
}

# ── 5. Le registre ─────────────────────────────────────────────────────────

function Step-RegistrePreparer {
    Set-TypedValues
    Proof "valeurs posées dans les trois clés ; $(Show-TypedCounts 'valeurs avant :')"
}

function Assert-TypedEmpty([string]$What) {
    foreach ($name in $TypedKeys) {
        $count = Get-ValueCount $name
        if ($count -lt 0) { Mismatch "$What : attendu : la clé $name existe, vide ; vu : la clé a disparu" }
        elseif ($count -ne 0) { Mismatch "$What : attendu : la clé $name est vide ; vu : $count valeur(s)" }
    }
}

function Step-RegistreVerifier {
    Assert-TypedEmpty 'après l''effacement de ce qui a été tapé'
    Proof "les trois clés sont présentes et vides : $(Show-TypedCounts 'valeurs après :')"
}

function Step-RegistreExplorateurPreparer {
    Set-TypedValues
    [void][IO.Directory]::CreateDirectory($ViewFolder)
    # Les valeurs sont posées avant d'ouvrir la fenêtre : elle les a donc en mémoire.
    $before = [EssaiFenetres]::Count($ViewLeaf)
    Start-Process -FilePath 'explorer.exe' -ArgumentList ('"{0}"' -f $ViewFolder) | Out-Null
    $deadline = (Get-Date).AddSeconds(30)
    $open = 0
    while ((Get-Date) -lt $deadline) {
        $open = [EssaiFenetres]::Count($ViewLeaf)
        if ($open -gt 0) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($open -le $before) {
        Set-StateValue 'explorateur' '0'
        Proof "aucune fenêtre de l'Explorateur (classe CabinetWClass, titre « $ViewLeaf ») n'a été vue par EnumWindows en 30 s ($before avant) : la mesure « avec une fenêtre ouverte » n'est pas faite sur cette machine ; $(Show-TypedCounts 'valeurs avant :')"
    } else {
        Set-StateValue 'explorateur' '1'
        Proof "fenêtres de l'Explorateur « $ViewLeaf » : $before avant, $open après l'ouverture ; $(Show-TypedCounts 'valeurs avant :')"
    }
}

function Step-RegistreExplorateurVerifier {
    $state = Get-State
    $open = [EssaiFenetres]::Count($ViewLeaf)
    Assert-TypedEmpty 'juste après l''effacement, fenêtre de l''Explorateur ouverte'
    Proof "juste après l'effacement, $open fenêtre(s) de l'Explorateur ouverte(s) : les trois clés sont vides ($(Show-TypedCounts 'valeurs :'))"
    if ($state['explorateur'] -eq '1') {
        $posted = [EssaiFenetres]::Close($ViewLeaf)
        Wait-ExplorerClosed 20
        Start-Sleep -Seconds 3
        $left = [EssaiFenetres]::Count($ViewLeaf)
        # Une mesure : elle ne fait pas échouer l'essai (c'est elle qui tranche si une fenêtre ouverte réécrit ses saisies en se fermant).
        Proof "MESURE : $open fenêtre(s) avant la fermeture, WM_CLOSE envoyé à $posted, $left après ; ce qui revient dans les trois clés : $(Show-TypedCounts 'valeurs :')"
    } else {
        Proof 'MESURE : aucune fenêtre de l''Explorateur n''avait été vue ouverte, rien à mesurer à la fermeture'
    }
}

# ── 6. Les récents ─────────────────────────────────────────────────────────

function Step-RecentsPreparer {
    [void][IO.Directory]::CreateDirectory($Work)
    $file = Join-Path $Work 'Essai Shinku Recent.txt'
    [IO.File]::WriteAllText($file, 'essai shinku', $Utf8)
    [EssaiShell]::AddRecent($file)
    $route = 'SHAddToRecentDocs'
    $deadline = (Get-Date).AddSeconds(15)
    $link = $null
    while ((Get-Date) -lt $deadline) {
        $link = Get-ChildItem -LiteralPath $RecentFolder -File -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'Essai Shinku Recent*.lnk' } | Select-Object -First 1
        if ($null -ne $link) { break }
        Start-Sleep -Milliseconds 300
    }
    if ($null -eq $link) {
        # Windows a ignoré SHAddToRecentDocs (processus élevé, réglage des éléments récents...) : le script pose lui-même un raccourci.
        Say '  SHAddToRecentDocs n''a pas créé de raccourci en 15 s : repli, le script crée lui-même un raccourci dans Recent'
        $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $RecentFolder 'Essai Shinku Recent.txt.lnk'))
        $shortcut.TargetPath = $file
        $shortcut.Save()
        $route = 'repli : raccourci créé par le script (WScript.Shell), SHAddToRecentDocs n''en a pas créé'
        $link = Get-ChildItem -LiteralPath $RecentFolder -File -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'Essai Shinku Recent*.lnk' } | Select-Object -First 1
        if ($null -eq $link) { Fail "attendu : un raccourci « Essai Shinku Recent*.lnk » dans Recent ; vu : aucun, même après le repli" }
    }
    $count = Get-RecentCount
    Set-StateValue 'recentAvant' ([string]$count)
    Proof "un fichier récent est posé (voie : $route) : raccourci $($link.Name), $count raccourci(s) dans Recent"

    # Un dossier épinglé à l'Accès rapide, si la machine le permet.
    $pinned = Join-Path $Work 'Essai Shinku Epingle'
    [void][IO.Directory]::CreateDirectory($pinned)
    $wanted = Get-ShellPath $pinned
    $invoked = $false
    $seen = $false
    $pinError = ''
    $listed = @()
    try {
        $shell = New-Object -ComObject Shell.Application
        $item = $shell.Namespace((Split-Path -Parent $pinned)).ParseName((Split-Path -Leaf $pinned))
        if ($null -eq $item) { throw "le Shell ne trouve pas le dossier $pinned" }
        try { Say "  verbes du dossier : $((@($item.Verbs() | ForEach-Object { $_.Name }) -join ' | '))" } catch { }
        $item.InvokeVerb('pintohome')
        $invoked = $true
        $deadline = (Get-Date).AddSeconds(15)
        while ((Get-Date) -lt $deadline -and -not $seen) {
            $paths = Get-QuickAccessPaths
            if ($null -ne $paths) { $listed = @($paths) }
            if (Test-Listed $paths @($wanted, $pinned)) { $seen = $true } else { Start-Sleep -Milliseconds 400 }
        }
    } catch {
        $pinError = $_.Exception.Message
    }
    if ($invoked) {
        # Noté dès que le verbe a été appelé : le nettoyage désépingle même si la liste ne l'a pas montré.
        Set-StateValue 'epingle' $pinned
    }
    if ($seen) {
        Set-StateValue 'epingleVu' '1'
        Proof "un dossier est épinglé à l'Accès rapide (verbe pintohome) : $wanted"
    } elseif ($invoked) {
        $shown = (@($listed | Select-Object -First 12)) -join ' | '
        Proof "ÉPINGLÉ NON VU : le verbe pintohome a été appelé sans erreur, mais ni « $wanted » ni « $pinned » n'est dans l'Accès rapide après 15 s (chemins vus : $shown) ; la preuve « l'épinglé reste » n'est pas faite ici ; l'essai continue"
    } else {
        Proof "ÉPINGLAGE IMPOSSIBLE sur cette machine : le verbe pintohome a échoué ($pinError) ; la preuve « l'épinglé reste » n'est pas faite ici ; l'essai continue"
    }
    Show-RecentFolders 'avant'
}

function Step-RecentsVerifier {
    $state = Get-State
    $before = [int]$state['recentAvant']
    $now = Get-RecentCount
    Start-Sleep -Seconds 2
    $later = Get-RecentCount
    $leftover = @(Get-ChildItem -LiteralPath $RecentFolder -File -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'Essai Shinku Recent*.lnk' })
    Show-RecentFolders 'après'
    if ($leftover.Count -gt 0) { Mismatch "attendu : le raccourci posé a disparu de Recent ; vu : $($leftover[0].Name) est toujours là ($now raccourci(s) à la première lecture, $later deux secondes après)" }
    # Pas de verdict sur le délai : cette lecture vient bien après le retour du cœur. Ce qui mesure SHAddToRecentDocs(NULL),
    # c'est l'issue de la page (le cœur relit jusqu'à 5 fois à 100 ms : « Effacé » veut dire zéro raccourci dans ce délai).
    Proof "le récent posé a disparu : $before raccourci(s) avant, $now à la première lecture de cette vérification, $later deux secondes plus tard ; l'issue affichée par la page est journalisée par le pilote"
    if ($state.ContainsKey('epingle') -and $state['epingleVu'] -eq '1') {
        $paths = Get-QuickAccessPaths
        if ($null -eq $paths) {
            Mismatch 'attendu : l''Accès rapide se lit encore ; vu : Namespace renvoie rien'
        } elseif (-not (Test-Listed $paths @((Get-ShellPath $state['epingle']), $state['epingle']))) {
            Mismatch "attendu : le dossier épinglé à l'Accès rapide est toujours là ; vu : il a disparu avec « Fichiers et dossiers récents » (la conception T5 : recent devient alors un bouton vers les Options de l'Explorateur) ; chemins vus : $((@($paths | Select-Object -First 12)) -join ' | ')"
        }
        Proof "le dossier épinglé à l'Accès rapide est toujours là après l'effacement des récents ($(@($paths).Count) élément(s) dans l'Accès rapide)"
    } else {
        Proof 'MESURE : rien n''avait pu être épinglé (ou vu épinglé), la preuve « l''épinglé reste » n''est pas faite sur cette machine'
    }
}

# ── 7. Le presse-papiers ───────────────────────────────────────────────────

function Get-ClipboardHistoryText {
    $parts = New-Object System.Collections.ArrayList
    try {
        $value = (Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Clipboard' -Name 'EnableClipboardHistory' -ErrorAction Stop).EnableClipboardHistory
        [void]$parts.Add("registre EnableClipboardHistory = $value")
    } catch { [void]$parts.Add('registre EnableClipboardHistory absent') }
    try {
        [void][Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType = WindowsRuntime]
        [void]$parts.Add("Clipboard.IsHistoryEnabled() = $([Windows.ApplicationModel.DataTransfer.Clipboard]::IsHistoryEnabled())")
    } catch { [void]$parts.Add("Clipboard.IsHistoryEnabled() impossible : $($_.Exception.Message)") }
    return ($parts -join ' ; ')
}

function Step-PressePapiersPreparer {
    Set-Clipboard -Value 'essai shinku : un premier texte'
    Set-Clipboard -Value 'essai shinku presse-papiers'
    $read = "$(Get-Clipboard -Raw)".Trim()
    if ($read -ne 'essai shinku presse-papiers') { Fail "attendu : le presse-papiers contient le texte posé ; vu : « $read »" }
    Proof "un texte est dans le presse-papiers (« $read »), un autre l'a précédé ; historique de Windows : $(Get-ClipboardHistoryText)"
}

function Step-PressePapiersVerifier {
    $read = "$(Get-Clipboard -Raw)"
    if ($read.Trim() -ne '') { Mismatch "attendu : le presse-papiers est vide ; vu : « $($read.Trim()) »" }
    Proof "le presse-papiers est lu : « $($read.Trim()) » (attendu : vide) ; historique de Windows : $(Get-ClipboardHistoryText)"
    # Une mesure : ce que rend ClearHistory() depuis une application de bureau non empaquetée (ce script en est une).
    try {
        [void][Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType = WindowsRuntime]
        $cleared = [Windows.ApplicationModel.DataTransfer.Clipboard]::ClearHistory()
        Proof "MESURE : Clipboard.ClearHistory() appelé par ce script (application de bureau non empaquetée) rend $cleared"
    } catch {
        Proof "MESURE : Clipboard.ClearHistory() appelé par ce script a échoué : $($_.Exception.Message)"
    }
}

# ── 8. Le vrai Chrome ──────────────────────────────────────────────────────

function Step-ChromeCreer {
    $chrome = Get-ChromePath
    if ($null -eq $chrome) {
        Proof 'Chrome n''est pas installé sur cette machine : la preuve du vrai Chrome n''est pas faite ; l''essai continue'
        exit 10
    }
    Say "  Chrome : $chrome"
    if (@(Get-ChromeProcesses).Count -gt 0) { Fail 'attendu : Chrome est fermé ; vu : un processus chrome.exe tourne' }
    # Chrome crée lui-même son User Data (ou en copie le modèle qu'il a créé).
    Initialize-UserData
    $created = 'modele'
    if (-not (Test-Path -LiteralPath (Join-Path $DefaultProfile 'History'))) {
        $created = 'direct'
        Start-Chrome 'chrome-creer' { Test-Path -LiteralPath (Join-Path $DefaultProfile 'History') } 'Chrome crée son profil (Default\History)'
        Start-Sleep -Seconds 3
        Stop-Chrome
    }
    $files = @(Get-ChildItem -LiteralPath $DefaultProfile -File -Force -ErrorAction SilentlyContinue | ForEach-Object { '{0} ({1})' -f $_.Name, $_.Length })
    $from = 'créé par Chrome à l''instant (--user-data-dir)'
    if ($created -eq 'modele') { $from = 'copié du modèle que Chrome a créé plus tôt dans cet essai' }
    Proof "profil $from, $UserData ; Chrome sans fenêtre puis s'est arrêté (plus aucun processus chrome.exe) : $($files -join ', ')"
}

function Step-ChromeOuvrir {
    $lock = Join-Path $UserData 'lockfile'
    Start-Chrome 'chrome-ouvrir' { Test-Path -LiteralPath $lock } 'Chrome tourne et a posé son lockfile'
    Start-Sleep -Seconds 2
    $processes = (Get-ChromeProcesses).Count
    Proof "Chrome tourne ($processes processus chrome.exe) ; lockfile présent : $(Test-Path -LiteralPath $lock) ; ouverture lecture+écriture (partage total) comme le cœur la fait : $(Test-LockfileOpen $lock)"
}

function Step-ChromeFermer {
    Stop-Chrome
    $lock = Join-Path $UserData 'lockfile'
    Proof "Chrome est fermé (plus aucun processus chrome.exe) ; lockfile : $(if (Test-Path -LiteralPath $lock) { 'toujours là' } else { 'absent' })"
}

function Step-ChromeAvant {
    if (@(Get-ChromeProcesses).Count -gt 0) { Fail 'attendu : Chrome est fermé avant l''effacement ; vu : un processus chrome.exe tourne' }
    $inLot = @($Lot | Where-Object { Test-Path -LiteralPath (Join-Path $DefaultProfile $_) -PathType Leaf })
    if ($inLot -notcontains 'History') { Fail "attendu : Chrome a créé Default\History ; vu : $(Describe-Lot $DefaultProfile)" }
    $protected = Get-ChromeProtected
    Save-Tree $protected 'chrome-avant'
    Set-StateValue 'chromeLot' ($inLot -join '|')
    Proof "juste avant l'effacement : fichiers de la règle : $(Describe-Lot $DefaultProfile) ; empreintes SHA-256 prises de : $((@($protected.Keys | Sort-Object)) -join ', ')"
    foreach ($name in @('Default\Bookmarks', 'Default\Login Data', 'Default\Preferences', 'Local State')) {
        if (-not $protected.ContainsKey($name)) { Say "  (note : Chrome n'a pas créé $name sur cette machine)" }
    }
}

function Step-ChromeApres {
    if (@(Get-ChromeProcesses).Count -gt 0) { Mismatch 'attendu : Chrome est fermé après l''effacement ; vu : un processus chrome.exe tourne' }
    $state = Get-State
    $wasThere = @($state['chromeLot'] -split '\|' | Where-Object { $_ })
    $still = @($wasThere | Where-Object { Test-Path -LiteralPath (Join-Path $DefaultProfile $_) -PathType Leaf })
    if ($still.Count -gt 0) { Mismatch "attendu : les $($wasThere.Count) fichiers de la règle ont disparu ; vu : toujours là : $($still -join ', ')" }
    Proof "fichiers de la règle de Default qui étaient là avant ($($wasThere.Count)) : $($wasThere -join ', ') ; encore là : $($still.Count)"
    $before = Load-Tree 'chrome-avant'
    $after = Get-ChromeProtected
    Assert-Tree 'Bookmarks, Login Data, Preferences et Local State entre juste avant et juste après l''effacement' $before $after @() 'empreintes identiques'
    Proof "mêmes empreintes SHA-256 juste avant et juste après l'effacement pour les fichiers que Chrome avait créés : $((@($after.Keys | Sort-Object)) -join ', ')"
    # Constat : on s'arrête là, sans relancer Chrome.
    if ($Constat) { return }
    # Il redémarre, et recrée son historique.
    Remove-Item -LiteralPath (Join-Path $DefaultProfile 'History') -Force -ErrorAction SilentlyContinue
    Start-Chrome 'chrome-apres' { Test-Path -LiteralPath (Join-Path $DefaultProfile 'History') } 'Chrome redémarre et recrée Default\History'
    $processes = (Get-ChromeProcesses).Count
    Proof "Chrome relancé démarre ($processes processus chrome.exe) et recrée History : $(Describe-Lot $DefaultProfile)"
    Start-Sleep -Seconds 2
    Stop-Chrome
}

# ── Tout remettre en place ─────────────────────────────────────────────────

function Step-Nettoyer([bool]$Final) {
    $problems = New-Object System.Collections.ArrayList
    $state = Get-State
    $run = {
        param([string]$What, [scriptblock]$Action)
        try { & $Action } catch {
            [void]$problems.Add("$What : $($_.Exception.Message)")
            Say "  $What : $($_.Exception.Message)"
        }
    }
    # Si le délai de la CI a tué le pilote, la fenêtre de Shinku peut être restée
    # ouverte : l'étape suivante de la CI la trouverait en marche.
    if ($Final) {
        & $run 'fenêtre de Shinku restée ouverte' {
            Get-Process -Name 'shinku' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        }
    }
    & $run 'processus qui tient un fichier' { Stop-Holder }
    if ($state['chromeUsed'] -eq '1') { & $run 'Chrome' { Stop-Chrome } }
    & $run 'fenêtre de l''Explorateur' {
        [void][EssaiFenetres]::Close($ViewLeaf)
        Wait-ExplorerClosed 10
        $left = [EssaiFenetres]::Count($ViewLeaf)
        if ($left -gt 0) { Fail "$left fenêtre(s) de l'Explorateur « $ViewLeaf » encore ouverte(s)" }
    }
    if ($state.ContainsKey('epingle')) {
        & $run 'dossier épinglé' {
            $shell = New-Object -ComObject Shell.Application
            $folder = $shell.Namespace($QuickAccess)
            $names = @((Get-ShellPath $state['epingle']), $state['epingle'])
            if ($null -ne $folder) {
                foreach ($item in @($folder.Items())) { if (Test-Listed @([string]$item.Path) $names) { $item.InvokeVerb('unpinfromhome') } }
            }
        }
    }
    & $run 'piège du registre (lien RunMRU et sa cible)' { Remove-RegistryTrap }
    if ($state['registreTouche'] -eq '1') {
        & $run 'valeurs des trois clés' {
            foreach ($name in $TypedKeys) {
                $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("$ExplorerKey\$name", $true)
                if ($null -ne $key) { try { foreach ($old in @($key.GetValueNames())) { $key.DeleteValue($old, $false) } } finally { $key.Close() } }
            }
        }
    }
    & $run 'raccourci récent' {
        foreach ($link in @(Get-ChildItem -LiteralPath $RecentFolder -File -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'Essai Shinku Recent*.lnk' })) { Remove-Item -LiteralPath $link.FullName -Force }
    }
    if ($state['userDataOurs'] -eq '1') {
        & $run 'User Data de l''essai' {
            Remove-TreeSafe $UserData
            Clear-StateValue 'userDataOurs'
        }
    }
    if ($Final -and $state['aside'] -eq '1') {
        & $run 'User Data d''origine' {
            if (Test-Path -LiteralPath $UserData) { Fail "le User Data de l'essai est encore là : $UserData" }
            [IO.Directory]::Move($Aside, $UserData)
            Clear-StateValue 'aside'
        }
    }
    & $run 'dossier de la fenêtre de l''Explorateur' { Remove-TreeSafe $ViewFolder }
    if ($problems.Count -gt 0) {
        # L'état est gardé : le nettoyage suivant sait encore ce qu'il reste à remettre.
        Fail "nettoyage incomplet (l'état est gardé dans $StateFile) : $($problems -join ' ; ')"
    }
    if ($Final) {
        & $run 'dossier de travail' { Remove-TreeSafe $Work }
    } else {
        & $run 'dossier de travail' {
            foreach ($child in @(Get-ChildItem -LiteralPath $Work -Force -ErrorAction SilentlyContinue)) {
                if ($child.Name -ne 'etat.txt' -and $child.Name -ne 'modele-chrome') { Remove-TreeSafe $child.FullName }
            }
            foreach ($key in @('holderPid', 'epingle', 'epingleVu', 'explorateur', 'registreTouche', 'lienRegistre', 'recentAvant', 'remplaceId', 'chromeUsed', 'chromeLot', 'lockfileMode')) { Clear-StateValue $key }
        }
    }
    if ($problems.Count -gt 0) { Fail "nettoyage incomplet : $($problems -join ' ; ')" }
    Say 'machine d''essai remise en place'
}

# ── Programme ──────────────────────────────────────────────────────────────

$exitCode = 0
try {
    Say "étape $Etape sur Windows $([Environment]::OSVersion.Version)"
    switch ($Etape) {
        'nettoyer' { Step-Nettoyer $true }
        'nettoyer-scenario' { Step-Nettoyer $false }
        'profil-preparer' { Step-ProfilPreparer }
        'profil-verifier' { Step-ProfilVerifier }
        'verrou-fichier-preparer' { Step-VerrouFichierPreparer }
        'verrou-fichier-verifier' { Step-VerrouFichierVerifier }
        'verrou-lockfile-preparer' { Step-VerrouLockfilePreparer }
        'verrou-lockfile-verifier' { Step-VerrouLockfileVerifier }
        'jonction-preparer' { Step-JonctionPreparer }
        'jonction-verifier' { Step-JonctionVerifier }
        'lien-physique-preparer' { Step-LienPhysiquePreparer }
        'lien-physique-verifier' { Step-LienPhysiqueVerifier }
        'lien-registre-preparer' { Step-LienRegistrePreparer }
        'lien-registre-verifier' { Step-LienRegistreVerifier }
        'profil-remplace-preparer' { Step-ProfilRemplacePreparer }
        'profil-remplace-remplacer' { Step-ProfilRemplaceRemplacer }
        'profil-remplace-verifier' { Step-ProfilRemplaceVerifier }
        'registre-preparer' { Step-RegistrePreparer }
        'registre-verifier' { Step-RegistreVerifier }
        'registre-explorateur-preparer' { Step-RegistreExplorateurPreparer }
        'registre-explorateur-verifier' { Step-RegistreExplorateurVerifier }
        'recents-preparer' { Step-RecentsPreparer }
        'recents-verifier' { Step-RecentsVerifier }
        'presse-papiers-preparer' { Step-PressePapiersPreparer }
        'presse-papiers-verifier' { Step-PressePapiersVerifier }
        'chrome-creer' { Step-ChromeCreer }
        'chrome-ouvrir' { Step-ChromeOuvrir }
        'chrome-fermer' { Step-ChromeFermer }
        'chrome-avant' { Step-ChromeAvant }
        'chrome-apres' { Step-ChromeApres }
    }
} catch {
    $exitCode = 1
    Say "ECHEC ($Etape) : $($_.Exception.Message)"
    if ($_.InvocationInfo -and $_.InvocationInfo.PositionMessage) {
        foreach ($where in ("$($_.InvocationInfo.PositionMessage)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
    if ($_.ScriptStackTrace) {
        foreach ($where in ("$($_.ScriptStackTrace)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
}
exit $exitCode
