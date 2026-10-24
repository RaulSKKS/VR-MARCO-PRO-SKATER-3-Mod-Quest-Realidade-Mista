# Funções compartilhadas pelos .bat do kit. Não rode este arquivo direto.
$ErrorActionPreference = 'Stop'

$script:Kit = Split-Path -Parent $PSScriptRoot
$script:Package = 'com.andrewnakas.skate3ar'
$script:HeadsetFiles = "/sdcard/Android/data/$script:Package/files"

function Find-Adb {
    $candidates = @(
        (Get-Command adb -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
        "$env:ProgramFiles\SideQuest\resources\platform-tools\adb.exe"
        "$env:LOCALAPPDATA\Android\platform-tools\adb.exe"
        $(if ($env:ANDROID_HOME) { Join-Path $env:ANDROID_HOME 'platform-tools\adb.exe' })
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
    $adb = $candidates | Select-Object -First 1
    if (-not $adb) {
        throw 'adb nao encontrado. Instale o SideQuest ou o Android platform-tools e rode de novo.'
    }
    return $adb
}

function Get-QuestAdb {
    $adb = Find-Adb
    & $adb start-server | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Nao foi possivel iniciar o adb.' }
    $lines = @(& $adb devices)
    $ready = @($lines | Where-Object { $_ -match '^\S+\s+device$' })
    $unauthorized = @($lines | Where-Object { $_ -match 'unauthorized' })
    if ($unauthorized.Count -gt 0 -and $ready.Count -eq 0) {
        throw 'O Quest pediu autorizacao. No oculos, aceite a depuracao USB e rode de novo.'
    }
    if ($ready.Count -eq 0) {
        throw 'Nenhum Quest conectado. Ligue o oculos, passe o cabo USB e ative a depuracao.'
    }
    if ($ready.Count -gt 1) {
        throw 'Ha mais de um aparelho no adb. Deixe so o Quest conectado.'
    }
    return $adb
}
