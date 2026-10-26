# Passo 1: prepara o jogo a partir da SUA ISO do Skate 3 (Xbox 360). Extrai o disco e converte mapas e skatista
# para o formato do VR MARCO PRO SKATER. Tudo fica em _trabalho\ e nada disso vai para o git (so o extrator de ISO,
# que ja vem no kit em _trabalho\extract-xiso). Chamado por 1-PREPARAR-JOGO.bat.
. "$PSScriptRoot\Comum.ps1"

# O conversor publicado pelo projeto Skate 3 AR (andrewnakas/skate3-quest-ar), conferido pelo hash.
$ConverterUrl = 'https://github.com/andrewnakas/skate3-quest-ar/releases/download/v0.0.2-alpha/skate3rust-windows-x64.zip'
$ConverterSha = 'cf2efade20832f560f2e53d1d9de0f85db85869a967ce4cfb61eccdb1bbe3f09'
# O extrator de ISO de Xbox que o proprio conversor usa (XboxDev/extract-xiso), mesma versao e hash. Ja vem no kit;
# so e baixado se a copia do kit sumir.
$XisoUrl = 'https://github.com/XboxDev/extract-xiso/releases/download/build-202505152050/extract-xiso-Win64_Release.zip'
$XisoSha = 'fec88d03c7efd6205ab09be4abba70c0afd0eb27a5709f0a6235b828ba5ac11e'

function Test-Disc([string]$Dir) {
    (Test-Path -LiteralPath (Join-Path $Dir 'default.xex')) -and
    (Test-Path -LiteralPath (Join-Path $Dir 'data\big\miscload.big')) -and
    (Test-Path -LiteralPath (Join-Path $Dir 'data\big\db.big')) -and
    (Test-Path -LiteralPath (Join-Path $Dir 'data\content\createacharacter.big'))
}

Step 'Procurando o seu Skate 3...'
$isoDir = Join-Path $Kit 'COLOQUE-SUA-ISO-AQUI'
$iso = Get-ChildItem -LiteralPath $isoDir -Filter *.iso -File -ErrorAction SilentlyContinue | Select-Object -First 1
# (um disco ja extraido nessa pasta tambem serve)
$given = Get-ChildItem -LiteralPath $isoDir -Directory -ErrorAction SilentlyContinue | Where-Object { Test-Disc $_.FullName } | Select-Object -First 1
$disc = Join-Path $Work 'disco'
if ($given) {
    $disc = $given.FullName
    Write-Host "Usando o disco extraido: $($given.Name)"
} elseif ($iso) {
    Write-Host "ISO: $($iso.Name)"
} else {
    throw 'Nao achei a ISO. Coloque o arquivo .iso do Skate 3 (Xbox 360) na pasta COLOQUE-SUA-ISO-AQUI e rode de novo.'
}

Step 'Preparando o conversor...'
$conv = Join-Path $Work 'conversor'
$exe = Join-Path $conv 'skate3rust.exe'
$setup = Join-Path $conv 'support\skate3setup.exe'
if (-not ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $setup))) {
    $zip = Save-Download $ConverterUrl 'skate3rust-windows-x64.zip' $ConverterSha
    $tmp = Join-Path $Work 'conversor-zip'
    if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force }
    Expand-Archive -LiteralPath $zip -DestinationPath $tmp
    $found = Get-ChildItem -LiteralPath $tmp -Recurse -Filter skate3rust.exe | Select-Object -First 1
    if (-not $found) { throw 'O pacote do conversor veio sem o skate3rust.exe.' }
    if (Test-Path -LiteralPath $conv) { Remove-Item -LiteralPath $conv -Recurse -Force }
    Move-Item -LiteralPath $found.Directory.FullName -Destination $conv
    Remove-Item -LiteralPath $tmp -Recurse -Force
}
Write-Host 'Conversor pronto.'

if (-not $given) {
    if (Test-Disc $disc) {
        Write-Host 'O disco ja foi extraido antes.'
    } else {
        Step 'Extraindo a ISO (alguns minutos)...'
        $tools = Join-Path $Work 'extract-xiso'
        $xiso = Get-ChildItem -LiteralPath $tools -Recurse -Filter extract-xiso.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $xiso) {
            $zip = Save-Download $XisoUrl 'extract-xiso.zip' $XisoSha
            Expand-Archive -LiteralPath $zip -DestinationPath $tools -Force
            $xiso = Get-ChildItem -LiteralPath $tools -Recurse -Filter extract-xiso.exe | Select-Object -First 1
        }
        if (-not $xiso) { throw 'O extrator de ISO veio incompleto.' }
        # O extrator se perde com espacos no caminho: ele trabalha dentro de _trabalho com nomes curtos.
        $link = Join-Path $Work 'skate3.iso'
        if (Test-Path -LiteralPath $link) { Remove-Item -LiteralPath $link -Force }
        try {
            New-Item -ItemType HardLink -Path $link -Target $iso.FullName | Out-Null
        } catch {
            Write-Host 'Copiando a ISO para _trabalho (outro disco)...'
            Copy-Item -LiteralPath $iso.FullName -Destination $link
        }
        # Algumas versoes do extrator ignoram o -d e criam a pasta skate3 (nome da ISO). Se uma tentativa anterior
        # parou no meio, essa pasta sobra e ele recusa extrair ("Unable to create directory skate3: File exists").
        $leftover = Join-Path $Work 'skate3'
        foreach ($old in @($disc, $leftover)) {
            if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Recurse -Force }
        }
        Push-Location $Work
        try {
            & $xiso.FullName -x 'skate3.iso' -d 'disco'
            $code = $LASTEXITCODE
        } finally {
            Pop-Location
            Remove-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue
        }
        if (-not (Test-Disc $disc)) {
            $other = @((Join-Path $disc 'skate3'), $leftover) | Where-Object { Test-Disc $_ } | Select-Object -First 1
            if ($other) { $disc = $other }
        }
        if (-not (Test-Disc $disc)) {
            if ($code -ne 0) {
                throw 'O extrator nao conseguiu extrair a ISO (o motivo esta nas linhas em ingles acima). Confira se a ISO baixou inteira (o Skate 3 tem cerca de 7 GB) e se ha espaco livre no disco, e rode de novo.'
            }
            throw 'Essa ISO nao parece ser o Skate 3 de Xbox 360 (faltam arquivos do jogo depois de extrair).'
        }
    }
}

Step 'Convertendo mapas e skatista (pode demorar bastante; deixe a janela aberta)...'
& $setup --task tools/prepare_assets.py --game-root $disc --output (Join-Path $conv 'data') --game-exe $exe
if ($LASTEXITCODE -ne 0) { throw "A conversao parou (codigo $LASTEXITCODE). Rode de novo: ela continua de onde parou." }

Step 'Empacotando para o Quest...'
$marker = Get-Content -LiteralPath (Join-Path $conv 'data\installation.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$directory = [string]$marker.directory
if ($directory -notmatch '^installations/[0-9a-f]{32}$') { throw 'A conversao nao terminou direito (installation.json). Rode de novo.' }
$assets = Join-Path $conv ('data\' + ($directory -replace '/', '\') + '\assets')
if (-not (Test-Path -LiteralPath (Join-Path $assets 'private\game.json'))) { throw "Conversao incompleta: $assets" }
if (Test-Path -LiteralPath $Pack) { Remove-Item -LiteralPath $Pack -Force }
& (Get-Tar) -cf $Pack -C $assets .
if ($LASTEXITCODE -ne 0) { throw 'Nao consegui empacotar os dados do jogo.' }
$mb = [math]::Round((Get-Item -LiteralPath $Pack).Length / 1MB)

Step 'Pronto!'
Write-Host "Dados do jogo prontos ($mb MB). Conecte o Quest no cabo e rode 2-INSTALAR-NO-QUEST.bat."
Write-Host 'Se quiser, pode apagar a ISO e _trabalho\disco agora; o pacote do Quest ja esta feito.'
