# Essai de bout en bout du mode jeu : le vrai shinku-agent.exe reconnaît-il un
# jeu, applique-t-il les réglages, puis remet-il tout en place ?
#
# Les tests du cœur essaient la décision avec une fausse machine. Celui-ci
# essaie l'agent installé, sur un vrai Windows, avec un vrai jeu au premier
# plan, un vrai mode d'alimentation et un vrai processus d'arrière-plan :
#
#   1. le mode d'alimentation est noté, puis forcé sur « Utilisation normale »
#      (sans cela, l'essai ne prouverait rien) ;
#   2. un settings.json d'essai allume le mode automatique, déclare le jeu
#      d'essai et met ping.exe en veilleuse ;
#   3. un ping, l'agent, puis le « jeu » (une petite fenêtre) sont lancés avec
#      les droits ordinaires d'un utilisateur : les machines de l'intégration
#      continue tournent en administrateur, et l'agent refuse de le faire ;
#   4. le jeu au premier plan : une session apparaît dans game.json, le ping est
#      en priorité basse, l'alimentation passe en « Performances élevées » (si
#      ce mode existe sur la machine) ;
#   5. le jeu est fermé : la session disparaît, le bilan est complet, le ping et
#      l'alimentation sont revenus ;
#   6. l'agent est arrêté par `--quitter`, et ne tourne plus.
#
# Le jeu est une petite application compilée à l'instant, avec son propre
# chemin : un powershell.exe aurait le chemin de tous les autres processus
# PowerShell de la machine (dont celui de cet essai), et l'agent ne verrait
# jamais la partie se terminer.
#
# Tout ce qui est changé est remis en place à la fin, même en cas d'échec ;
# `-Restaurer` le refait seul (étape « toujours » de l'intégration continue).
# À lancer sur une machine d'essai.
#
#   powershell -File scripts/essai-mode-jeu.ps1 -Agent "C:\Program Files\Shinku\shinku-agent.exe"
#
# Ce fichier commence par une marque d'ordre des octets (BOM) : sans elle,
# Windows PowerShell 5 lirait ses lettres accentuées de travers.

param(
    [string]$Agent,
    # Temps laissé à chaque attente, en secondes.
    [int]$Attente = 60,
    # Remet en place ce qu'un essai précédent a changé, puis s'arrête.
    [switch]$Restaurer
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }

$Balanced = '381b4222-f694-41f0-9685-ff5bb260df2e'
$HighPerformance = '8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c'

$Temporary = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$Work = Join-Path $Temporary 'essai-mode-jeu'
$StateFile = Join-Path $Work 'etat.json'
$SettingsFile = Join-Path $env:APPDATA 'Shinku\settings.json'
$GameFile = Join-Path $env:LOCALAPPDATA 'Shinku\game.json'
$Launcher = Join-Path $PSScriptRoot 'lancer-en-utilisateur.ps1'
$GameProgram = Join-Path $Work 'jeu\JeuEssai.exe'

$Started = Get-Date
function Say([string]$Line) {
    $seconds = ((Get-Date) - $Started).TotalSeconds
    Write-Host ('[{0,6:N1} s] {1}' -f $seconds, $Line)
}
function Done([string]$Line) { Say "OK $Line" }
function Fail([string]$Message) { throw $Message }

# ── Ce qu'il faut remettre ─────────────────────────────────────────────────

# L'état d'origine, écrit avant le premier changement : l'étape « toujours » de
# l'intégration continue le relit si cet essai a été interrompu.
$State = @{ scheme = ''; settings = $false; game = $false; agent = ''; pids = @() }

function Save-State {
    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    $pids = '[' + (($State.pids | ForEach-Object { [string]$_ }) -join ',') + ']'
    $text = '{{"scheme":"{0}","settings":{1},"game":{2},"agent":"{3}","pids":{4}}}' -f `
        $State.scheme, $State.settings.ToString().ToLower(), $State.game.ToString().ToLower(), `
        ($State.agent -replace '\\', '\\'), $pids
    [IO.File]::WriteAllText($StateFile, $text, (New-Object System.Text.UTF8Encoding $false))
}

function Active-Scheme {
    $text = (& powercfg /getactivescheme) -join ' '
    if ($text -match '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})') {
        return $Matches[1].ToLower()
    }
    Fail "powercfg ne donne pas le mode d'alimentation actif : $text"
}

function Scheme-Name([string]$Guid) {
    switch ($Guid) {
        $Balanced { return 'Utilisation normale' }
        $HighPerformance { return 'Performances élevées' }
        default { return $Guid }
    }
}

function Register-Process([int]$ProcessId) {
    $State.pids = @($State.pids) + $ProcessId
    Save-State
}

function Restore-Everything {
    if (-not (Test-Path $StateFile)) { return }
    $saved = Get-Content -Raw -Encoding UTF8 $StateFile | ConvertFrom-Json
    Say "remise en place de ce que l'essai a changé"

    # L'agent d'abord : il remet lui-même ce qu'il a changé, puis on arrête ce qui reste.
    if ($saved.agent -and (Test-Path $saved.agent)) {
        try {
            $quit = Start-Process -FilePath $saved.agent -ArgumentList '--quitter' -Wait -PassThru -WindowStyle Hidden
            Say "  agent : --quitter rend le code $($quit.ExitCode)"
        } catch { Say "  agent : --quitter impossible ($($_.Exception.Message))" }
    }
    Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    foreach ($id in @($saved.pids)) {
        Stop-Process -Id ([int]$id) -Force -ErrorAction SilentlyContinue
    }

    if ($saved.scheme) {
        & powercfg /setactive $saved.scheme | Out-Null
        Say "  alimentation : $(Scheme-Name (Active-Scheme))"
    }
    foreach ($entry in @(@($SettingsFile, 'settings', $saved.settings), @($GameFile, 'game', $saved.game))) {
        $file = $entry[0]
        $saveCopy = Join-Path $Work "$($entry[1]).json.origine"
        if ($entry[2]) {
            Copy-Item -Force $saveCopy $file
        } elseif (Test-Path $file) {
            Remove-Item -Force $file
        }
    }
    Say '  réglages et journal d''origine remis'
    Remove-Item -Force $StateFile -ErrorAction SilentlyContinue
}

if ($Restaurer) {
    Restore-Everything
    exit 0
}

if (-not $Agent) {
    [Console]::Error.WriteLine('Usage : essai-mode-jeu.ps1 -Agent <shinku-agent.exe> [-Attente <secondes>]')
    exit 2
}
if (-not (Test-Path $Agent)) {
    [Console]::Error.WriteLine("Agent introuvable : $Agent")
    exit 2
}

# ── Les outils de l'essai ──────────────────────────────────────────────────

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class Foreground
{
    [DllImport("user32.dll")]
    static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);

    // Le numéro du processus dont la fenêtre est au premier plan, ou 0.
    public static int Owner()
    {
        uint process;
        GetWindowThreadProcessId(GetForegroundWindow(), out process);
        return (int)process;
    }
}
'@

# Le « jeu » : une fenêtre qui réclame le premier plan. Windows ne le donne pas
# toujours à un programme qui n'a reçu aucune saisie : la fenêtre simule une
# pression sur Alt (la façon documentée de lever ce refus) et réessaie.
function Build-Game {
    New-Item -ItemType Directory -Force -Path (Split-Path $GameProgram) | Out-Null
    Add-Type -ReferencedAssemblies 'System.Windows.Forms', 'System.Drawing' `
        -OutputAssembly $GameProgram -OutputType WindowsApplication -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public static class JeuEssai
{
    [DllImport("user32.dll")]
    static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    static extern bool BringWindowToTop(IntPtr window);

    [DllImport("user32.dll")]
    static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);

    [STAThread]
    public static void Main()
    {
        Form form = new Form();
        form.Text = "Jeu d'essai de Shinku";
        form.StartPosition = FormStartPosition.CenterScreen;
        form.Size = new Size(480, 320);
        form.TopMost = true;
        int tries = 0;
        System.Windows.Forms.Timer timer = new System.Windows.Forms.Timer();
        timer.Interval = 500;
        timer.Tick += delegate
        {
            if (GetForegroundWindow() != form.Handle && tries < 40)
            {
                tries++;
                keybd_event(0x12, 0, 0, UIntPtr.Zero);
                keybd_event(0x12, 0, 2, UIntPtr.Zero);
                SetForegroundWindow(form.Handle);
                BringWindowToTop(form.Handle);
            }
        };
        form.Shown += delegate { form.Activate(); timer.Start(); };
        Application.Run(form);
    }
}
'@
}

# Lance un programme avec les droits ordinaires d'un utilisateur, sans attendre
# sa fin. Rend le numéro du programme (pas celui du lanceur).
function Start-AsUser([string]$Program, [string]$Arguments = '') {
    $out = Join-Path $Work ("lanceur-{0}.txt" -f [IO.Path]::GetFileNameWithoutExtension($Program))
    Remove-Item -Force $out -ErrorAction SilentlyContinue
    $parameters = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $Launcher, '-Programme', $Program)
    if ($Arguments) { $parameters += @('-Arguments', $Arguments) }
    $launcher = Start-Process -FilePath 'powershell' -ArgumentList $parameters -PassThru `
        -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError "$out.err"
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $out) {
            $line = Get-Content $out -ErrorAction SilentlyContinue | Where-Object { $_ -match '^PID (\d+)' } | Select-Object -First 1
            if ($line -match '^PID (\d+)') {
                $id = [int]$Matches[1]
                Register-Process $id
                return $id
            }
        }
        if ($launcher.HasExited) { break }
        Start-Sleep -Milliseconds 200
    }
    $reason = if (Test-Path "$out.err") { (Get-Content "$out.err") -join ' ' } else { '' }
    Fail "le lanceur n'a pas pu démarrer $Program ($reason)"
}

function Read-Game {
    if (-not (Test-Path $GameFile)) { return $null }
    try { return Get-Content -Raw -Encoding UTF8 $GameFile | ConvertFrom-Json } catch { return $null }
}

function Priority-Of([int]$ProcessId) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process) { return 'absent' }
    return [string]$process.PriorityClass
}

function Foreground-Name {
    $owner = [Foreground]::Owner()
    if ($owner -eq 0) { return 'aucun' }
    $process = Get-Process -Id $owner -ErrorAction SilentlyContinue
    if ($null -eq $process) { return "processus $owner" }
    return $process.ProcessName
}

# Ce que l'essai voit en ce moment, sur une ligne.
function Observe([int]$PingId) {
    $game = Read-Game
    $session = if ($null -eq $game) { 'pas de journal' } elseif ($null -eq $game.session) { 'aucune session' } else { "session de $($game.session.game.name), alimentation $($game.session.power.outcome)" }
    return "premier plan : $(Foreground-Name) ; $session ; ping : $(Priority-Of $PingId) ; alimentation : $(Scheme-Name (Active-Scheme))"
}

# Répète une question jusqu'à une réponse vraie ; dit ce qu'il voit chaque
# fois que cela change (et au moins toutes les dix secondes).
function Wait-For([string]$What, [int]$PingId, [scriptblock]$Test) {
    $deadline = (Get-Date).AddSeconds($Attente)
    $lastLine = ''
    $lastTime = Get-Date
    while ($true) {
        $answer = & $Test
        if ($answer) { return $answer }
        $line = Observe $PingId
        if ($line -ne $lastLine -or ((Get-Date) - $lastTime).TotalSeconds -ge 10) {
            Say "  en attendant : $line"
            $lastLine = $line
            $lastTime = Get-Date
        }
        if ((Get-Date) -ge $deadline) {
            Fail "$What : toujours pas après $Attente s`n$line"
        }
        Start-Sleep -Seconds 1
    }
}

function Json-String([string]$Text) { '"' + ($Text -replace '\\', '\\' -replace '"', '\"') + '"' }

# ── L'essai ────────────────────────────────────────────────────────────────

$exitCode = 0
try {
    Say "Windows $([Environment]::OSVersion.Version), agent $Agent"
    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    if (Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue) {
        Fail "un agent tourne déjà sur cette machine : l'essai ne l'arrête pas"
    }

    # État d'origine, noté avant toute modification.
    $State.scheme = Active-Scheme
    $State.agent = $Agent
    foreach ($entry in @(@($SettingsFile, 'settings'), @($GameFile, 'game'))) {
        $has = Test-Path $entry[0]
        if ($has) { Copy-Item -Force $entry[0] (Join-Path $Work "$($entry[1]).json.origine") }
        $State[$entry[1]] = $has
    }
    Save-State
    Say "alimentation d'origine : $(Scheme-Name $State.scheme)"

    # 1. L'alimentation : « Utilisation normale », pour que l'essai prouve quelque chose.
    $schemes = (& powercfg /list) -join "`n"
    $hasHigh = $schemes -match $HighPerformance
    & powercfg /setactive $Balanced | Out-Null
    if ((Active-Scheme) -ne $Balanced) { Fail "impossible de passer l'alimentation en « Utilisation normale »" }
    Done "alimentation forcée sur « Utilisation normale » ; « Performances élevées » $(if ($hasHigh) { 'existe' } else { 'n''existe pas sur cette machine' })"

    # 2. Les réglages d'essai.
    Build-Game
    if (-not (Test-Path $GameProgram)) { Fail "le jeu d'essai n'a pas pu être compilé" }
    $settings = '{{"version":1,"gameModeAuto":true,"gameModePower":true,"gameModeQuietApps":true,' +
        '"gameModeQuietAdded":["ping.exe"],"gameModeQuietRemoved":[],"gameModeGames":[{0}],"gameModeNotGames":[]}}'
    New-Item -ItemType Directory -Force -Path (Split-Path $SettingsFile) | Out-Null
    [IO.File]::WriteAllText($SettingsFile, ($settings -f (Json-String $GameProgram)), (New-Object System.Text.UTF8Encoding $false))
    if (Test-Path $GameFile) { Remove-Item -Force $GameFile }
    Done "réglages d'essai écrits : mode automatique, jeu $GameProgram, ping.exe en veilleuse"

    # 3. Le ping, l'agent, puis le jeu.
    $pingId = Start-AsUser (Join-Path $env:SystemRoot 'System32\PING.EXE') '-t 127.0.0.1'
    Done "ping lancé (processus $pingId), priorité $(Priority-Of $pingId)"
    if ((Priority-Of $pingId) -ne 'Normal') { Fail "le ping devait partir en priorité normale" }

    $agentId = Start-AsUser $Agent
    Start-Sleep -Seconds 3
    if ($null -eq (Get-Process -Id $agentId -ErrorAction SilentlyContinue)) {
        Fail "l'agent s'est arrêté aussitôt (un agent lancé en administrateur refuse de tourner : code 3)"
    }
    Done "l'agent tourne (processus $agentId)"
    Start-Sleep -Seconds 4
    if ((Priority-Of $pingId) -ne 'Normal') { Fail "sans jeu au premier plan, le ping ne devait pas être touché" }
    if ($null -ne (Read-Game) -and $null -ne (Read-Game).session) { Fail "sans jeu au premier plan, l'agent ne devait pas ouvrir de session" }
    Done "sans jeu au premier plan, rien n'est touché"

    $gameId = Start-AsUser $GameProgram
    Done "le jeu est lancé (processus $gameId)"
    $front = $null
    try {
        $front = Wait-For "le jeu obtient le premier plan" $pingId { [Foreground]::Owner() -eq $gameId }
    } catch {
        Fail "premier plan refusé par la machine d'essai : $($_.Exception.Message)"
    }
    Done 'le jeu est au premier plan'

    # 4. L'agent le reconnaît et applique les réglages.
    $gamePath = $GameProgram.ToLower()
    $session = Wait-For "l'agent ouvre une session pour le jeu" $pingId {
        $game = Read-Game
        if ($null -ne $game -and $null -ne $game.session -and $game.session.game.path.ToLower() -eq $gamePath) { $game.session }
    }
    Done "session ouverte pour $($session.game.name) ($($session.game.source)), alimentation : $($session.power.outcome)"

    Wait-For "le ping passe en priorité basse" $pingId { (Priority-Of $pingId) -eq 'BelowNormal' } | Out-Null
    Done 'le ping est en priorité basse (BelowNormal)'

    $outcome = (Read-Game).session.power.outcome
    if ($hasHigh) {
        if ($outcome -ne 'applied') { Fail "« Performances élevées » existe, l'alimentation devait être appliquée ; l'agent dit : $outcome" }
        Wait-For "l'alimentation passe en « Performances élevées »" $pingId { (Active-Scheme) -eq $HighPerformance } | Out-Null
        Done "l'alimentation est en « Performances élevées »"
    } else {
        if ($outcome -ne 'unavailable') { Fail "sans « Performances élevées », l'agent devait dire « unavailable » ; il dit : $outcome" }
        Done "« Performances élevées » n'existe pas : l'agent le dit (unavailable), l'alimentation n'est pas essayée"
    }

    # 5. Le jeu se ferme : tout revient.
    $game = Get-Process -Id $gameId
    [void]$game.CloseMainWindow()
    if (-not $game.WaitForExit(10000)) { Stop-Process -Id $gameId -Force }
    Done 'le jeu est fermé'
    $last = Wait-For "la session se termine" $pingId {
        $file = Read-Game
        if ($null -ne $file -and $null -eq $file.session -and $null -ne $file.last) { $file.last }
    }
    if (-not $last.complete) { Fail "le bilan de la partie dit que tout n'a pas été remis en place" }
    Done "session terminée, bilan complet (alimentation remise : $($last.powerRestored), apps remises : $($last.appsRestored))"
    Wait-For "le ping retrouve sa priorité normale" $pingId { (Priority-Of $pingId) -eq 'Normal' } | Out-Null
    Done 'le ping a retrouvé sa priorité normale'
    Wait-For "l'alimentation revient en « Utilisation normale »" $pingId { (Active-Scheme) -eq $Balanced } | Out-Null
    Done "l'alimentation est revenue en « Utilisation normale »"

    # 6. L'agent s'arrête sur demande.
    $quit = Start-Process -FilePath $Agent -ArgumentList '--quitter' -Wait -PassThru -WindowStyle Hidden
    if ($quit.ExitCode -ne 0) { Fail "--quitter rend le code $($quit.ExitCode) au lieu de 0" }
    $left = Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue
    if ($left) { Fail "après --quitter, l'agent tourne encore (processus $($left.Id))" }
    Done "--quitter arrête l'agent (code 0), plus aucun agent ne tourne"

    Say 'Essai réussi.'
} catch {
    $exitCode = 1
    Say "ECHEC : $($_.Exception.Message)"
    Say 'ce que la machine montrait :'
    try {
        Say "  premier plan : $(Foreground-Name)"
        $file = Read-Game
        Say "  game.json : $(if ($null -eq $file) { 'absent ou illisible' } else { ($file | ConvertTo-Json -Depth 6 -Compress) })"
        $agents = @(Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue)
        Say "  agents en marche : $($agents.Count)"
    } catch { Say "  (diagnostic impossible : $($_.Exception.Message))" }
} finally {
    try { Restore-Everything } catch { Say "remise en place incomplète : $($_.Exception.Message)"; $exitCode = 1 }
}
exit $exitCode
