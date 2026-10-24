# Funcoes usadas pelos dois .bat do kit. Nao rode este arquivo direto.
$ErrorActionPreference = 'Stop'

$script:Kit = Split-Path -Parent $PSScriptRoot
$script:Work = Join-Path $Kit '_trabalho'
$script:Package = 'com.andrewnakas.skate3ar'
$script:HeadsetFiles = "/sdcard/Android/data/$script:Package/files"
$script:Pack = Join-Path $script:Work 'quest-assets.tar'
New-Item -ItemType Directory -Force -Path $script:Work | Out-Null

function Step([string]$Text) {
    Write-Host ''
    Write-Host "==> $Text" -ForegroundColor Cyan
}

function Test-Sha256([string]$Path, [string]$Expected) {
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    $got = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    return $got -eq $Expected.ToLowerInvariant()
}

# Baixa um arquivo para _trabalho\downloads; com hash, so aceita o arquivo publicado.
function Save-Download([string]$Url, [string]$Name, [string]$Sha) {
    $dir = Join-Path $script:Work 'downloads'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $dest = Join-Path $dir $Name
    if ($Sha -and (Test-Sha256 $dest $Sha)) { return $dest }
    if (Test-Path -LiteralPath $dest) { Remove-Item -LiteralPath $dest -Force }
    Write-Host "Baixando $Name ..."
    & curl.exe -L --fail --retry 3 -o $dest $Url
    if ($LASTEXITCODE -ne 0) { throw "O download falhou: $Name. Confira a internet e rode de novo." }
    if ($Sha -and -not (Test-Sha256 $dest $Sha)) { throw "O arquivo $Name nao confere com o publicado." }
    return $dest
}

# O tar do Windows (o do Git, se estiver no PATH, nao entende C:\).
function Get-Tar {
    $tar = Join-Path $env:SystemRoot 'System32\tar.exe'
    if (-not (Test-Path -LiteralPath $tar)) { throw 'Este Windows nao tem o tar.exe (Windows 10 1803 ou mais novo).' }
    return $tar
}

function Find-Adb {
    $candidates = @(
        (Join-Path $script:Work 'platform-tools\adb.exe')
        (Get-Command adb -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
        "$env:ProgramFiles\SideQuest\resources\platform-tools\adb.exe"
        "$env:LOCALAPPDATA\Android\Sdk\platform-tools\adb.exe"
        $(if ($env:ANDROID_HOME) { Join-Path $env:ANDROID_HOME 'platform-tools\adb.exe' })
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
    $adb = $candidates | Select-Object -First 1
    if ($adb) { return $adb }
    # Nenhum adb no PC: o oficial do Google, so para este kit.
    $zip = Save-Download 'https://dl.google.com/android/repository/platform-tools-latest-windows.zip' 'platform-tools.zip' ''
    Expand-Archive -LiteralPath $zip -DestinationPath $script:Work -Force
    $adb = Join-Path $script:Work 'platform-tools\adb.exe'
    if (-not (Test-Path -LiteralPath $adb)) { throw 'Nao consegui preparar o adb.' }
    return $adb
}

function Get-QuestAdb {
    $adb = Find-Adb
    & $adb start-server | Out-Null
    $lines = @(& $adb devices)
    $ready = @($lines | Where-Object { $_ -match '^\S+\s+device$' })
    if (@($lines | Where-Object { $_ -match 'unauthorized' }).Count -gt 0 -and $ready.Count -eq 0) {
        throw 'O Quest pediu autorizacao. Coloque o oculos, marque "Sempre permitir" na depuracao USB e rode de novo.'
    }
    if ($ready.Count -eq 0) {
        throw 'Nenhum Quest encontrado. Ligue o oculos, conecte o cabo USB, ative o modo desenvolvedor e rode de novo.'
    }
    if ($ready.Count -gt 1) { throw 'Ha mais de um aparelho conectado. Deixe so o Quest no cabo.' }
    Write-Host "Quest encontrado: $(($ready[0] -split '\s+')[0])"
    return $adb
}
