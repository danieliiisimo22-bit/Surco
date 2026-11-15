param([Parameter(Mandatory)][string]$Directory)

$ErrorActionPreference = 'Stop'
$package = (Resolve-Path -LiteralPath $Directory).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('surco-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$previousPath = $env:PATH
$previousProfile = $env:SURCO_DATA
$OutputEncoding = [Text.UTF8Encoding]::new($false)
$server = $null
try {
    $env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    $env:SURCO_DATA = Join-Path $scratch 'perfil'
    $python = Join-Path $package 'runtime\python\python.exe'
    $node = Join-Path $package 'runtime\node.exe'
    $ffmpeg = Join-Path $package 'runtime\ffmpeg.exe'

    & $python -B -c 'import sys, requests, truststore, ytmusicapi, yt_dlp, mutagen, PIL.Image; assert sys.flags.isolated'
    if ($LASTEXITCODE -ne 0) { throw 'Python no se pudo iniciar de forma aislada.' }
    & $python -B (Join-Path $package 'bridge\download.py') --version
    if ($LASTEXITCODE -ne 0) { throw 'yt-dlp no se pudo iniciar.' }
    & $node --version
    if ($LASTEXITCODE -ne 0) { throw 'Node.js no se pudo iniciar.' }

    $tone = Join-Path $scratch 'tono.mp3'
    & $ffmpeg -v error -f lavfi -i 'sine=frequency=440:duration=1' -c:a libmp3lame $tone
    if ($LASTEXITCODE -ne 0) { throw 'FFmpeg no pudo codificar un MP3.' }
    $request = ConvertTo-Json -InputObject @{ op = 'inspect'; path = $tone } -Compress
    $metadata = $request | & $python -B (Join-Path $package 'bridge\catalog.py') | ConvertFrom-Json
    if (-not $metadata.ok -or $metadata.data.duration -lt 1) { throw 'El adaptador no pudo leer el MP3.' }

    $server = Start-Process -FilePath (Join-Path $package 'surco.exe') -ArgumentList '--headless', '--ephemeral-port' `
        -WorkingDirectory $scratch -WindowStyle Hidden -PassThru
    $started = Join-Path $env:SURCO_DATA 'startup.json'
    $deadline = (Get-Date).AddSeconds(12)
    while (-not (Test-Path -LiteralPath $started) -and -not $server.HasExited -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 150
    }
    if ($server.HasExited -or -not (Test-Path -LiteralPath $started)) { throw 'El motor no se pudo iniciar.' }
    $port = (Get-Content -LiteralPath $started -Raw | ConvertFrom-Json).port
    if (-not $port) { throw 'El motor no publico su puerto local.' }
    $url = "http://127.0.0.1:$port"
    $health = Invoke-RestMethod -Uri "$url/api/health" -TimeoutSec 5
    $bootstrap = Invoke-RestMethod -Uri "$url/api/bootstrap" -TimeoutSec 5
    if ($health.app -ne 'Surco Rust' -or -not $bootstrap.ready -or -not $bootstrap.token) {
        throw 'Surco no detecto todos los componentes del ZIP.'
    }
    $page = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 5
    if ($page.Content -notmatch '<title>Surco</title>') { throw 'La interfaz no respondio.' }
    $jobs = Invoke-RestMethod -Uri "$url/api/jobs" -TimeoutSec 5
    if ($jobs.Count -ne 0) { throw 'El perfil nuevo debe empezar sin descargas.' }
    Invoke-RestMethod -Uri "$url/api/shutdown" -Method Post -Headers @{ 'X-Surco-Token' = $bootstrap.token } -TimeoutSec 5 | Out-Null
    if (-not $server.WaitForExit(5000) -or $server.ExitCode -ne 0) { throw 'Surco no se cerro correctamente.' }
    Write-Host 'Paquete portable verificado.'
} finally {
    if ($server -and -not $server.HasExited) {
        Stop-Process -InputObject $server -Force
        $server.WaitForExit(5000) | Out-Null
    }
    $env:PATH = $previousPath
    $env:SURCO_DATA = $previousProfile
    $temp = [IO.Path]::GetTempPath().TrimEnd('\')
    if ((Split-Path -Parent ([IO.Path]::GetFullPath($scratch))) -eq $temp -and (Test-Path -LiteralPath $scratch)) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
