$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$localTools = Join-Path $PSScriptRoot '.build-tools'
if (Test-Path -LiteralPath (Join-Path $localTools 'cargo\bin\cargo.exe')) {
    $env:CARGO_HOME = Join-Path $localTools 'cargo'
    $env:RUSTUP_HOME = Join-Path $localTools 'rustup'
    $cargo = Join-Path $localTools 'cargo\bin\cargo.exe'
} else { $cargo = 'cargo' }
$python = Join-Path $PSScriptRoot 'runtime\python\Scripts\python.exe'
if (-not (Test-Path -LiteralPath $python)) { throw 'Ejecuta Preparar.ps1 antes de compilar.' }
$node = Join-Path $PSScriptRoot 'runtime\node.exe'
if (-not (Test-Path -LiteralPath $node)) { $node = 'node.exe' }

& $cargo test --locked
if ($LASTEXITCODE -ne 0) { throw 'Fallaron las pruebas Rust.' }
& $python -m unittest discover -s tests
if ($LASTEXITCODE -ne 0) { throw 'Fallaron las pruebas del catalogo.' }
& $node --test tests/ui.cjs
if ($LASTEXITCODE -ne 0) { throw 'Fallaron las pruebas de interfaz.' }
& $cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Fallo la compilacion.' }
Copy-Item -LiteralPath '.\target\release\surco.exe' -Destination '.\surco.exe' -Force
Write-Host 'Surco compilado. Abre Iniciar Surco.cmd.'
