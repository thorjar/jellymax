$root = Split-Path -Parent $MyInvocation.MyCommand.Path
& "$root\nssm.exe" stop Jellymax confirm 2>$null
& "$root\nssm.exe" remove Jellymax confirm 2>$null
exit 0
