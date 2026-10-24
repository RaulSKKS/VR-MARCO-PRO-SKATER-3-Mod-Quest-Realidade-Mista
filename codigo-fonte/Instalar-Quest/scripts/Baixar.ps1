# Baixa o APK e o conversor de PC da mesma versao publicada, se ainda nao estiverem aqui.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\Comum.ps1"

$Release = 'https://github.com/andrewnakas/skate3-quest-ar/releases/download/v0.0.2-alpha'
$ApkName = 'skate3ar-quest-v0.0.2-alpha.apk'
$ApkSha = 'e1e70c18df5d1b89c0d501aa9c6b4064bc8662548c07768787f36d288114aee3'
$ZipName = 'skate3rust-windows-x64.zip'
$ZipSha = 'cf2efade20832f560f2e53d1d9de0f85db85869a967ce4cfb61eccdb1bbe3f09'

$apkDir = Join-Path $Kit 'apk'
$pc = Join-Path $Kit 'pc'
$download = Join-Path $Kit '_download'
New-Item -ItemType Directory -Force -Path $apkDir, $pc, $download | Out-Null

function Test-Sha256([string]$Path, [string]$Expected) {
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    $got = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    return $got -eq $Expected.ToLowerInvariant()
}

function Save-Verified([string]$Name, [string]$Expected) {
    $dest = Join-Path $download $Name
    if (Test-Sha256 $dest $Expected) { return $dest }
    if (Test-Path -LiteralPath $dest) { Remove-Item -LiteralPath $dest -Force }
    Write-Host "Baixando $Name ..."
    & curl.exe -L --fail --retry 3 -o $dest "$Release/$Name"
    if ($LASTEXITCODE -ne 0) { throw "Download falhou: $Name" }
    if (-not (Test-Sha256 $dest $Expected)) { throw "O arquivo $Name nao confere com a versao publicada." }
    return $dest
}

$apkSrc = Save-Verified $ApkName $ApkSha
$apkDest = Join-Path $apkDir $ApkName
if (-not (Test-Sha256 $apkDest $ApkSha)) {
    Copy-Item -LiteralPath $apkSrc -Destination $apkDest -Force
}

$exe = Join-Path $pc 'skate3rust.exe'
$setup = Join-Path $pc 'support\skate3setup.exe'
if (-not ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $setup))) {
    $zip = Save-Verified $ZipName $ZipSha
    $tmp = Join-Path $download 'pc-zip'
    if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force }
    Expand-Archive -LiteralPath $zip -DestinationPath $tmp
    $found = Get-ChildItem -LiteralPath $tmp -Recurse -Filter skate3rust.exe | Select-Object -First 1
    if (-not $found) { throw 'O zip do Windows nao contem skate3rust.exe.' }
    & robocopy $found.Directory.FullName $pc /E /NFL /NDL /NJH /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "Nao consegui extrair o conversor de PC (robocopy $LASTEXITCODE)." }
    $global:LASTEXITCODE = 0
    Remove-Item -LiteralPath $tmp -Recurse -Force
}

if (-not (Test-Path -LiteralPath (Join-Path $pc 'release.json'))) {
    throw 'Pacote de PC incompleto: falta release.json ao lado do skate3rust.exe.'
}
$global:LASTEXITCODE = 0
Write-Host 'APK e conversor de PC prontos.'
