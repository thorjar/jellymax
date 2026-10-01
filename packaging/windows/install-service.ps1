$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$data = Join-Path $env:ProgramData "Jellymax"
New-Item -ItemType Directory -Force -Path $data | Out-Null
& "$root\nssm.exe" stop Jellymax confirm 2>$null
& "$root\nssm.exe" remove Jellymax confirm 2>$null
& "$root\nssm.exe" install Jellymax "$root\jellymax.exe"
& "$root\nssm.exe" set Jellymax AppParameters "--data-dir `"$data`" serve --bind 127.0.0.1:8097 --web-dir `"$root\web`" --ffmpeg `"$root\ffmpeg.exe`" --ffprobe `"$root\ffprobe.exe`""
& "$root\nssm.exe" set Jellymax AppDirectory $root
& "$root\nssm.exe" set Jellymax Start SERVICE_AUTO_START
& "$root\nssm.exe" set Jellymax AppExit Default Restart
& "$root\nssm.exe" start Jellymax
