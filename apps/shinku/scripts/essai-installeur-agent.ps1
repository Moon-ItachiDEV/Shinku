# Essai des crochets de l'installeur : mise à jour, puis désinstallation, avec
# un agent du mode jeu qui tourne.
#
# Les crochets de l'installeur (installeur/agent.nsh) font sortir l'agent avant
# de copier ou de retirer son fichier. Une installation sur une machine vierge
# ne les exerce pas : il n'y a pas encore d'agent. Cet essai part donc d'un
# Shinku déjà installé (étape « Installation » de l'intégration continue) :
#
#   1. mise à jour : des réglages d'essai allument le mode automatique, la
#      valeur de démarrage `Run\Shinku` est posée (c'est `shinku.exe` qui
#      l'écrit, pas l'agent : l'essai le fait à sa place), l'agent est lancé avec
#      les droits ordinaires d'un utilisateur, puis l'installeur tout juste
#      fabriqué est relancé en silence (/S). Il doit rendre 0, laisser
#      shinku-agent.exe à sa place (le même fichier que celui qui vient d'être
#      compilé), ne laisser aucun agent en marche (l'ancien est parti, et
#      personne ne l'a relancé), et laisser la valeur `Run` ;
#   2. désinstallation : l'agent est relancé, puis uninstall.exe /S. NSIS se
#      recopie dans un dossier temporaire et rend la main tout de suite : l'essai
#      attend que le dossier d'installation disparaisse. Il exige : plus aucun
#      agent, plus de valeur `Run`, plus de dossier d'installation, et un mode
#      d'alimentation inchangé.
#
# La valeur `Run` est aussi effacée par le modèle de Tauri (DeleteRegValue) : sa
# disparition prouve l'état final, pas lequel des deux l'a effacée.
#
# Après l'essai, Shinku n'est plus installé. À lancer sur une machine jetable,
# comme celle de l'intégration continue. Les réglages, le journal et la valeur
# `Run` d'origine du compte sont remis à la fin, même en cas d'échec ;
# `-Restaurer` le refait seul (étape « toujours » de l'intégration continue).
#
#   powershell -File scripts/essai-installeur-agent.ps1 -Installeur "...\Shinku_0.1.0_x64-setup.exe"
#
# Ce fichier commence par une marque d'ordre des octets (BOM) : sans elle,
# Windows PowerShell 5 lirait ses lettres accentuées de travers.

param(
    # L'installeur NSIS fabriqué par `tauri build`.
    [string]$Installeur,
    # Le dossier où Shinku est installé.
    [string]$Dossier = (Join-Path $env:ProgramFiles 'Shinku'),
    # L'agent tel que cargo l'a compilé : le fichier installé doit lui ressembler.
    [string]$AgentCompile = (Join-Path $PSScriptRoot '..\..\..\target\release\shinku-agent.exe'),
    # Temps laissé à chaque attente, en secondes.
    [int]$Attente = 120,
    # Remet en place ce qu'un essai précédent a changé, puis s'arrête.
    [switch]$Restaurer
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }

$Temporary = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$Work = Join-Path $Temporary 'essai-installeur-agent'
$StateFile = Join-Path $Work 'etat.json'
$SettingsFile = Join-Path $env:APPDATA 'Shinku\settings.json'
$GameFile = Join-Path $env:LOCALAPPDATA 'Shinku\game.json'
$Launcher = Join-Path $PSScriptRoot 'lancer-en-utilisateur.ps1'
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$RunName = 'Shinku'
$InstalledAgent = Join-Path $Dossier 'shinku-agent.exe'

$Started = Get-Date
function Say([string]$Line) {
    $seconds = ((Get-Date) - $Started).TotalSeconds
    Write-Host ('[{0,6:N1} s] {1}' -f $seconds, $Line)
}
function Done([string]$Line) { Say "OK $Line" }
function Fail([string]$Message) { throw $Message }

function Json-String([string]$Text) { '"' + ($Text -replace '\\', '\\' -replace '"', '\"') + '"' }

# ── Ce qu'il faut remettre ─────────────────────────────────────────────────

# L'état d'origine, écrit avant le premier changement. Chaque processus lancé
# est noté avec son chemin et son heure de départ (en ticks UTC) : on ne
# l'arrêtera que si les deux correspondent encore.
$State = @{ settings = $false; game = $false; hasRun = $false; run = ''; agent = ''; procs = @() }

function Save-State {
    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    $entries = @($State.procs | ForEach-Object {
        '{{"id":{0},"path":{1},"start":"{2}"}}' -f $_.id, (Json-String $_.path), $_.start
    })
    $text = '{{"settings":{0},"game":{1},"hasRun":{2},"run":{3},"agent":{4},"procs":[{5}]}}' -f `
        $State.settings.ToString().ToLower(), $State.game.ToString().ToLower(), `
        $State.hasRun.ToString().ToLower(), (Json-String $State.run), (Json-String $State.agent), ($entries -join ',')
    [IO.File]::WriteAllText($StateFile, $text, (New-Object System.Text.UTF8Encoding $false))
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

function Get-RunValue {
    $item = Get-ItemProperty -Path $RunKey -Name $RunName -ErrorAction SilentlyContinue
    if ($null -eq $item) { return $null }
    return [string]$item.$RunName
}

function Remove-RunValue {
    Remove-ItemProperty -Path $RunKey -Name $RunName -ErrorAction SilentlyContinue
}

function Active-Scheme {
    $text = (& powercfg /getactivescheme) -join ' '
    if ($text -match '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})') {
        return $Matches[1].ToLower()
    }
    Fail "powercfg ne donne pas le mode d'alimentation actif : $text"
}

function Agents-Running { @(Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue) }

function Restore-Everything {
    if (-not (Test-Path $StateFile)) {
        Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
        return
    }
    $saved = Get-Content -Raw -Encoding UTF8 $StateFile | ConvertFrom-Json
    Say "remise en place de ce que l'essai a changé"

    # Seulement ce que l'essai a lancé : même chemin et même heure de départ.
    $killed = $false
    foreach ($entry in @($saved.procs)) {
        $own = Find-Own $entry
        if ($null -eq $own) { continue }
        Stop-Process -Id $own.Id -Force -ErrorAction SilentlyContinue
        $killed = $true
    }
    # Un agent arrêté de force n'a rien remis en place : `--quitter` reprend le
    # journal avant qu'on le remette. Il ne part que si un agent installé existe
    # encore, et si aucun autre agent ne tourne (celui d'un autre compte ou de
    # la personne n'est pas à l'essai).
    if ($killed -and $saved.agent -and (Test-Path $saved.agent) -and ((Agents-Running).Count -eq 0)) {
        try {
            $quit = Start-Process -FilePath $saved.agent -ArgumentList '--quitter' -Wait -PassThru -WindowStyle Hidden
            Say "  agent arrêté de force : --quitter reprend le journal, code $($quit.ExitCode)"
        } catch { Say "  agent : reprise du journal impossible ($($_.Exception.Message))" }
    }

    foreach ($entry in @(@($SettingsFile, 'settings', $saved.settings), @($GameFile, 'game', $saved.game))) {
        $file = $entry[0]
        $saveCopy = Join-Path $Work "$($entry[1]).json.origine"
        if ($entry[2]) {
            New-Item -ItemType Directory -Force -Path (Split-Path $file) | Out-Null
            Copy-Item -Force $saveCopy $file
        } elseif (Test-Path $file) {
            Remove-Item -Force $file
        }
    }
    if ($saved.hasRun) {
        New-Item -Path $RunKey -Force | Out-Null
        Set-ItemProperty -Path $RunKey -Name $RunName -Value ([string]$saved.run) -Type String
    } else {
        Remove-RunValue
    }
    Say '  réglages, journal et démarrage d''origine remis'
    Remove-Item -Force $StateFile -ErrorAction SilentlyContinue
    # Les copies des réglages de la personne ne restent pas dans le dossier temporaire.
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}

if ($Restaurer) {
    Restore-Everything
    exit 0
}

if (-not $Installeur) {
    [Console]::Error.WriteLine('Usage : essai-installeur-agent.ps1 -Installeur <...-setup.exe> [-Dossier <dossier d''installation>]')
    exit 2
}
if (-not (Test-Path $Installeur)) {
    [Console]::Error.WriteLine("Installeur introuvable : $Installeur")
    exit 2
}

# ── Les outils de l'essai ──────────────────────────────────────────────────

# Lance un programme avec les droits ordinaires d'un utilisateur, sans attendre
# sa fin. Rend le numéro du programme (pas celui du lanceur).
function Start-AsUser([string]$Program, [string]$Arguments = '') {
    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    $out = Join-Path $Work ("lanceur-{0}.txt" -f [IO.Path]::GetFileNameWithoutExtension($Program))
    Remove-Item -Force $out -ErrorAction SilentlyContinue
    $parameters = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $Launcher, '-Programme', $Program)
    if ($Arguments) { $parameters += @('-Arguments', $Arguments) }
    # Start-Process recolle la liste avec des espaces, sans guillemets : un
    # chemin ou une ligne de commande qui en contient doit porter les siens.
    $parameters = @($parameters | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } })
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

# Lance l'agent installé avec les droits ordinaires et vérifie qu'il tourne.
function Start-Agent {
    $id = Start-AsUser $InstalledAgent
    Start-Sleep -Seconds 3
    if ($null -eq (Get-Process -Id $id -ErrorAction SilentlyContinue)) {
        Fail "l'agent s'est arrêté aussitôt (un agent lancé en administrateur refuse de tourner : code 3)"
    }
    return $id
}

# Attend qu'une condition soit vraie, ou échoue avec ce qu'on attendait.
function Wait-Until([string]$What, [scriptblock]$Test, [int]$Seconds = $Attente) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ($true) {
        if (& $Test) { return }
        if ((Get-Date) -ge $deadline) { Fail "$What : toujours pas après $Seconds s" }
        Start-Sleep -Milliseconds 500
    }
}

# Lance un programme, attend sa fin (au plus $Seconds s) et rend son code de sortie.
function Run-AndWait([string]$Program, [string]$Arguments, [int]$Seconds) {
    $process = Start-Process -FilePath $Program -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    $null = $process.Handle
    if (-not $process.WaitForExit($Seconds * 1000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        Fail "$Program $Arguments ne se termine pas en $Seconds s"
    }
    return $process.ExitCode
}

function Describe-Dir([string]$Path) {
    if (-not (Test-Path $Path)) { return 'absent' }
    $names = @(Get-ChildItem -Force -Recurse -ErrorAction SilentlyContinue $Path | ForEach-Object { $_.FullName.Substring($Path.Length) })
    return "présent, contient : $($names -join ', ')"
}

# ── L'essai ────────────────────────────────────────────────────────────────

$exitCode = 0
try {
    Say "Windows $([Environment]::OSVersion.Version), installeur $Installeur, dossier $Dossier"
    # Un essai précédent interrompu a laissé son état (et ses réglages d'essai
    # à la place de ceux de la personne) : on le remet en place d'abord.
    if (Test-Path $StateFile) {
        Say "un essai précédent a laissé son état sur le disque : remise en place d'abord"
        Restore-Everything
    }
    New-Item -ItemType Directory -Force -Path $Work | Out-Null

    if (-not (Test-Path $InstalledAgent)) {
        Fail "Shinku n'est pas installé dans $Dossier (pas de shinku-agent.exe) : cet essai part d'une installation existante"
    }
    if (-not (Test-Path $AgentCompile)) {
        Fail "l'agent compilé est introuvable ($AgentCompile) : on ne peut pas dire si l'installeur dépose le bon fichier"
    }
    $running = Agents-Running
    if ($running.Count -gt 0) {
        Fail ("un agent tourne déjà sur cette machine (processus $(($running | ForEach-Object { $_.Id }) -join ', ')) : " +
            "ce n'est pas celui de l'essai, qui ne l'arrête ni ne le touche. Ferme-le (Shinku, page Mode jeu), puis relance l'essai.")
    }

    # État d'origine, noté avant toute modification.
    $State.agent = $InstalledAgent
    foreach ($entry in @(@($SettingsFile, 'settings'), @($GameFile, 'game'))) {
        $has = Test-Path $entry[0]
        if ($has) { Copy-Item -Force $entry[0] (Join-Path $Work "$($entry[1]).json.origine") }
        $State[$entry[1]] = $has
    }
    $originalRun = Get-RunValue
    $State.hasRun = $null -ne $originalRun
    $State.run = if ($null -ne $originalRun) { $originalRun } else { '' }
    Save-State
    $scheme = Active-Scheme
    Say "alimentation au départ : $scheme (elle ne doit pas bouger)"

    # Les réglages d'essai : mode automatique allumé, sans jeu déclaré, et les
    # trois apps cochées d'office retirées (rien n'est mis en veilleuse).
    $settings = '{"version":1,"gameModeAuto":true,"gameModePower":true,"gameModeQuietApps":true,' +
        '"gameModeQuietAdded":[],"gameModeQuietRemoved":["onedrive.exe","dropbox.exe","googledrivefs.exe"],' +
        '"gameModeGames":[],"gameModeNotGames":[]}'
    New-Item -ItemType Directory -Force -Path (Split-Path $SettingsFile) | Out-Null
    [IO.File]::WriteAllText($SettingsFile, $settings, (New-Object System.Text.UTF8Encoding $false))
    if (Test-Path $GameFile) { Remove-Item -Force $GameFile }
    Done 'réglages d''essai écrits : mode jeu automatique allumé'

    # La valeur de démarrage, telle que shinku.exe l'écrit quand le mode est allumé.
    $runValue = '"{0}" --demarrage' -f $InstalledAgent
    New-Item -Path $RunKey -Force | Out-Null
    Set-ItemProperty -Path $RunKey -Name $RunName -Value $runValue -Type String
    if ((Get-RunValue) -ne $runValue) { Fail "la valeur $RunKey\$RunName n'a pas pu être écrite" }
    Done "valeur de démarrage posée : $runValue"

    # ── 1. Mise à jour ─────────────────────────────────────────────────────
    Say '1. mise à jour : l''installeur est relancé pendant que l''agent tourne'
    $oldId = Start-Agent
    $oldEntry = @($State.procs)[-1]
    Done "l'agent tourne (processus $oldId)"

    $code = Run-AndWait $Installeur '/S' 300
    if ($code -ne 0) { Fail "l'installeur rend le code $code au lieu de 0" }
    Done 'l''installeur rend le code 0'
    if (-not (Test-Path $InstalledAgent)) { Fail "après la mise à jour, $InstalledAgent n'existe plus" }
    $built = (Get-FileHash -Algorithm SHA256 $AgentCompile).Hash
    $installed = (Get-FileHash -Algorithm SHA256 $InstalledAgent).Hash
    if ($built -ne $installed) { Fail "le shinku-agent.exe installé n'est pas celui qui vient d'être compilé (SHA-256 $installed au lieu de $built)" }
    Done 'shinku-agent.exe est à sa place, identique à l''agent compilé'
    if ($null -ne (Find-Own $oldEntry)) { Fail "l'agent de l'ancienne exécution (processus $oldId) tourne encore après la mise à jour" }
    $left = Agents-Running
    if ($left.Count -gt 0) { Fail "après la mise à jour, $($left.Count) agent(s) tournent (processus $(($left | ForEach-Object { $_.Id }) -join ', ')) : plus rien ne devait en laisser" }
    Done 'plus aucun agent en marche : l''ancien est parti, et rien ne l''a relancé'
    if ((Get-RunValue) -ne $runValue) { Fail "après la mise à jour, la valeur de démarrage a changé : « $(Get-RunValue) »" }
    Done 'la valeur de démarrage est toujours là'

    # ── 2. Désinstallation ─────────────────────────────────────────────────
    Say '2. désinstallation : uninstall.exe /S pendant que l''agent tourne'
    $uninstaller = Join-Path $Dossier 'uninstall.exe'
    if (-not (Test-Path $uninstaller)) { Fail "pas de désinstalleur : $uninstaller" }
    $agentId = Start-Agent
    $agentEntry = @($State.procs)[-1]
    Done "l'agent tourne (processus $agentId)"
    $schemeBefore = Active-Scheme

    # NSIS se recopie dans un dossier temporaire et rend la main aussitôt : le
    # travail continue dans le processus recopié. On attend la disparition du
    # dossier d'installation, puis celle du processus recopié (Un_*.exe).
    $stub = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru -WindowStyle Hidden
    Say "désinstalleur lancé (processus $($stub.Id))"
    try {
        Wait-Until "le dossier $Dossier disparaît" { -not (Test-Path $Dossier) }
    } catch {
        Fail "$($_.Exception.Message) ; le dossier est $(Describe-Dir $Dossier)"
    }
    Done 'le dossier d''installation a disparu'
    Wait-Until 'le désinstalleur recopié a fini' { @(Get-Process -Name 'Un_*' -ErrorAction SilentlyContinue).Count -eq 0 } 60
    if ($null -ne (Find-Own $agentEntry)) { Fail "l'agent (processus $agentId) tourne encore après la désinstallation" }
    $left = Agents-Running
    if ($left.Count -gt 0) { Fail "après la désinstallation, $($left.Count) agent(s) tournent encore" }
    Done 'plus aucun agent en marche'
    if ($null -ne (Get-RunValue)) { Fail "après la désinstallation, la valeur de démarrage existe encore : « $(Get-RunValue) »" }
    Done 'la valeur de démarrage est effacée'
    if ((Active-Scheme) -ne $schemeBefore) { Fail "l'alimentation a changé : $schemeBefore avant, $(Active-Scheme) après" }
    Done 'l''alimentation n''a pas bougé'

    Say 'Essai réussi. Shinku n''est plus installé sur cette machine.'
} catch {
    $exitCode = 1
    Say "ECHEC : $($_.Exception.Message)"
    Say 'ce que la machine montrait :'
    try {
        Say "  dossier d'installation : $(Describe-Dir $Dossier)"
        Say "  valeur Run : $(if ($null -eq (Get-RunValue)) { 'absente' } else { Get-RunValue })"
        Say "  agents en marche : $((Agents-Running).Count)"
    } catch { Say "  (diagnostic impossible : $($_.Exception.Message))" }
} finally {
    try { Restore-Everything } catch { Say "remise en place incomplète : $($_.Exception.Message)"; $exitCode = 1 }
}
exit $exitCode
