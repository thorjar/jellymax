$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$data = Join-Path $env:ProgramData "Jellymax"
$logs = Join-Path $data "logs"
$nssm = Join-Path $root "nssm.exe"
New-Item -ItemType Directory -Force -Path $data, $logs | Out-Null

function Invoke-Nssm {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$NssmArgs)
    & $nssm @NssmArgs
    if ($LASTEXITCODE -ne 0) {
        throw "NSSM command failed ($LASTEXITCODE): $($NssmArgs -join ' ')"
    }
}

if (Get-Service -Name Jellymax -ErrorAction SilentlyContinue) {
    Invoke-Nssm stop Jellymax confirm
    Invoke-Nssm remove Jellymax confirm
}
Invoke-Nssm install Jellymax (Join-Path $root "jellymax.exe")
Invoke-Nssm set Jellymax AppParameters "--data-dir `"$data`" serve --bind 127.0.0.1:8097 --web-dir `"$(Join-Path $root 'web')`" --ffmpeg `"$(Join-Path $root 'ffmpeg.exe')`" --ffprobe `"$(Join-Path $root 'ffprobe.exe')`""
Invoke-Nssm set Jellymax AppDirectory $root
Invoke-Nssm set Jellymax ObjectName LocalSystem
Invoke-Nssm set Jellymax Start SERVICE_AUTO_START
Invoke-Nssm set Jellymax AppExit Default Restart
Invoke-Nssm set Jellymax AppStdout (Join-Path $logs "server.log")
Invoke-Nssm set Jellymax AppStderr (Join-Path $logs "server-error.log")
Invoke-Nssm set Jellymax AppRotateFiles 1
Invoke-Nssm set Jellymax AppRotateBytes 10485760
Invoke-Nssm start Jellymax

$healthy = $false
for ($attempt = 0; $attempt -lt 60; $attempt++) {
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:8097/health" -TimeoutSec 2
        if ($response.StatusCode -eq 200) {
            $healthy = $true
            break
        }
    } catch {
        Start-Sleep -Milliseconds 500
    }
}
if (-not $healthy) {
    $status = & $nssm status Jellymax
    $errorLog = Join-Path $logs "server-error.log"
    $details = if (Test-Path $errorLog) { Get-Content $errorLog -Tail 40 | Out-String } else { "No error log was created." }
    throw ("Jellymax service did not become healthy. NSSM status: " + $status + [Environment]::NewLine + $details)
}
