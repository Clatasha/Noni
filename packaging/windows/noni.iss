; Noni's Windows installer (Inno Setup 6). For one user, no administrator
; needed: it goes to %LOCALAPPDATA%\Programs\Noni, with a shortcut in the
; Start menu, one on the desktop if wanted, and Noni's project files opening
; in it. Uninstalling leaves the user's settings and projects where they are.
;
; Built by .github/workflows/package.yml:
;   ISCC /DAppVersion=0.2.0 /DArch=x64 /DFiles=<folder of the zip's files>
;        /O<output folder> /F<file name> packaging\windows\noni.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef Files
  #error Say where the files are: /DFiles=<folder>
#endif

[Setup]
; The installation's identity: updates find it by this, so it never changes.
AppId={{F610FB82-9716-47A7-BD5F-0278333AB873}
AppName=Noni Video Editor
AppVersion={#AppVersion}
AppVerName=Noni Video Editor {#AppVersion}
AppPublisher=Clatasha
AppPublisherURL=https://github.com/Clatasha/Noni
AppUpdatesURL=https://github.com/Clatasha/Noni/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Noni
DefaultGroupName=Noni
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
MinVersion=10.0
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
SetupIconFile=..\icons\noni.ico
UninstallDisplayIcon={app}\noni.ico
UninstallDisplayName=Noni Video Editor
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[CustomMessages]
english.OpenProjects=Open Noni projects (.noniproj) with Noni
brazilianportuguese.OpenProjects=Abrir projetos do Noni (.noniproj) com o Noni

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "projects"; Description: "{cm:OpenProjects}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Files}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs
Source: "..\icons\noni.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Noni"; Filename: "{app}\Noni.exe"; IconFilename: "{app}\noni.ico"
Name: "{autodesktop}\Noni"; Filename: "{app}\Noni.exe"; IconFilename: "{app}\noni.ico"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.noniproj"; ValueType: string; ValueName: ""; ValueData: "Noni.Project"; Flags: uninsdeletevalue; Tasks: projects
Root: HKA; Subkey: "Software\Classes\Noni.Project"; ValueType: string; ValueName: ""; ValueData: "Noni Project"; Flags: uninsdeletekey; Tasks: projects
Root: HKA; Subkey: "Software\Classes\Noni.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\noni.ico"; Tasks: projects
Root: HKA; Subkey: "Software\Classes\Noni.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\Noni.exe"" ""%1"""; Tasks: projects

[Run]
Filename: "{app}\Noni.exe"; Description: "{cm:LaunchProgram,Noni}"; Flags: nowait postinstall skipifsilent
