; Instalador do hud-monitor (Inno Setup 6). Gere com scripts/build-installer.ps1.
; Por usuário, sem admin: instala em %LOCALAPPDATA%\hud-monitor (mesmo lugar da instalação manual).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F3B2C1E-8A4D-4F7B-9C2E-1D5A7B3E9F10}
AppName=HUD Monitor
AppVersion={#AppVersion}
AppPublisher=douglasmoura-dev
AppPublisherURL=https://github.com/douglasmoura-dev/hud-monitor
DefaultDirName={localappdata}\hud-monitor
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\target\installer
OutputBaseFilename=hud-monitor-setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
UninstallDisplayIcon={app}\hud-monitor.exe
UninstallDisplayName=HUD Monitor
LicenseFile=..\LICENSE
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern

[Languages]
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na área de trabalho"; Flags: unchecked

[Files]
Source: "..\target\release\hud-app.exe"; DestDir: "{app}"; DestName: "hud-monitor.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\HUD Monitor"; Filename: "{app}\hud-monitor.exe"
Name: "{userdesktop}\HUD Monitor"; Filename: "{app}\hud-monitor.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\hud-monitor.exe"; Description: "Abrir o HUD Monitor agora"; Flags: nowait postinstall skipifsilent

[Registry]
; O próprio app liga/desliga o início com o Windows; o desinstalador só apaga o valor se sobrar.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "hud-monitor"; ValueType: none; Flags: uninsdeletevalue dontcreatekey

[Code]
// ponytail: fecha pelo nome do exe; o nome é exclusivo deste app.
procedure CloseApp();
var Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM hud-monitor.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseApp();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  CloseApp();
  Result := True;
end;
