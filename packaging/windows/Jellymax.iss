[Setup]
AppId={{CC591448-50D1-4F11-B447-808A3028C4A2}
AppName=Jellymax
AppVersion=0.1.0
DefaultDirName={autopf}\Jellymax
DefaultGroupName=Jellymax
OutputDir=..\..\dist
OutputBaseFilename=Jellymax-Windows-x64
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Files]
Source: "..\..\stage\windows\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{group}\Open Jellymax"; Filename: "http://localhost:8097"
Name: "{commondesktop}\Jellymax"; Filename: "http://localhost:8097"

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command ""& '{app}\install-service.ps1'"""; Flags: runhidden waituntilterminated logoutput
Filename: "http://localhost:8097"; Flags: shellexec postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command ""& '{app}\uninstall-service.ps1'"""; Flags: runhidden waituntilterminated logoutput; RunOnceId: "RemoveJellymaxService"
