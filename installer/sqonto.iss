; Compile through scripts/build-installer.ps1 after building the release binaries.
#ifndef AppVersion
  #error AppVersion must be supplied by the build script
#endif

[Setup]
AppId={{01CF2607-5352-4334-AF58-D800028178A7}
AppName=Sqonto
AppVersion={#AppVersion}
AppPublisher=Sqonto
DefaultDirName={localappdata}\Programs\Sqonto
DefaultGroupName=Sqonto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
SetupIconFile=..\backend\assets\icons\sqonto.ico
UninstallDisplayIcon={app}\Sqonto.exe
OutputDir=..\dist\packages
OutputBaseFilename=Sqonto-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
; Explicit allowlist: never ship local accounting data or recursively uninstall it.
Source: "..\dist\Sqonto.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\sqonto-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\THIRD-PARTY-LICENSES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Sqonto"; Filename: "{app}\Sqonto.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Sqonto"; Filename: "{app}\Sqonto.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\Sqonto.exe"; Description: "{cm:LaunchProgram,Sqonto}"; Flags: nowait postinstall skipifsilent
