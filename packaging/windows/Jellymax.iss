[Setup]
AppId={{CC591448-50D1-4F11-B447-808A3028C4A2}
AppName=Jellymax
AppVersion=0.1.3
DefaultDirName={autopf}\Jellymax
DefaultGroupName=Jellymax
OutputDir=..\..\dist
OutputBaseFilename=Jellymax-Windows-x64
SetupIconFile=..\icons\jellymax.ico
UninstallDisplayIcon={app}\jellymax.ico
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Files]
Source: "..\..\stage\windows\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{group}\Open Jellymax"; Filename: "http://localhost:8097"; IconFilename: "{app}\jellymax.ico"
Name: "{commondesktop}\Jellymax"; Filename: "http://localhost:8097"; IconFilename: "{app}\jellymax.ico"

[Run]
Filename: "powershell.exe"; Parameters: "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File ""{app}\install-service.ps1"""; WorkingDir: "{app}"; Flags: runhidden waituntilterminated logoutput
Filename: "http://localhost:8097"; Flags: shellexec postinstall skipifsilent

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File ""{app}\uninstall-service.ps1"""; WorkingDir: "{app}"; Flags: runhidden waituntilterminated logoutput; RunOnceId: "RemoveJellymaxService"
