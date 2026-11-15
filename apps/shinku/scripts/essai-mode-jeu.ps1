# Essai de bout en bout du mode jeu : le vrai shinku-agent.exe reconnaît-il un
# jeu, applique-t-il les réglages, puis remet-il tout en place ?
#
# Les tests du cœur essaient la décision avec une fausse machine. Celui-ci
# essaie l'agent installé, sur un vrai Windows, avec un vrai jeu au premier
# plan, un vrai mode d'alimentation et un vrai processus d'arrière-plan :
#
#   1. le mode d'alimentation est noté, puis forcé sur « Utilisation normale »
#      (sans cela, l'essai ne prouverait rien) ;
#   2. une sonde dit ce que la machine permet : un compte ordinaire (jeton
#      filtré, comme celui de l'agent) peut-il passer en « Performances
#      élevées » ? Sur un Windows de particulier, oui. Sur un Windows Server,
#      comme les machines de GitHub, PowerSetActiveScheme lui est refusé. La
#      sonde, la description de sécurité de l'action et les groupes du jeton
#      ordinaire sont affichés pour le compte rendu ;
#   3. un settings.json d'essai allume le mode automatique, déclare le jeu
#      d'essai, met ping.exe en veilleuse et retire les trois apps cochées
#      d'office (OneDrive, Dropbox, Google Drive) : l'essai ne touche que le
#      ping ;
#   4. un ping, l'agent, puis le « jeu » (une petite fenêtre) sont lancés avec
#      les droits ordinaires d'un utilisateur : les machines de l'intégration
#      continue tournent en administrateur, et l'agent refuse de le faire ;
#   5. le jeu au premier plan : une session apparaît dans game.json, le ping est
#      en priorité basse. Si la sonde a réussi, l'agent doit dire « applied » et
#      l'alimentation passer en « Performances élevées » ; si elle a échoué,
#      l'agent doit dire « refused » et l'alimentation ne pas bouger. Dans ce
#      second cas l'essai le dit en toutes lettres : le passage en
#      « Performances élevées » n'est pas prouvé sur cette machine ;
#   6. le jeu est fermé : la session disparaît, le bilan est complet, le ping et
#      l'alimentation sont revenus ;
#   7. l'agent est arrêté par `--quitter`, et ne tourne plus.
#
# Le jeu est une petite application compilée à l'instant, avec son propre
# chemin : un powershell.exe aurait le chemin de tous les autres processus
# PowerShell de la machine (dont celui de cet essai), et l'agent ne verrait
# jamais la partie se terminer.
#
# Tout ce qui est changé est remis en place à la fin, même en cas d'échec ;
# `-Restaurer` le refait seul (étape « toujours » de l'intégration continue), et
# le dossier de travail est alors effacé. Un essai interrompu sans remise en
# place (fenêtre fermée, coupure) laisse son état sur le disque : le prochain
# essai commence par le remettre en place. Les processus de l'essai ne sont
# arrêtés que si leur chemin et leur heure de départ sont ceux notés au
# lancement : un numéro de processus ne dit rien après un redémarrage. L'essai
# n'arrête jamais un agent qu'il n'a pas lancé : s'il en trouve un au départ, il
# s'arrête avec un message. À lancer sur une machine d'essai.
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

$ProbeDir = Join-Path $env:TEMP 'essai-mode-jeu-sonde'

function Json-String([string]$Text) { '"' + ($Text -replace '\\', '\\' -replace '"', '\"') + '"' }

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
# Chaque processus lancé est noté avec son chemin et son heure de départ (en
# ticks UTC) : on ne l'arrêtera que si les deux correspondent encore.
$State = @{ scheme = ''; settings = $false; game = $false; agent = ''; procs = @() }

function Save-State {
    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    $entries = @($State.procs | ForEach-Object {
        '{{"id":{0},"path":{1},"start":"{2}"}}' -f $_.id, (Json-String $_.path), $_.start
    })
    $text = '{{"scheme":"{0}","settings":{1},"game":{2},"agent":{3},"procs":[{4}]}}' -f `
        $State.scheme, $State.settings.ToString().ToLower(), $State.game.ToString().ToLower(), `
        (Json-String $State.agent), ($entries -join ',')
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
    $path = ''
    $start = ''
    try {
        $process = Get-Process -Id $ProcessId -ErrorAction Stop
        $path = [string]$process.Path
        $start = [string]$process.StartTime.ToUniversalTime().Ticks
    } catch { }
    # Sans chemin ni heure, le processus ne sera jamais arrêté : mieux vaut en laisser un que tuer un autre.
    $State.procs = @($State.procs) + [pscustomobject]@{ id = $ProcessId; path = $path; start = $start }
    Save-State
}

# Le processus que l'essai avait noté, s'il tourne encore : même numéro, même
# chemin, même heure de départ. Sinon, $null.
function Find-Own($Entry) {
    if (-not $Entry.path -or -not $Entry.start) { return $null }
    $process = Get-Process -Id ([int]$Entry.id) -ErrorAction SilentlyContinue
    if ($null -eq $process) { return $null }
    try {
        if (([string]$process.Path -ieq [string]$Entry.path) -and
            ([string]$process.StartTime.ToUniversalTime().Ticks -eq [string]$Entry.start)) { return $process }
    } catch { }
    return $null
}

function Clean-Work {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $ProbeDir -ErrorAction SilentlyContinue
}

function Restore-Everything {
    if (-not (Test-Path $StateFile)) {
        # Rien n'a été changé : il reste au plus un dossier de travail.
        Clean-Work
        return
    }
    $saved = Get-Content -Raw -Encoding UTF8 $StateFile | ConvertFrom-Json
    Say "remise en place de ce que l'essai a changé"

    # Les processus que l'essai avait lancés et qui tournent encore (même
    # numéro, même chemin, même heure de départ). Un autre agent, celui de la
    # personne par exemple, n'est pas à l'essai : `--quitter` l'arrêterait aussi,
    # alors on ne l'envoie pas tant qu'il tourne.
    $ownIds = @(@($saved.procs) | ForEach-Object { $found = Find-Own $_; if ($null -ne $found) { $found.Id } })
    $foreign = @(Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue | Where-Object { $ownIds -notcontains $_.Id })
    $hasAgent = $saved.agent -and (Test-Path $saved.agent)
    if ($foreign.Count -gt 0) {
        Say "  un agent qui n'est pas celui de l'essai tourne (processus $(($foreign | ForEach-Object { $_.Id }) -join ', ')) : il n'est pas touché, et --quitter n'est pas envoyé"
        $hasAgent = $false
    }
    # L'agent d'abord : il remet lui-même ce qu'il a changé, puis on arrête ce qui reste.
    if ($hasAgent) {
        try {
            $quit = Start-Process -FilePath $saved.agent -ArgumentList '--quitter' -Wait -PassThru -WindowStyle Hidden
            Say "  agent : --quitter rend le code $($quit.ExitCode)"
        } catch { Say "  agent : --quitter impossible ($($_.Exception.Message))" }
    }
    # Seulement ce que l'essai a lancé : même chemin et même heure de départ.
    $killedAgent = $false
    foreach ($entry in @($saved.procs)) {
        $own = Find-Own $entry
        if ($null -eq $own) { continue }
        # Le premier --quitter a pu faire sortir l'agent à l'instant : Find-Own le revérifie.
        Stop-Process -Id $own.Id -Force -ErrorAction SilentlyContinue
        if ($saved.agent -and ([string]$entry.path -ieq [string]$saved.agent)) { $killedAgent = $true }
    }
    # Un agent arrêté de force n'a rien remis en place. Plus aucun agent ne
    # répond : `--quitter` reprend lui-même la partie restée au journal. Cela
    # doit se faire avant de remettre game.json, qui est cette trace.
    if ($killedAgent -and $hasAgent) {
        try {
            $quit = Start-Process -FilePath $saved.agent -ArgumentList '--quitter' -Wait -PassThru -WindowStyle Hidden
            Say "  agent arrêté de force : --quitter reprend le journal, code $($quit.ExitCode)"
        } catch { Say "  agent : reprise du journal impossible ($($_.Exception.Message))" }
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
    # Les copies des réglages de la personne ne restent pas dans le dossier temporaire.
    Clean-Work
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
# sa fin. Rend le lanceur (qui attend le programme et rend son code de sortie) et
# le numéro du programme (pas celui du lanceur).
function Launch-AsUser([string]$Program, [string]$Arguments = '') {
    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    $out = Join-Path $Work ("lanceur-{0}.txt" -f [IO.Path]::GetFileNameWithoutExtension($Program))
    Remove-Item -Force $out -ErrorAction SilentlyContinue
    $parameters = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $Launcher, '-Programme', $Program)
    if ($Arguments) { $parameters += @('-Arguments', $Arguments) }
    # Start-Process recolle la liste avec des espaces, sans guillemets : un
    # chemin ou une ligne de commande qui en contient doit porter les siens, et
    # ses propres guillemets, les siens, précédés d'une barre oblique inverse.
    $parameters = @($parameters | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } })
    $launcher = Start-Process -FilePath 'powershell' -ArgumentList $parameters -PassThru `
        -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError "$out.err"
    # Sans cela, ExitCode peut rester vide après la fin d'un processus lancé avec -PassThru.
    $null = $launcher.Handle
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $out) {
            $line = Get-Content $out -ErrorAction SilentlyContinue | Where-Object { $_ -match '^PID (\d+)' } | Select-Object -First 1
            if ($line -match '^PID (\d+)') {
                return [pscustomobject]@{ Id = [int]$Matches[1]; Launcher = $launcher }
            }
        }
        if ($launcher.HasExited) { break }
        Start-Sleep -Milliseconds 200
    }
    $reason = if (Test-Path "$out.err") { (Get-Content "$out.err") -join ' ' } else { '' }
    Fail "le lanceur n'a pas pu démarrer $Program ($reason)"
}

# Lance un programme et rend son numéro ; l'essai le note pour le remettre en place.
function Start-AsUser([string]$Program, [string]$Arguments = '') {
    $launched = Launch-AsUser $Program $Arguments
    Register-Process $launched.Id
    return $launched.Id
}

# Lance une commande de l'interpréteur de commandes avec les droits ordinaires,
# attend sa fin, et rend son code de sortie et ce qu'elle a écrit. La commande
# passe par un petit fichier .cmd, dans le dossier temporaire du compte (le
# programme lancé y a ses droits : le dossier de travail de l'intégration
# continue n'est pas forcément ouvert en écriture à un compte filtré).
function Invoke-AsUser([string]$Name, [string]$Command) {
    New-Item -ItemType Directory -Force -Path $ProbeDir | Out-Null
    $script = Join-Path $ProbeDir "$Name.cmd"
    $output = Join-Path $ProbeDir "$Name.txt"
    Remove-Item -Force $output -ErrorAction SilentlyContinue
    $lines = @('@echo off', ('{0} > "%~dp0{1}.txt" 2>&1' -f $Command, $Name), 'exit /b %errorlevel%')
    [IO.File]::WriteAllText($script, (($lines -join "`r`n") + "`r`n"), (New-Object System.Text.UTF8Encoding $false))
    $launched = Launch-AsUser (Join-Path $env:SystemRoot 'System32\cmd.exe') ('/c ""{0}""' -f $script)
    if (-not $launched.Launcher.WaitForExit(30000)) {
        Stop-Process -Id $launched.Id -Force -ErrorAction SilentlyContinue
        Fail "la commande « $Command » ne rend pas la main en 30 s"
    }
    if ($null -eq $launched.Launcher.ExitCode) { Fail "le code de sortie de « $Command » est illisible" }
    $text = if (Test-Path $output) { (Get-Content -Raw -ErrorAction SilentlyContinue $output) } else { '' }
    return [pscustomobject]@{ ExitCode = $launched.Launcher.ExitCode; Output = ([string]$text).Trim() }
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

# ── L'essai ────────────────────────────────────────────────────────────────

$NotProved = "cette machine refuse le changement d'alimentation à un compte ordinaire : le passage en Performances élevées n'est pas prouvé ici"
$exitCode = 0
$probeOk = $false
try {
    Say "Windows $([Environment]::OSVersion.Version), agent $Agent"
    # Un essai précédent interrompu a laissé son état (et peut-être ses
    # réglages d'essai à la place de ceux de la personne) : on le remet en place
    # avant de noter quoi que ce soit, sans quoi les réglages d'essai seraient
    # pris pour les réglages d'origine.
    if (Test-Path $StateFile) {
        Say "un essai précédent a laissé son état sur le disque : remise en place d'abord"
        Restore-Everything
    }
    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    $running = @(Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue)
    if ($running.Count -gt 0) {
        Fail ("un agent tourne déjà sur cette machine (processus $(($running | ForEach-Object { $_.Id }) -join ', ')) : " +
            "ce n'est pas celui de l'essai, qui ne l'arrête ni ne le touche. Ferme-le (Shinku, page Mode jeu), puis relance l'essai.")
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

    # 1 bis. La sonde : un compte ordinaire peut-il changer le mode d'alimentation ?
    # Le même appel que celui de l'agent (PowerSetActiveScheme, par powercfg), avec
    # le même jeton. Sur un Windows Server, il est refusé à un compte sans droits
    # d'administrateur ; sur un Windows 11 de particulier, il passe.
    $probeOk = $false
    if ($hasHigh) {
        $probe = Invoke-AsUser 'sonde-alimentation' "powercfg /setactive $HighPerformance"
        $afterProbe = Active-Scheme
        $probeOk = ($probe.ExitCode -eq 0) -and ($afterProbe -eq $HighPerformance)
        Say "sonde : powercfg /setactive « Performances élevées » avec les droits ordinaires : code $($probe.ExitCode), alimentation ensuite : $(Scheme-Name $afterProbe)"
        if ($probe.Output) { Say "  sortie : $($probe.Output -replace '\s+', ' ')" }
        # Remise avec les droits de l'essai.
        & powercfg /setactive $Balanced | Out-Null
        if ((Active-Scheme) -ne $Balanced) { Fail "impossible de remettre l'alimentation en « Utilisation normale » après la sonde" }
        Done "sonde : $(if ($probeOk) { 'un compte ordinaire PEUT changer l''alimentation' } else { 'un compte ordinaire NE PEUT PAS changer l''alimentation' }) ; alimentation remise en « Utilisation normale »"
        # Pour le compte rendu : qui a le droit, et qui est ce compte ordinaire.
        Say 'sécurité de l''action SetActive (powercfg /getsecuritydescriptor ActionSetActive) :'
        try {
            $descriptor = @(& cmd.exe /c 'powercfg /getsecuritydescriptor ActionSetActive 2>&1')
            foreach ($line in $descriptor) { if ("$line".Trim()) { Say "  $line" } }
            Say "  (code $LASTEXITCODE)"
        } catch { Say "  commande impossible : $($_.Exception.Message)" }
        $groups = Invoke-AsUser 'sonde-groupes' 'whoami /groups'
        Say "groupes du jeton ordinaire (whoami /groups, code $($groups.ExitCode)) :"
        foreach ($line in ($groups.Output -split "`r?`n")) { if ($line.Trim()) { Say "  $line" } }
    } else {
        Say "sonde : sans « Performances élevées » sur la machine, il n'y a rien à sonder"
    }

    # 2. Les réglages d'essai.
    Build-Game
    if (-not (Test-Path $GameProgram)) { Fail "le jeu d'essai n'a pas pu être compilé" }
    $settings = '{{"version":1,"gameModeAuto":true,"gameModePower":true,"gameModeQuietApps":true,' +
        '"gameModeQuietAdded":["ping.exe"],"gameModeQuietRemoved":["onedrive.exe","dropbox.exe","googledrivefs.exe"],' +
        '"gameModeGames":[{0}],"gameModeNotGames":[]}}'
    New-Item -ItemType Directory -Force -Path (Split-Path $SettingsFile) | Out-Null
    [IO.File]::WriteAllText($SettingsFile, ($settings -f (Json-String $GameProgram)), (New-Object System.Text.UTF8Encoding $false))
    if (Test-Path $GameFile) { Remove-Item -Force $GameFile }
    Done "réglages d'essai écrits : mode automatique, jeu $GameProgram, ping.exe seul en veilleuse (OneDrive, Dropbox et Google Drive retirés)"

    # 3. Le ping, l'agent, puis le jeu.
    $pingId = Start-AsUser (Join-Path $env:SystemRoot 'System32\PING.EXE') '127.0.0.1 -t'
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
    if ($hasHigh -and $probeOk) {
        if ($outcome -ne 'applied') { Fail "la sonde a réussi, l'alimentation devait être appliquée ; l'agent dit : $outcome" }
        Wait-For "l'alimentation passe en « Performances élevées »" $pingId { (Active-Scheme) -eq $HighPerformance } | Out-Null
        Done "l'alimentation est en « Performances élevées »"
    } elseif ($hasHigh) {
        if ($outcome -ne 'refused') { Fail "la sonde a échoué (un compte ordinaire ne peut pas changer l'alimentation ici), l'agent devait dire « refused » ; il dit : $outcome" }
        if ((Active-Scheme) -ne $Balanced) { Fail "l'agent dit « refused », mais l'alimentation a bougé : $(Scheme-Name (Active-Scheme))" }
        Done "l'agent dit « refused » comme la sonde, et l'alimentation n'a pas bougé"
        Say $NotProved
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

    if ($hasHigh -and -not $probeOk) {
        Say 'Essai réussi, SAUF une preuve :'
        Say "  $NotProved"
        # Une annotation visible dans le résumé de l'exécution, sur GitHub.
        Write-Host "::warning title=Alimentation non prouvée::$NotProved"
    } else {
        Say 'Essai réussi.'
    }
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
