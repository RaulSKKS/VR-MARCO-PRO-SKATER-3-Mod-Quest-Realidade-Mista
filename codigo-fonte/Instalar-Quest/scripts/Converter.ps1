# Converte a pasta ja extraida. Nao abre o seletor de ISO:
# o caminho com espacos faz o extrator parar em "File not found".
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\Comum.ps1"

$pc = Join-Path $Kit 'pc'
$game = Get-ChildItem -LiteralPath $pc -Directory | Where-Object {
    (Test-Path -LiteralPath (Join-Path $_.FullName 'default.xex')) -and
    (Test-Path -LiteralPath (Join-Path $_.FullName 'data\big\miscload.big')) -and
    (Test-Path -LiteralPath (Join-Path $_.FullName 'data\big\db.big')) -and
    (Test-Path -LiteralPath (Join-Path $_.FullName 'data\content\createacharacter.big'))
} | Select-Object -First 1

if (-not $game) {
    throw 'Nao achei o jogo extraido ao lado do skate3rust.exe. Feche a janela de erro do setup e rode de novo.'
}

Write-Host 'Usando a pasta ja extraida. Nao escolha o ISO.'
Write-Host $game.FullName
Write-Host 'A conversao dos mapas pode demorar. Deixe esta janela aberta.'
Write-Host ''

$setup = Join-Path $pc 'support\skate3setup.exe'
$exe = Join-Path $pc 'skate3rust.exe'
& $setup --task tools/prepare_assets.py --game-root $game.FullName --output (Join-Path $pc 'data') --game-exe $exe
if ($LASTEXITCODE -ne 0) {
    throw "A conversao parou (codigo $LASTEXITCODE)."
}

$global:LASTEXITCODE = 0
Write-Host ''
Write-Host 'Conversao pronta. Abrindo o jogo para gerar o som.'
Write-Host 'Quando o mapa aparecer, feche o jogo e rode 2-Enviar-Quest.bat'
& $exe
if ($LASTEXITCODE -ne 0) { throw "O jogo fechou com codigo $LASTEXITCODE" }
$global:LASTEXITCODE = 0
