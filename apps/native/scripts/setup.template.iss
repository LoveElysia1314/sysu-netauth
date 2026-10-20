; Native Rust distribution. Build with apps/native/scripts/build.ps1 from a VS developer shell.
#define MyAppVersion "0.7.0"
#define MyAppName "SYSU NetAuth"
#define MyAppId "SYSUNetAuth"

[Setup]
AppId=7E9B2C3A-1D4F-4E8A-9B6C-2F3D0E5A7C1B
AppName={#MyAppName}
AppVersion={#MyAppVersion}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64os
OutputDir=..\dist
OutputBaseFilename=SYSUNetAuth_Setup_v{#MyAppVersion}
Compression=lzma2/max
SolidCompression=yes
SetupIconFile=..\assets\icon-ethernet.ico
UninstallDisplayIcon={app}\sysu_netauth_gui.exe
WizardStyle=modern
CloseApplications=force
CloseApplicationsFilter=*.exe,*.dll,*.pyd
ChangesEnvironment=yes
DisableProgramGroupPage=yes

[Languages]
Name: "chinesesimplified"; MessagesFile: "Languages\ChineseSimplified.isl"

[Files]
Source: "..\dist\sysu_netauth\sysu_netauth.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\sysu_netauth\sysu_netauth_service.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\sysu_netauth\sysu_netauth_gui.exe"; DestDir: "{app}"; Flags: ignoreversion restartreplace

[Dirs]
Name: "{app}"; Permissions: admins-full system-full users-readexec
Name: "{commonappdata}\SYSUNetAuth"; Permissions: admins-full system-full

[Icons]
Name: "{group}\SYSU NetAuth"; Filename: "{app}\sysu_netauth_gui.exe"
Name: "{autodesktop}\SYSU NetAuth"; Filename: "{app}\sysu_netauth_gui.exe"

[Run]
Filename: "{app}\sysu_netauth_gui.exe"; Description: "启动 SYSU NetAuth"; Flags: nowait postinstall skipifsilent runascurrentuser

[Code]
const EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

procedure StopProcesses;
var Code, Remaining: Integer;
begin
  Exec('sc.exe', 'stop SYSUNetAuth', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Remaining := 10;
  while Remaining > 0 do begin
    if not Exec('cmd.exe', '/c tasklist /fi "imagename eq sysu_netauth_service.exe" /nh | findstr /i sysu_netauth_service.exe', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then Break;
    Sleep(1000);
    Dec(Remaining);
  end;
  Exec('taskkill.exe', '/f /im sysu_netauth_service.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec('taskkill.exe', '/f /im sysu_netauth_gui.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec('taskkill.exe', '/f /im sysu_netauth.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopProcesses;
  Result := '';
end;

procedure SetPath(Add: Boolean);
var Value, AppPath: String; Position: Integer;
begin
  RegQueryStringValue(HKLM, EnvironmentKey, 'Path', Value);
  AppPath := ExpandConstant('{app}');
  Position := Pos(';' + Lowercase(AppPath) + ';', ';' + Lowercase(Value) + ';');
  if Add and (Position = 0) then Value := Value + ';' + AppPath;
  if not Add and (Position > 0) then begin
    Value := ';' + Value + ';';
    Delete(Value, Position, Length(AppPath) + 1);
    if (Length(Value) > 0) and (Value[1] = ';') then Delete(Value, 1, 1);
    if (Length(Value) > 0) and (Value[Length(Value)] = ';') then Delete(Value, Length(Value), 1);
  end;
  RegWriteExpandStringValue(HKLM, EnvironmentKey, 'Path', Value);
end;

procedure CurStepChanged(Step: TSetupStep);
var Code: Integer;
begin
  if Step = ssPostInstall then begin
    // Retire Python/Qt bundles while retaining ProgramData settings for migration.
    DelTree(ExpandConstant('{app}\_internal'), True, True, True);
    Exec('schtasks.exe', '/Change /TN "SYSUNetAuth GUI" /TR "\"' + ExpandConstant('{app}\sysu_netauth_gui.exe') + '\" --startup"', '', SW_HIDE, ewWaitUntilTerminated, Code);
    RegDeleteValue(HKLM, 'SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers', ExpandConstant('{app}\sysu_netauth.exe'));
    DeleteFile(ExpandConstant('{userstartup}\SYSU NetAuth.lnk'));
    Exec('icacls.exe', '"' + ExpandConstant('{commonappdata}\SYSUNetAuth') + '" /inheritance:r /remove:g *S-1-5-32-545 /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F /T', '', SW_HIDE, ewWaitUntilTerminated, Code);
    if not Exec(ExpandConstant('{app}\sysu_netauth_service.exe'), '--install', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then
      RaiseException('Windows 服务注册失败。');
    Exec('sc.exe', 'failure SYSUNetAuth reset= 86400 actions= restart/60000', '', SW_HIDE, ewWaitUntilTerminated, Code);
    // The service must start without Npcap so configuration and setup remain available.
    Exec('sc.exe', 'start SYSUNetAuth', '', SW_HIDE, ewWaitUntilTerminated, Code);
    SetPath(True);
  end;
end;

procedure CurUninstallStepChanged(Step: TUninstallStep);
var Code: Integer;
begin
  if Step = usUninstall then begin
    StopProcesses;
    Exec('sc.exe', 'delete SYSUNetAuth', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Exec('schtasks.exe', '/Delete /TN "SYSUNetAuth GUI" /F', '', SW_HIDE, ewWaitUntilTerminated, Code);
    SetPath(False);
    if SuppressibleMsgBox('是否删除保存的账号、密码和设置？', mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES then
      DelTree(ExpandConstant('{commonappdata}\SYSUNetAuth'), True, True, True);
  end;
end;
