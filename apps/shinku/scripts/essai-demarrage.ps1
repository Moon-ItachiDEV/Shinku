# Prépare, puis remet en place, ce que l'essai de la fenêtre lit dans la page
# Démarrage : cinq entrées d'essai, une par place que Shinku lit.
#
#   -Preparer : nettoie un essai interrompu, affiche ce que la machine dit de
#               ses droits et de ses approbations (pour le compte rendu et pour
#               préparer le lot 2), puis crée les cinq entrées :
#     HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Essai Shinku Compte
#         une commande portée par powershell.exe (ligne « Lancée par »)
#     HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Essai Shinku Cible
#         notepad.exe, une cible trouvée qui a une description
#     HKCU\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Run\Essai Shinku 32
#     un raccourci « Essai Shinku Dossier.lnk » dans le dossier Démarrage du compte
#     HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\Essai Shinku PC
#         (les machines de l'intégration continue tournent en administrateur)
#   -Nettoyer : retire ces cinq entrées et leurs approbations, par leurs noms
#               exacts, et rien d'autre. Ne plante pas si elles n'existent pas.
#
# Les entrées appartiennent au compte de la machine d'essai : Shinku y tourne
# avec un jeton filtré du même compte, HKCU est donc le même.
#
#   powershell -File scripts/essai-demarrage.ps1 -Preparer
#   powershell -File scripts/essai-demarrage.ps1 -Nettoyer
#
# À lancer sur une machine d'essai. Ce fichier commence par une marque d'ordre
# des octets (BOM) : sans elle, Windows PowerShell 5 lirait ses lettres
# accentuées de travers.

param(
    [switch]$Preparer,
    [switch]$Nettoyer
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }

$Started = Get-Date
function Say([string]$Line) {
    $seconds = ((Get-Date) - $Started).TotalSeconds
    Write-Host ('[{0,6:N1} s] {1}' -f $seconds, $Line)
}
function Done([string]$Line) { Say "OK $Line" }
function Fail([string]$Message) { throw $Message }

# ── Ce que l'essai crée ────────────────────────────────────────────────────

$Hosted = '"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -Command exit'
$Notepad = '"C:\Windows\System32\notepad.exe"'

$UserRun = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$UserRun32 = 'HKCU:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Run'
$MachineRun = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run'
$UserApproved = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved'
$MachineApproved = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved'
$Policies = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'

$NameAccount = 'Essai Shinku Compte'
$NameTarget = 'Essai Shinku Cible'
$Name32 = 'Essai Shinku 32'
$NameFolder = 'Essai Shinku Dossier.lnk'
$NameMachine = 'Essai Shinku PC'

function Startup-Folder { return [Environment]::GetFolderPath('Startup') }

# ── Nettoyer ───────────────────────────────────────────────────────────────

# Retire une valeur de registre si elle existe ; jamais d'erreur.
function Remove-Value([string]$Key, [string]$Name) {
    try {
        if (Test-Path -LiteralPath $Key) {
            $item = Get-ItemProperty -LiteralPath $Key -ErrorAction SilentlyContinue
            if ($null -ne $item -and $null -ne $item.PSObject.Properties[$Name]) {
                Remove-ItemProperty -LiteralPath $Key -Name $Name -ErrorAction Stop
                Say "  retiré : $Key\$Name"
            }
        }
    } catch {
        Say "  impossible de retirer $Key\$Name : $($_.Exception.Message)"
    }
}

function Clean-Up {
    Say "retrait des cinq entrées d'essai et de leurs approbations"
    Remove-Value $UserRun $NameAccount
    Remove-Value $UserRun $NameTarget
    Remove-Value $UserRun32 $Name32
    Remove-Value $MachineRun $NameMachine
    # Les approbations : une valeur par nom exact, dans la sous-clé de sa place.
    Remove-Value "$UserApproved\Run" $NameAccount
    Remove-Value "$UserApproved\Run" $NameTarget
    Remove-Value "$UserApproved\Run32" $Name32
    Remove-Value "$UserApproved\StartupFolder" $NameFolder
    Remove-Value "$MachineApproved\Run" $NameMachine
    try {
        $shortcut = Join-Path (Startup-Folder) $NameFolder
        if (Test-Path -LiteralPath $shortcut) {
            Remove-Item -LiteralPath $shortcut -Force -ErrorAction Stop
            Say "  retiré : $shortcut"
        }
    } catch {
        Say "  impossible de retirer le raccourci : $($_.Exception.Message)"
    }
}

# ── Préparer ───────────────────────────────────────────────────────────────

# Ce que la machine dit, pour le compte rendu : sans effet.
function Show-Machine {
    Say 'whoami /groups :'
    foreach ($line in @(& whoami.exe /groups)) { Say "  $line" }
    Say 'UAC (HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System) :'
    $item = $null
    if (Test-Path -LiteralPath $Policies) { $item = Get-ItemProperty -LiteralPath $Policies -ErrorAction SilentlyContinue }
    foreach ($name in @('EnableLUA', 'ConsentPromptBehaviorAdmin', 'ConsentPromptBehaviorUser', 'PromptOnSecureDesktop')) {
        $value = if ($null -ne $item -and $null -ne $item.PSObject.Properties[$name]) { [string]$item.$name } else { '(absente)' }
        Say "  $name = $value"
    }
    foreach ($key in @(
            'HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved',
            'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved')) {
        Say "reg query $key /s /reg:64 :"
        # Par cmd : dans Windows PowerShell, rediriger ici la sortie d'erreur d'un
        # programme transforme sa première ligne en erreur qui arrête le script.
        $shown = @(& cmd.exe /c "reg query $key /s /reg:64 2>nul")
        if ($shown.Count -eq 0) { Say '  (absente ou vide)' }
        foreach ($line in $shown) { Say "  $line" }
    }
}

function Set-Run([string]$Key, [string]$Name, [string]$Command) {
    if (-not (Test-Path -LiteralPath $Key)) { New-Item -Path $Key -Force | Out-Null }
    New-ItemProperty -LiteralPath $Key -Name $Name -Value $Command -PropertyType String -Force | Out-Null
    $read = (Get-ItemProperty -LiteralPath $Key -Name $Name).$Name
    if ($read -ne $Command) { Fail "$Key\$Name vaut « $read » au lieu de « $Command »" }
    Done "$Key\$Name = $Command"
}

function Prepare {
    Say "nettoyage d'un essai interrompu"
    Clean-Up
    Show-Machine

    Say "création des cinq entrées d'essai"
    Set-Run $UserRun $NameAccount $Hosted
    Set-Run $UserRun $NameTarget $Notepad
    Set-Run $UserRun32 $Name32 $Hosted
    Set-Run $MachineRun $NameMachine $Hosted

    $folder = Startup-Folder
    if (-not (Test-Path -LiteralPath $folder)) { New-Item -ItemType Directory -Force -Path $folder | Out-Null }
    $path = Join-Path $folder $NameFolder
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($path)
    $link.TargetPath = 'C:\Windows\System32\notepad.exe'
    $link.Save()
    if (-not (Test-Path -LiteralPath $path)) { Fail "le raccourci $path n'a pas été créé" }
    Done "raccourci $path"
}

# ── Programme ──────────────────────────────────────────────────────────────

if ([bool]$Preparer -eq [bool]$Nettoyer) {
    Write-Host 'Usage : essai-demarrage.ps1 -Preparer | -Nettoyer'
    exit 2
}

$exitCode = 0
try {
    if ($Preparer) {
        Prepare
        Say 'Préparation faite.'
    } else {
        Clean-Up
        Say 'Nettoyage fait.'
    }
} catch {
    $exitCode = 1
    Say "ECHEC : $($_.Exception.Message)"
    if ($_.InvocationInfo -and $_.InvocationInfo.PositionMessage) {
        foreach ($where in ("$($_.InvocationInfo.PositionMessage)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
    if ($_.ScriptStackTrace) {
        foreach ($where in ("$($_.ScriptStackTrace)" -split "`r?`n")) { if ("$where".Trim()) { Say "  $where" } }
    }
    # Une préparation à moitié faite ne reste pas sur la machine.
    if ($Preparer) {
        try { Clean-Up } catch { Say "nettoyage incomplet : $($_.Exception.Message)" }
    }
}
exit $exitCode
