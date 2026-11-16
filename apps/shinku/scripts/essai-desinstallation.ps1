# Prépare, puis remet en place, ce que l'essai de la fenêtre lit dans la page
# Désinstallation : des apps d'essai inscrites dans le compte, avec leurs
# dossiers, leurs réglages et leurs raccourcis.
#
#   -Preparer : nettoie un essai interrompu, puis crée, sous
#               HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall :
#     EssaiShinkuApp        « Essai Shinku App », dans
#         %LOCALAPPDATA%\Programs\Essai Shinku App : un app.exe factice, un
#         retirer.cmd (désinstalleur : il supprime app.exe puis son inscription,
#         et laisse le dossier et reste.txt, comme le font Inno Setup et NSIS),
#         et reste.txt. Restes plantés : %APPDATA%\Essai Shinku App\reglages.json
#         et un raccourci du menu Démarrer du compte vers app.exe. Ce sont les
#         trois restes que la fenêtre doit proposer, sans en cocher un seul.
#     EssaiShinkuVoisine    « Essai Shinku Voisine », jamais désinstallée : son
#         dossier, ses réglages (%APPDATA%\Essai Shinku Voisine) et son
#         raccourci ne doivent jamais être proposés ni touchés.
#     EssaiShinkuSansDossier « Essai Shinku Sans Dossier » : aucune
#         InstallLocation, son désinstalleur vit dans
#         %LOCALAPPDATA%\Essai Shinku Sans Dossier. Jamais désinstallée.
#     EssaiShinkuJumelle1 et EssaiShinkuJumelle2 : « Essai Shinku Jumelle 1.0 » et
#         « Essai Shinku Jumelle 2.0 », chacune son dossier, plus des réglages
#         communs %APPDATA%\Essai Shinku Jumelle. Désinstaller la 1.0 ne doit
#         rien proposer tant que la 2.0 est là (même famille).
#     Et un leurre : %APPDATA%\Essai Shinku (le nom de l'éditeur seul), jamais
#         proposé.
#   -Nettoyer : retire ces inscriptions (jamais la clé Uninstall elle-même), ces dossiers et ces raccourcis, par
#               leurs noms exacts, et rien d'autre. Ne plante pas s'ils
#               n'existent pas.
#
# Tout appartient au compte de la machine d'essai : Shinku y tourne avec un
# jeton filtré du même compte, HKCU et les dossiers sont donc les mêmes.
#
#   powershell -File scripts/essai-desinstallation.ps1 -Preparer
#   powershell -File scripts/essai-desinstallation.ps1 -Nettoyer
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

$UninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'

# Les noms de variables ne tiennent pas compte de la casse : chacun est distinct
# des autres même en minuscules.
$LocalRoot = $env:LOCALAPPDATA
$RoamingRoot = $env:APPDATA
if ([string]::IsNullOrWhiteSpace($LocalRoot) -or [string]::IsNullOrWhiteSpace($RoamingRoot)) {
    Write-Host 'LOCALAPPDATA et APPDATA doivent être définis.'
    exit 1
}
$ProgramsRoot = Join-Path $LocalRoot 'Programs'
$StartPrograms = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs'

# Chaque app d'essai : clé, nom, dossier du programme, dossier des réglages,
# nom du raccourci (ou vide). `Location` vide : aucune InstallLocation.
$Apps = @(
    @{ Key = 'EssaiShinkuApp'; Name = 'Essai Shinku App'; Version = '1.0.0'
       Folder = (Join-Path $ProgramsRoot 'Essai Shinku App'); Location = $true
       Data = (Join-Path $RoamingRoot 'Essai Shinku App'); Link = 'Essai Shinku App.lnk' },
    @{ Key = 'EssaiShinkuVoisine'; Name = 'Essai Shinku Voisine'; Version = '2.3.1'
       Folder = (Join-Path $ProgramsRoot 'Essai Shinku Voisine'); Location = $true
       Data = (Join-Path $RoamingRoot 'Essai Shinku Voisine'); Link = 'Essai Shinku Voisine.lnk' },
    @{ Key = 'EssaiShinkuSansDossier'; Name = 'Essai Shinku Sans Dossier'; Version = '0.9'
       Folder = (Join-Path $LocalRoot 'Essai Shinku Sans Dossier'); Location = $false
       Data = $null; Link = $null },
    @{ Key = 'EssaiShinkuJumelle1'; Name = 'Essai Shinku Jumelle 1.0'; Version = '1.0'
       Folder = (Join-Path $ProgramsRoot 'Essai Shinku Jumelle 1.0'); Location = $true
       Data = $null; Link = $null },
    @{ Key = 'EssaiShinkuJumelle2'; Name = 'Essai Shinku Jumelle 2.0'; Version = '2.0'
       Folder = (Join-Path $ProgramsRoot 'Essai Shinku Jumelle 2.0'); Location = $true
       Data = $null; Link = $null }
)

# Les réglages communs de la famille, et le leurre (le nom de l'éditeur seul).
$FamilyData = Join-Path $RoamingRoot 'Essai Shinku Jumelle'
$Decoy = Join-Path $RoamingRoot 'Essai Shinku'

# ── Nettoyer ───────────────────────────────────────────────────────────────

# Retire un dossier d'essai, s'il existe ; jamais d'erreur. Refuse tout chemin
# dont le dernier nom ne commence pas par « Essai Shinku ».
function Remove-Folder([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { return }
    try {
        $last = Split-Path -Leaf $Path
        if (-not $last.StartsWith('Essai Shinku')) { Fail "refus de retirer $Path" }
        if (Test-Path -LiteralPath $Path) {
            Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
            Say "  retiré : $Path"
        }
    } catch {
        Say "  impossible de retirer $Path : $($_.Exception.Message)"
    }
}

function Remove-File([string]$Path) {
    try {
        if (Test-Path -LiteralPath $Path) {
            Remove-Item -LiteralPath $Path -Force -ErrorAction Stop
            Say "  retiré : $Path"
        }
    } catch {
        Say "  impossible de retirer $Path : $($_.Exception.Message)"
    }
}

function Remove-AppKey([string]$Key) {
    try {
        if (-not $Key.StartsWith('EssaiShinku')) { Fail "refus de retirer la clé $Key" }
        $path = Join-Path $UninstallRoot $Key
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction Stop
            Say "  retiré : $path"
        }
    } catch {
        Say "  impossible de retirer la clé $Key : $($_.Exception.Message)"
    }
}

function Clean-Up {
    Say "retrait des apps d'essai : inscriptions, dossiers, raccourcis"
    foreach ($app in $Apps) {
        Remove-AppKey $app.Key
        Remove-Folder $app.Folder
        Remove-Folder $app.Data
        if ($null -ne $app.Link) { Remove-File (Join-Path $StartPrograms $app.Link) }
    }
    Remove-Folder $FamilyData
    Remove-Folder $Decoy
}

# ── Préparer ───────────────────────────────────────────────────────────────

function Write-Text([string]$Path, [string]$Text) {
    $folder = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $folder)) { New-Item -ItemType Directory -Path $folder | Out-Null }
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.ASCIIEncoding))
    if (-not (Test-Path -LiteralPath $Path)) { Fail "$Path n'a pas été écrit" }
}

# Le désinstalleur d'essai : il retire app.exe, puis son inscription, et laisse
# le dossier avec retirer.cmd et reste.txt. Rien ici ne nomme de dossier de
# l'utilisateur : le fichier est en ASCII.
function Uninstaller-Text([string]$Key) {
    $lines = @(
        '@echo off',
        'del /f /q "%~dp0app.exe"',
        "reg delete `"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Key`" /f",
        'exit /b 0'
    )
    return (($lines -join "`r`n") + "`r`n")
}

function Set-Value([string]$Key, [string]$Name, $Value, [string]$Type) {
    New-ItemProperty -LiteralPath $Key -Name $Name -Value $Value -PropertyType $Type -Force | Out-Null
    $read = (Get-ItemProperty -LiteralPath $Key -Name $Name).$Name
    if ("$read" -ne "$Value") { Fail "$Key\$Name vaut « $read » au lieu de « $Value »" }
}

function New-Link([string]$Path, [string]$Target) {
    $folder = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $folder)) { New-Item -ItemType Directory -Path $folder | Out-Null }
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($Path)
    $link.TargetPath = $Target
    $link.Save()
    if (-not (Test-Path -LiteralPath $Path)) { Fail "le raccourci $Path n'a pas été créé" }
}

function Register-App($App) {
    $folder = $App.Folder
    Write-Text (Join-Path $folder 'app.exe') 'MZ'
    $uninstaller = Join-Path $folder 'retirer.cmd'
    Write-Text $uninstaller (Uninstaller-Text $App.Key)
    Write-Text (Join-Path $folder 'reste.txt') 'un fichier que le desinstalleur laisse'
    if ($null -ne $App.Data) {
        Write-Text (Join-Path $App.Data 'reglages.json') '{}'
    }
    if ($null -ne $App.Link) {
        New-Link (Join-Path $StartPrograms $App.Link) (Join-Path $folder 'app.exe')
    }

    # Une clé neuve : le nettoyage d'avant l'a retirée. Jamais -Force ici.
    $key = Join-Path $UninstallRoot $App.Key
    if (Test-Path -LiteralPath $key) { Fail "la clé $key existe déjà" }
    New-Item -Path $key | Out-Null
    Set-Value $key 'DisplayName' $App.Name 'String'
    Set-Value $key 'Publisher' 'Essai Shinku' 'String'
    Set-Value $key 'DisplayVersion' $App.Version 'String'
    Set-Value $key 'EstimatedSize' 2048 'DWord'
    Set-Value $key 'UninstallString' ('"' + $uninstaller + '"') 'String'
    if ($App.Location) { Set-Value $key 'InstallLocation' $folder 'String' }
    Done "$($App.Name) : $key, dossier $folder"
}

function Prepare {
    Say "nettoyage d'un essai interrompu"
    Clean-Up

    # La clé parente peut manquer sur une image neuve : elle seule est créée,
    # jamais avec -Force, et le nettoyage ne la retire jamais.
    if (-not (Test-Path -LiteralPath $UninstallRoot)) {
        New-Item -Path $UninstallRoot | Out-Null
        Say "  clé parente créée : $UninstallRoot"
    }

    Say "création des apps d'essai"
    foreach ($app in $Apps) { Register-App $app }

    # Les réglages communs de la famille, et le leurre : jamais proposés.
    Write-Text (Join-Path $FamilyData 'commun.json') '{}'
    Write-Text (Join-Path $Decoy 'note.txt') 'le nom de l editeur seul : ni une app, ni un reste'
    Done "réglages communs $FamilyData, leurre $Decoy"
}

# ── Programme ──────────────────────────────────────────────────────────────

if ([bool]$Preparer -eq [bool]$Nettoyer) {
    Write-Host 'Usage : essai-desinstallation.ps1 -Preparer | -Nettoyer'
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
