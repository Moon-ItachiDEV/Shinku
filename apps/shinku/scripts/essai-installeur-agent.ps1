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
#   2. désinstallation : l'agent est relancé, puis uninstall.exe /S, le chemin de
#      « Paramètres > Applications ». NSIS se recopie dans %TEMP%\~nsu<n>.tmp\Un.exe
#      et rend la main tout de suite : l'essai attend ce processus, puis exige,
#      dans l'ordre où le désinstalleur finit : le dossier d'installation a
#      disparu, la clé de désinstallation aussi, la valeur `Run` est effacée, plus
#      aucun agent. Le code de sortie n'est pas lisible sur ce chemin : un crochet
#      qui abandonne se voit à la clé ou au dossier restés là.
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
    # La liste des licences telle que scripts/licences-tiers.mjs l'a fabriquée : le fichier installé doit lui ressembler.
    [string]$LicencesCompilees = (Join-Path $PSScriptRoot '..\src-tauri\licences\LICENCES-TIERS.txt'),
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
$UninstallKey = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Shinku'
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

# Les désinstalleurs recopiés par NSIS : `uninstall.exe` se copie dans
# `%TEMP%\~nsu<n>.tmp\Un.exe` (NSIS 3.09 et suivants, dont la version que Tauri
# télécharge) et rend la main aussitôt ; le travail continue dans la copie.
function Copied-Uninstallers {
    @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -like '*\~nsu*.tmp\Un*.exe' })
}

# Attend, au plus $Seconds s, qu'aucun désinstalleur recopié ne vive plus. Ne
# lève rien : la remise en place ne doit jamais courir après un désinstalleur
# qui n'a pas fini (il effacerait après coup une valeur `Run` qu'elle vient de remettre).
function Wait-Uninstallers([int]$Seconds = 60) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Copied-Uninstallers).Count -gt 0) {
        if ((Get-Date) -ge $deadline) { Say "  un désinstalleur recopié vit encore après $Seconds s : la remise en place continue"; return }
        Start-Sleep -Milliseconds 500
    }
}

function Restore-Everything {
    Wait-Uninstallers
    if (-not (Test-Path $StateFile)) {
        Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
        return
    }
    $saved = Get-Content -Raw -Encoding UTF8 $StateFile | ConvertFrom-Json
    Say "remise en place de ce que l'essai a changé"

    # Les processus que l'essai avait lancés et qui tournent encore (même
    # numéro, même chemin, même heure de départ). Un autre agent, celui de la
    # personne par exemple, n'est pas à l'essai : `--quitter` l'arrêterait aussi,
    # alors on ne l'envoie pas tant qu'il tourne.
    $ownIds = @(@($saved.procs) | ForEach-Object { $found = Find-Own $_; if ($null -ne $found) { $found.Id } })
    $foreign = @(Agents-Running | Where-Object { $ownIds -notcontains $_.Id })
    $hasAgent = $saved.agent -and (Test-Path $saved.agent)
    if ($foreign.Count -gt 0) {
        Say "  un agent qui n'est pas celui de l'essai tourne (processus $(($foreign | ForEach-Object { $_.Id }) -join ', ')) : il n'est pas touché, et --quitter n'est pas envoyé"
        $hasAgent = $false
    }
    # L'agent d'abord : il remet lui-même ce qu'il a changé.
    if ($hasAgent) {
        try {
            $quit = Start-Process -FilePath $saved.agent -ArgumentList '--quitter' -Wait -PassThru -WindowStyle Hidden
            Say "  agent : --quitter rend le code $($quit.ExitCode)"
        } catch { Say "  agent : --quitter impossible ($($_.Exception.Message))" }
    }
    # Puis seulement ce que l'essai a lancé : même chemin et même heure de départ.
    $killedAgent = $false
    foreach ($entry in @($saved.procs)) {
        $own = Find-Own $entry
        if ($null -eq $own) { continue }
        Stop-Process -Id $own.Id -Force -ErrorAction SilentlyContinue
        if ($saved.agent -and ([string]$entry.path -ieq [string]$saved.agent)) { $killedAgent = $true }
    }
    # Un agent arrêté de force n'a rien remis en place : `--quitter` reprend le
    # journal avant qu'on le remette (plus aucun agent ne répond).
    if ($killedAgent -and $hasAgent) {
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
        if (-not (Test-Path -LiteralPath $RunKey)) { New-Item -Path $RunKey -Force | Out-Null }
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
    if (-not (Test-Path $LicencesCompilees)) {
        Fail "la liste des licences fabriquée est introuvable ($LicencesCompilees) : on ne peut pas dire si l'installeur dépose le bon fichier"
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
    Say "alimentation au départ : $scheme (simple garde-fou : elle ne doit pas bouger)"

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
    if (-not (Test-Path -LiteralPath $RunKey)) { New-Item -Path $RunKey -Force | Out-Null }
    Set-ItemProperty -Path $RunKey -Name $RunName -Value $runValue -Type String
    if ((Get-RunValue) -ne $runValue) { Fail "la valeur $RunKey\$RunName n'a pas pu être écrite" }
    Done "valeur de démarrage posée : $runValue"

    # ── 1. Mise à jour ─────────────────────────────────────────────────────
    Say '1. mise à jour : l''installeur est relancé pendant que l''agent tourne'
    # Un octet de plus à la fin du fichier installé (un exécutable en garde un
    # sans broncher) : sur la CI, l'installation vient du même installeur, et le
    # fichier a déjà le hachage de l'agent compilé. Après la mise à jour, le
    # hachage prouve ainsi que l'installeur l'a vraiment remplacé (en silencieux,
    # un `File` qui échoue est ignoré et l'installeur rend quand même 0).
    $stream = [IO.File]::Open($InstalledAgent, [IO.FileMode]::Append)
    try { $stream.WriteByte(0) } finally { $stream.Close() }
    if ((Get-FileHash -Algorithm SHA256 $InstalledAgent).Hash -eq (Get-FileHash -Algorithm SHA256 $AgentCompile).Hash) {
        Fail 'le fichier installé n''a pas pu être marqué : le hachage ne prouverait rien'
    }
    Done 'l''agent installé est marqué (un octet de plus) : son hachage diffère de l''agent compilé'

    $oldId = Start-Agent
    $oldEntry = @($State.procs)[-1]
    Done "l'agent tourne (processus $oldId)"
    # Shinku ouvert aussi, comme sur un PC : l'installeur doit le fermer (Tauri le
    # fait après le premier crochet), et rien ne doit relancer l'agent ensuite.
    # Le cas où la page Mode jeu relance l'agent entre les deux crochets n'est pas
    # exercé : la fenêtre n'est pas pilotée ici.
    $shinku = Join-Path $Dossier 'shinku.exe'
    $windowId = Start-AsUser $shinku
    $windowEntry = @($State.procs)[-1]
    Start-Sleep -Seconds 8
    if ($null -eq (Find-Own $windowEntry)) { Fail "shinku.exe (processus $windowId) s'est arrêté seul avant la mise à jour" }
    Done "shinku.exe est ouvert (processus $windowId)"

    $code = Run-AndWait $Installeur '/S' 300
    if ($code -ne 0) { Fail "l'installeur rend le code $code au lieu de 0" }
    Done 'l''installeur rend le code 0'
    if (-not (Test-Path $InstalledAgent)) { Fail "après la mise à jour, $InstalledAgent n'existe plus" }
    $built = (Get-FileHash -Algorithm SHA256 $AgentCompile).Hash
    $installed = (Get-FileHash -Algorithm SHA256 $InstalledAgent).Hash
    if ($built -ne $installed) { Fail "l'installeur n'a pas remplacé shinku-agent.exe : il n'est pas identique à l'agent compilé (SHA-256 $installed au lieu de $built)" }
    Done 'shinku-agent.exe a été remplacé : identique à l''agent compilé alors qu''il avait été marqué'
    # L'assistant élevé du Démarrage (installeur/helper.nsh) : redéposé à côté de shinku.exe.
    $installedHelper = Join-Path $Dossier 'shinku-helper.exe'
    if (-not (Test-Path $installedHelper)) { Fail "après la mise à jour, $installedHelper n'existe pas : l'installeur ne dépose plus l'assistant" }
    Done 'shinku-helper.exe est à côté de shinku.exe après la mise à jour'
    # La liste des licences (installeur/licences.nsh) : redéposée, non vide, identique à celle que le script a fabriquée.
    $installedLicences = Join-Path $Dossier 'LICENCES-TIERS.txt'
    if (-not (Test-Path $installedLicences)) { Fail "après la mise à jour, $installedLicences n'existe pas : l'installeur ne dépose plus la liste des licences" }
    if ((Get-Item $installedLicences).Length -eq 0) { Fail "après la mise à jour, $installedLicences est vide" }
    if ((Get-FileHash -Algorithm SHA256 $installedLicences).Hash -ne (Get-FileHash -Algorithm SHA256 $LicencesCompilees).Hash) {
        Fail "après la mise à jour, $installedLicences n'est pas identique à la liste fabriquée ($LicencesCompilees)"
    }
    Done 'LICENCES-TIERS.txt est à côté de shinku.exe après la mise à jour, non vide, identique à la liste fabriquée'
    if ($null -ne (Find-Own $windowEntry)) { Fail "shinku.exe (processus $windowId) tourne encore après la mise à jour : l'installeur devait le fermer" }
    Done 'shinku.exe a été fermé par l''installeur'
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

    # Le chemin réel, celui de « Paramètres > Applications » : uninstall.exe /S se
    # recopie dans %TEMP%\~nsu<n>.tmp\Un.exe et rend la main aussitôt. Le code de
    # sortie du travail n'est donc pas lisible ici ; un crochet qui abandonne
    # (Abort) se voit à ce qui reste : le dossier d'installation ou la clé de
    # désinstallation. On attend d'abord le processus recopié, puis seulement on
    # vérifie, dans l'ordre où le désinstalleur finit : dossier, clé, valeur `Run`,
    # agent.
    $stub = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru -WindowStyle Hidden
    Say "désinstalleur lancé (processus $($stub.Id))"
    $seen = $false
    Wait-Until 'le désinstalleur recopié a fini' {
        if ((Copied-Uninstallers).Count -gt 0) { $script:seen = $true; return $false }
        return ($script:seen -or -not (Test-Path $Dossier))
    } $Attente
    Done 'le désinstalleur recopié (~nsu*.tmp\Un.exe) a fini'
    if (Test-Path $Dossier) { Fail "après la désinstallation, le dossier $Dossier existe encore : $(Describe-Dir $Dossier) ; un crochet a peut-être abandonné (le code de sortie n'est pas lisible sur ce chemin)" }
    Done 'le dossier d''installation a disparu'
    # Le dossier part avant la clé (raccourcis entre les deux), et un processus
    # dont Windows ne donne pas encore le chemin échappe à Copied-Uninstallers :
    # l'attente ci-dessus peut alors rendre la main dès que le dossier a disparu,
    # la copie encore en marche (vu une fois sur la machine d'intégration). On
    # laisse donc à la clé le temps de partir : un crochet qui abandonne la
    # laisse pour de bon, et l'essai échoue quand même.
    $deadline = (Get-Date).AddSeconds(30)
    while ((Test-Path $UninstallKey) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
    Wait-Uninstallers 30
    if (Test-Path $UninstallKey) { Fail "après la désinstallation, la clé $UninstallKey existe encore : le désinstalleur n'est pas allé au bout" }
    Done 'la clé de désinstallation a disparu'
    if ($null -ne (Get-RunValue)) { Fail "après la désinstallation, la valeur de démarrage existe encore : « $(Get-RunValue) »" }
    Done 'la valeur de démarrage est effacée'
    if ($null -ne (Find-Own $agentEntry)) { Fail "l'agent (processus $agentId) tourne encore après la désinstallation" }
    $left = Agents-Running
    if ($left.Count -gt 0) { Fail "après la désinstallation, $($left.Count) agent(s) tournent encore" }
    Done 'plus aucun agent en marche'
    # Garde-fou, pas une preuve : aucune partie n'a lieu, l'alimentation ne pouvait pas bouger.
    if ((Active-Scheme) -ne $schemeBefore) { Fail "l'alimentation a changé : $schemeBefore avant, $(Active-Scheme) après" }
    Done 'garde-fou : l''alimentation n''a pas bougé (aucune partie n''a eu lieu : ce n''est pas une preuve)'

    Say 'Essai réussi. Shinku n''est plus installé sur cette machine.'
} catch {
    $exitCode = 1
    Say "ECHEC : $($_.Exception.Message)"
    if ($_.InvocationInfo -and $_.InvocationInfo.PositionMessage) {
        foreach ($where in ("$($_.InvocationInfo.PositionMessage)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
    if ($_.ScriptStackTrace) {
        foreach ($where in ("$($_.ScriptStackTrace)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
    Say 'ce que la machine montrait :'
    try {
        Say "  dossier d'installation : $(Describe-Dir $Dossier)"
        Say "  valeur Run : $(if ($null -eq (Get-RunValue)) { 'absente' } else { Get-RunValue })"
        Say "  agents en marche : $((Agents-Running).Count)"
        # De quoi dire si le désinstalleur a fini, s'est arrêté en route, ou tourne encore.
        $copies = @(Copied-Uninstallers)
        Say "  désinstalleurs recopiés en marche : $($copies.Count) $(($copies | ForEach-Object { "$($_.Id) $($_.Path)" }) -join ' ; ')"
        $autres = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '(?i)setup|uninstall|^Un_|^Au_' })
        Say "  installeurs ou désinstalleurs en marche : $(($autres | ForEach-Object { "$($_.Id) $($_.Name) $($_.Path)" }) -join ' ; ')"
        foreach ($cle in $UninstallKey, 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Shinku', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Shinku') {
            if (Test-Path $cle) {
                $valeurs = Get-ItemProperty -Path $cle -ErrorAction SilentlyContinue
                Say "  clé $cle : présente ($(($valeurs.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ' ; '))"
            } else { Say "  clé $cle : absente" }
        }
        foreach ($raccourci in (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Shinku.lnk'), (Join-Path $env:PUBLIC 'Desktop\Shinku.lnk')) {
            Say "  raccourci $raccourci : $(if (Test-Path $raccourci) { 'présent' } else { 'absent' })"
        }
        Start-Sleep -Seconds 5
        Say "  cinq secondes plus tard : clé de désinstallation $(if (Test-Path $UninstallKey) { 'présente' } else { 'absente' }), désinstalleurs recopiés en marche : $((Copied-Uninstallers).Count)"
    } catch { Say "  (diagnostic impossible : $($_.Exception.Message))" }
} finally {
    try { Restore-Everything } catch { Say "remise en place incomplète : $($_.Exception.Message)"; $exitCode = 1 }
}
exit $exitCode
