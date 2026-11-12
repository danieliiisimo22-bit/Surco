param([string]$Python = 'python')

$ErrorActionPreference = 'Stop'
$runtime = Join-Path $PSScriptRoot 'runtime'
$environment = Join-Path $runtime 'python'
$interpreter = Join-Path $environment 'Scripts\python.exe'

foreach ($tool in @('ffmpeg.exe', 'node.exe')) {
    if (-not (Test-Path -LiteralPath (Join-Path $runtime $tool)) -and
        -not (Get-Command $tool -CommandType Application -ErrorAction SilentlyContinue)) {
        throw "Falta $tool. Instala el programa y vuelve a abrir PowerShell."
    }
}

if (-not (Test-Path -LiteralPath $interpreter)) {
    & $Python -m venv $environment
    if ($LASTEXITCODE -ne 0) { throw 'No se pudo crear el entorno de Python.' }
}

& $interpreter -m pip install --requirement (Join-Path $PSScriptRoot 'requirements.txt')
if ($LASTEXITCODE -ne 0) { throw 'No se pudieron instalar las dependencias de Python.' }
