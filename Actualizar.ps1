$ErrorActionPreference = 'Stop'
$python = Join-Path $PSScriptRoot 'runtime\python\Scripts\python.exe'
if (-not (Test-Path -LiteralPath $python)) {
    throw 'Ejecuta Preparar.ps1 antes de actualizar yt-dlp.'
}
& $python -m pip install --upgrade yt-dlp yt-dlp-ejs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $python -m yt_dlp --version
exit $LASTEXITCODE
