[CmdletBinding()]
param(
    [switch]$Visualization,
    [switch]$NoBrowser,
    [switch]$SmokeTest,
    [ValidateRange(1, 65535)][int]$Port = 8080,
    [ValidateRange(10, 600)][int]$StartupTimeoutSeconds = 120
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$target = 'x86_64-pc-windows-msvc'
$process = $null
Push-Location $root
try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'Install Rust MSVC and Visual Studio C++ Build Tools first. See docs/windows.md.'
    }
    # Refuse an existing listener: its state must never pass this launcher's health check.
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Any, $Port)
    try { $listener.Start() } catch { throw "Port $Port is already in use. Stop that server or choose -Port." }
    finally { $listener.Stop() }
    $mode = if ($Visualization) { 'viz' } else { 'headless' }
    $buildDir = Join-Path $root "target/windows-$mode"
    $buildArgs = @('build', '--locked', '--release', '-p', 'app', '--target', $target, '--target-dir', $buildDir)
    if (-not $Visualization) { $buildArgs += '--no-default-features' }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed ($LASTEXITCODE). See docs/windows.md for prerequisites." }
    $binary = Join-Path $buildDir "$target/release/ttx.exe"
    $runArgs = @('--port', "$Port")
    if (-not $Visualization) { $runArgs += '--headless' }
    # The repository working directory makes dotenvy load its .env and serves its web/assets.
    $process = Start-Process -FilePath $binary -ArgumentList $runArgs -WorkingDirectory $root -PassThru -NoNewWindow
    $base = "http://127.0.0.1:$Port"
    $deadline = (Get-Date).AddSeconds($StartupTimeoutSeconds)
    $ready = $false
    while ((Get-Date) -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "ttx exited during startup ($($process.ExitCode))." }
        try {
            $state = Invoke-RestMethod "$base/api/state" -TimeoutSec 2
            if ($null -ne $state.world) { $ready = $true; break }
        } catch { Start-Sleep -Milliseconds 250 }
    }
    if (-not $ready) { throw "ttx did not become ready within $StartupTimeoutSeconds seconds." }
    foreach ($path in @('/', '/master.html', '/player.html', '/play.html')) {
        $response = Invoke-WebRequest "$base$path" -UseBasicParsing -TimeoutSec 10
        if ($response.StatusCode -ne 200) { throw "Health check failed: $path" }
    }
    if ($Visualization) {
        # Allow renderer initialization to complete; CI also inspects the startup log.
        Start-Sleep -Seconds 15
        $process.Refresh()
        if ($process.HasExited) { throw "Visualization exited during initialization ($($process.ExitCode))." }
    }
    Write-Host "Ready: $base/play.html (PID $($process.Id), $mode)"
    if (-not $NoBrowser -and -not $SmokeTest) { Start-Process "$base/play.html" }
    if (-not $SmokeTest) {
        Write-Host 'Press Ctrl+C to stop the exercise.'
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "ttx exited with code $($process.ExitCode)." }
    }
} finally {
    if ($null -ne $process -and -not $process.HasExited) { Stop-Process -Id $process.Id }
    Pop-Location
}
