# Passo 2: instala o VR MARCO PRO SKATER no Quest e envia os dados do jogo preparados no passo 1.
# Chamado por 2-INSTALAR-NO-QUEST.bat.
. "$PSScriptRoot\Comum.ps1"

$apk = Get-ChildItem -LiteralPath (Join-Path $Kit 'apk') -Filter *.apk | Sort-Object Name -Descending | Select-Object -First 1
if (-not $apk) { throw 'Nao achei o APK na pasta apk. Baixe o kit de novo.' }
if (-not (Test-Path -LiteralPath $Pack)) { throw 'Os dados do jogo ainda nao foram preparados. Rode 1-PREPARAR-JOGO.bat primeiro.' }

Step 'Procurando o Quest no cabo USB...'
$adb = Get-QuestAdb

Step "Instalando $($apk.Name)..."
$out = & $adb install -r $apk.FullName 2>&1 | Out-String
Write-Host $out.Trim()
if ($out -notmatch 'Success') {
    if ($out -match 'UPDATE_INCOMPATIBLE|signatures do not match|INCONSISTENT_CERTIFICATES') {
        Write-Host 'Havia outra versao com assinatura diferente. Removendo e instalando de novo...'
        & $adb uninstall $Package | Out-Null
        $out = & $adb install $apk.FullName 2>&1 | Out-String
        Write-Host $out.Trim()
    }
    if ($out -notmatch 'Success') { throw 'O Quest recusou o APK.' }
}

$mb = [math]::Round((Get-Item -LiteralPath $Pack).Length / 1MB)
Step "Enviando mapas e skatista ($mb MB, leva um minuto)..."
& $adb push $Pack /data/local/tmp/skate3-assets.tar
if ($LASTEXITCODE -ne 0) { throw 'Falha ao copiar os dados para o Quest.' }
Step 'Descompactando no Quest...'
& $adb shell "mkdir -p $HeadsetFiles/assets && tar -xf /data/local/tmp/skate3-assets.tar -C $HeadsetFiles/assets && rm -f /data/local/tmp/skate3-assets.tar && test -f $HeadsetFiles/assets/private/game.json"
if ($LASTEXITCODE -ne 0) { throw 'O Quest nao gravou os dados do jogo.' }

Step 'Pronto!'
Write-Host 'No Quest: Biblioteca > Fontes desconhecidas > "VR MARCO PRO SKATER".'
Write-Host 'A versao aparece no canto de cima do menu (botao MENU do controle esquerdo).'
