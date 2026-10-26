# Instala o APK e envia os mapas no mesmo passo.
# O som do skatista ja vai dentro do APK. O cache do jogo de PC, se existir, vai junto.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\Comum.ps1"

$pc = Join-Path $Kit 'pc'
$markerPath = Join-Path $pc 'data\installation.json'
if (-not (Test-Path -LiteralPath $markerPath)) {
    throw 'O jogo ainda nao foi convertido. Rode 1-Converter.bat e escolha o ISO de Xbox 360.'
}
$marker = Get-Content -LiteralPath $markerPath -Raw -Encoding UTF8 | ConvertFrom-Json
$directory = [string]$marker.directory
if ($directory -notmatch '^installations/[0-9a-f]{32}$') {
    throw 'installation.json invalido. Rode 1-Converter.bat de novo.'
}
$assets = Join-Path $pc ('data\' + ($directory -replace '/', '\') + '\assets')
if (-not (Test-Path -LiteralPath (Join-Path $assets 'private\game.json'))) {
    throw "Pasta convertida incompleta: $assets"
}

$apk = Get-ChildItem -LiteralPath (Join-Path $Kit 'apk') -Filter *.apk | Select-Object -First 1
if (-not $apk) { throw 'Nenhum APK na pasta apk. Rode 1-Converter.bat uma vez para baixar.' }

$adb = Get-QuestAdb
Write-Host "Instalando $($apk.Name) ..."
& $adb install -r --no-streaming $apk.FullName
if ($LASTEXITCODE -ne 0) {
    & $adb install -r $apk.FullName
    if ($LASTEXITCODE -ne 0) { throw 'O Quest recusou o APK.' }
}

Write-Host 'Empacotando mapas e personagem...'
$pack = Join-Path $Kit '_download\quest-assets.tar'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $pack) | Out-Null
if (Test-Path -LiteralPath $pack) { Remove-Item -LiteralPath $pack -Force }
& tar.exe -cf $pack -C $assets .
if ($LASTEXITCODE -ne 0) { throw 'Nao consegui empacotar a pasta do jogo.' }

Write-Host 'Enviando para o Quest...'
& $adb push $pack /data/local/tmp/skate3-assets.tar
if ($LASTEXITCODE -ne 0) { throw 'Falha ao copiar o pacote para o Quest.' }
& $adb shell "mkdir -p $HeadsetFiles/assets && tar -xf /data/local/tmp/skate3-assets.tar -C $HeadsetFiles/assets && rm -f /data/local/tmp/skate3-assets.tar && test -f $HeadsetFiles/assets/private/game.json"
if ($LASTEXITCODE -ne 0) { throw 'O Quest nao gravou a pasta do jogo.' }

$cache = Join-Path $env:LOCALAPPDATA 'Skate3RustEngine\audio-pcm-cache'
$banks = @(Get-ChildItem -LiteralPath $cache -Filter *.v3.pcm -ErrorAction SilentlyContinue)
if ($banks.Count -gt 0) {
    Write-Host 'Enviando som extra do jogo de PC...'
    & $adb push --sync "$cache\." "$HeadsetFiles/Skate3RustEngine/audio-pcm-cache/"
    if ($LASTEXITCODE -ne 0) {
        & $adb push "$cache\." "$HeadsetFiles/Skate3RustEngine/audio-pcm-cache/"
        if ($LASTEXITCODE -ne 0) { throw 'Falha ao enviar o som extra.' }
    }
}
$global:LASTEXITCODE = 0
Write-Host 'Pronto. Abra VR MARCO PRO SKATER no Quest, em fontes desconhecidas.'
