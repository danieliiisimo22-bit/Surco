param([string]$Destination = (Join-Path $PSScriptRoot 'surco.exe'))

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
$originalRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
$registry = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$releaseFlags = @(
    "--remap-path-prefix=$PSScriptRoot=surco"
    "--remap-path-prefix=$registry=cargo"
)
if ($originalRustFlags) { $releaseFlags = @($originalRustFlags) + $releaseFlags }
elseif ($env:RUSTFLAGS) { $releaseFlags = ($env:RUSTFLAGS -split '\s+' | Where-Object { $_ }) + $releaseFlags }
try {
    $env:CARGO_ENCODED_RUSTFLAGS = $releaseFlags -join [char]31
    & $cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Fallo la compilacion.' }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $originalRustFlags
}
Copy-Item -LiteralPath '.\target\release\surco.exe' -Destination $Destination -Force
Write-Host 'Surco compilado.'
