# Instala o APK pelo cabo. O mesmo arquivo em apk\ serve para arrastar no SideQuest.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\Comum.ps1"

$apk = Get-ChildItem -LiteralPath (Join-Path $Kit 'apk') -Filter *.apk | Select-Object -First 1
if (-not $apk) { throw 'Nenhum APK na pasta apk. Rode 1-Converter.bat uma vez para baixar, ou Baixar.bat.' }

$adb = Get-QuestAdb
Write-Host "Instalando $($apk.Name) ..."
& $adb install -r --no-streaming $apk.FullName
if ($LASTEXITCODE -ne 0) {
    & $adb install -r $apk.FullName
    if ($LASTEXITCODE -ne 0) { throw 'O Quest recusou o APK.' }
}
$global:LASTEXITCODE = 0
Write-Host 'APK instalado. Depois de converter o disco, rode 2-Enviar-Quest.bat.'
