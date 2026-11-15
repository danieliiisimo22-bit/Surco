param([string]$Python = 'python')

$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
Add-Type -AssemblyName System.IO.Compression.FileSystem

$version = [regex]::Match((Get-Content -LiteralPath 'Cargo.toml' -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
if (-not $version) { throw 'No se encontro la version de Surco en Cargo.toml.' }
$dist = Join-Path $PSScriptRoot 'dist'
$cache = Join-Path $PSScriptRoot '.build-tools\release-downloads'
New-Item -ItemType Directory -Path $dist, $cache -Force | Out-Null
$work = Join-Path $dist ('package-' + [guid]::NewGuid().ToString('N'))
$package = Join-Path $work 'Surco'
$tools = Join-Path $work 'upstream'
$runtime = Join-Path $package 'runtime'
$embedded = Join-Path $runtime 'python'
$licenses = Join-Path $package 'Licencias'
$devRuntime = Join-Path $PSScriptRoot 'runtime'
New-Item -ItemType Directory -Path $tools, $embedded, $licenses, $devRuntime -Force | Out-Null

function Get-VerifiedArchive($Url, $Sha256) {
    $file = Join-Path $cache ([uri]$Url).Segments[-1]
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        $ProgressPreference = 'SilentlyContinue'
        Write-Host "Descargando $([IO.Path]::GetFileName($file))..."
        Invoke-WebRequest -Uri $Url -OutFile "$file.download" -UseBasicParsing -TimeoutSec 180
        Move-Item -LiteralPath "$file.download" -Destination $file
    }
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $Sha256) {
        throw "La suma SHA-256 no coincide. Revisa la copia en $file."
    }
    return $file
}

try {
    $pythonZip = Get-VerifiedArchive `
        'https://www.python.org/ftp/python/3.14.7/python-3.14.7-embed-amd64.zip' `
        'D297E5FF019966817AD8502465176139F2D3D840FA4ED84B13BED399A6AB1F15'
    $nodeZip = Get-VerifiedArchive `
        'https://nodejs.org/dist/v24.22.1/node-v24.22.1-win-x64.zip' `
        'D2046D28C2C3DCD211AC5145C0BF30E2047EF9DFB306693C8C6E2ECCD8B95FA5'
    $ffmpegZip = Get-VerifiedArchive `
        'https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip' `
        '60F467265B1E312373DBCD92200C2618A74850F98D3D078E94296BB3FA2047BA'

    [IO.Compression.ZipFile]::ExtractToDirectory($pythonZip, $embedded)
    Set-Content -LiteralPath (Join-Path $embedded 'python314._pth') -Encoding Ascii -Value @(
        'python314.zip', '.', 'Lib\site-packages'
    )

    $node = 'node-v24.22.1-win-x64'
    $ffmpeg = 'ffmpeg-9.0.2-essentials_build'
    & tar.exe -xf $nodeZip -C $tools "$node/node.exe" "$node/LICENSE"
    if ($LASTEXITCODE -ne 0) { throw 'No se pudo extraer Node.js.' }
    & tar.exe -xf $ffmpegZip -C $tools "$ffmpeg/bin/ffmpeg.exe" "$ffmpeg/LICENSE" "$ffmpeg/README.txt"
    if ($LASTEXITCODE -ne 0) { throw 'No se pudo extraer FFmpeg.' }
    Copy-Item -LiteralPath (Join-Path $tools "$node/node.exe"), (Join-Path $tools "$ffmpeg/bin/ffmpeg.exe") -Destination $runtime
    Copy-Item -LiteralPath (Join-Path $tools "$node/LICENSE") -Destination (Join-Path $licenses 'Node.js.txt')
    Copy-Item -LiteralPath (Join-Path $tools "$ffmpeg/LICENSE") -Destination (Join-Path $licenses 'FFmpeg-GPL-3.0.txt')
    Copy-Item -LiteralPath (Join-Path $tools "$ffmpeg/README.txt") -Destination (Join-Path $licenses 'FFmpeg-fuentes-y-compilacion.txt')
    Copy-Item -LiteralPath 'web\vendor\glasswork\LICENSE' -Destination (Join-Path $licenses 'Glasswork-MIT.txt')
    Copy-Item -LiteralPath 'docs\componentes.md' -Destination $licenses

    foreach ($tool in @('node.exe', 'ffmpeg.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $devRuntime $tool))) {
            Copy-Item -LiteralPath (Join-Path $runtime $tool) -Destination $devRuntime
        }
    }
    & .\Preparar.ps1 -Python $Python
    & .\Compilar.ps1 -Destination (Join-Path $package 'surco.exe')

    $builderPython = Join-Path $devRuntime 'python\Scripts\python.exe'
    $sitePackages = Join-Path $embedded 'Lib\site-packages'
    & $builderPython -m pip install --disable-pip-version-check --no-compile --only-binary=:all: --no-deps --target $sitePackages -r requirements.txt
    if ($LASTEXITCODE -ne 0) { throw 'No se pudieron empaquetar las dependencias de Python.' }
    # pip generates launchers containing the build interpreter's absolute path.
    # Surco calls its own bridge scripts with the bundled interpreter.
    $launchers = Join-Path $sitePackages 'bin'
    if ((Split-Path -Parent ([IO.Path]::GetFullPath($launchers))) -eq $sitePackages -and (Test-Path -LiteralPath $launchers)) {
        Remove-Item -LiteralPath $launchers -Recurse -Force
    }

    $cargo = if ($env:CARGO_HOME -and (Test-Path -LiteralPath (Join-Path $env:CARGO_HOME 'bin\cargo.exe'))) {
        Join-Path $env:CARGO_HOME 'bin\cargo.exe'
    } else { 'cargo' }
    $metadata = & $cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'No se pudieron consultar las licencias de Rust.' }
    $crates = ConvertFrom-Json -InputObject ($metadata -join "`n")
    foreach ($crate in $crates.packages | Where-Object source) {
        $notice = Join-Path $licenses "Rust\$($crate.name)-$($crate.version)"
        New-Item -ItemType Directory -Path $notice -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $notice 'Licencia.txt') -Encoding Ascii -Value $crate.license
        Get-ChildItem -LiteralPath (Split-Path -Parent $crate.manifest_path) -File |
            Where-Object Name -Match '^(LICENSE|COPYING|NOTICE|COPYRIGHT)' |
            Copy-Item -Destination $notice
    }

    New-Item -ItemType Directory -Path (Join-Path $package 'bridge') | Out-Null
    Copy-Item -LiteralPath 'bridge\catalog.py', 'bridge\download.py' -Destination (Join-Path $package 'bridge')
    Copy-Item -LiteralPath 'Iniciar Surco.cmd' -Destination $package
    $guide = Get-Content -LiteralPath 'docs\uso.md' -Raw -Encoding UTF8
    $guide = $guide -replace '(?m)^#+ ', '' -replace '\*\*|`', ''
    $guide = $guide -replace '\[([^\]]+)\]\((https?://[^)]+)\)', '$1 ($2)'
    Set-Content -LiteralPath (Join-Path $package 'LEEME.txt') -Encoding UTF8 -Value $guide
    & .\tests\portable.ps1 -Directory $package

    $name = "Surco-$version-windows-x64.zip"
    $zip = Join-Path $dist $name
    $stagedZip = Join-Path $work $name
    [IO.Compression.ZipFile]::CreateFromDirectory($package, $stagedZip, 'Optimal', $false)
    $hash = (Get-FileHash -LiteralPath $stagedZip -Algorithm SHA256).Hash.ToLowerInvariant()
    Move-Item -LiteralPath $stagedZip -Destination $zip -Force
    Set-Content -LiteralPath "$zip.sha256" -Encoding Ascii -Value "$hash  $name"
    Write-Host "Listo: $zip"
} finally {
    if ((Split-Path -Parent ([IO.Path]::GetFullPath($work))) -eq $dist -and (Test-Path -LiteralPath $work)) {
        Remove-Item -LiteralPath $work -Recurse -Force
    }
}
