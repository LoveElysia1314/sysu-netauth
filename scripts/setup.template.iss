; Build with scripts/build.ps1 from a VS developer shell.
#ifndef MyAppVersion
  #error Pass the release version using /DMyAppVersion=version
#endif
#ifndef MyAppFileVersion
  #error Pass numeric MyAppFileVersion using /DMyAppFileVersion=major.minor.patch
#endif
#ifndef AppSourceDir
  #define AppSourceDir "..\dist\sysu_netauth"
#endif
#ifndef MyAppArch
  #define MyAppArch "x64"
#endif
#if MyAppArch != "x64" && MyAppArch != "arm64"
  #error Unsupported MyAppArch
#endif
#define MyAppName "SYSU NetAuth"
#define MyAppGuid "7E9B2C3A-1D4F-4E8A-9B6C-2F3D0E5A7C1B"
#define MyAppId "SYSUNetAuth"

[Setup]
AppId={#MyAppGuid}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=SYSU NetAuth Contributors
VersionInfoVersion={#MyAppFileVersion}
VersionInfoDescription=SYSU 802.1X/EAPOL 认证客户端
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=
#if MyAppArch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
#endif
OutputDir=..\dist
OutputBaseFilename=SYSUNetAuth_windows_{#MyAppArch}_Setup_v{#MyAppVersion}
Compression=lzma2/max
SolidCompression=yes
SetupIconFile=..\assets\icons\generated\windows\app.ico
UninstallDisplayIcon={app}\sysu_netauth_gui.exe
UninstallLogging=yes
WizardStyle=modern
SetupLogging=yes
CloseApplications=force
CloseApplicationsFilter=*.exe,*.dll
RestartApplications=no
ChangesEnvironment=yes
DisableProgramGroupPage=yes

[Languages]
Name: "chinesesimplified"; MessagesFile: "Languages\ChineseSimplified.isl"

[Files]
Source: "{#AppSourceDir}\sysu_netauth.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\sysu_netauth_service.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\sysu_netauth_gui.exe"; DestDir: "{app}"; Flags: ignoreversion restartreplace
Source: "{#AppSourceDir}\sysu_netauth_notify.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\docs\WINDOWS.md"; DestDir: "{app}"; Flags: ignoreversion

[Dirs]
Name: "{app}"; Permissions: admins-full system-full users-readexec
Name: "{commonappdata}\SYSUNetAuth"; Permissions: admins-full system-full

[Icons]
Name: "{group}\SYSU NetAuth"; Filename: "{app}\sysu_netauth_gui.exe"
Name: "{group}\卸载 SYSU NetAuth"; Filename: "{uninstallexe}"
Name: "{autodesktop}\SYSU NetAuth"; Filename: "{app}\sysu_netauth_gui.exe"

[Run]
Filename: "{app}\sysu_netauth_gui.exe"; Description: "启动 SYSU NetAuth"; Flags: nowait postinstall skipifsilent runascurrentuser
Filename: "ms-settings:appsfeatures"; Description: "打开应用列表，检查是否卸载旧 Npcap（其他软件可能仍需要）"; Flags: shellexec postinstall unchecked skipifsilent; Check: OfferLegacyDriverCleanup

[Code]
const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#MyAppGuid}_is1';

#include "windows/process.iss"
#include "windows/service.iss"
#include "windows/data.iss"
#include "windows/legacy-driver.iss"

var
  ExistingInstallDir: String;
  LegacyDriverDetected: Boolean;

function OfferLegacyDriverCleanup: Boolean;
begin
  Result := LegacyDriverDetected;
end;

function InitializeSetup: Boolean;
var PreviousVersion: String;
begin
  Result := IsAdminInstallMode;
  if not Result then begin
    SuppressibleMsgBox('安装程序必须以管理员身份运行。', mbCriticalError, MB_OK, IDOK);
    Exit;
  end;
  if not RegQueryStringValue(HKLM64, UninstallKey, 'InstallLocation', ExistingInstallDir) then
    RegQueryStringValue(HKLM32, UninstallKey, 'InstallLocation', ExistingInstallDir);
  if not DirExists(ExistingInstallDir) then ExistingInstallDir := '';
  if not RegQueryStringValue(HKLM64, UninstallKey, 'DisplayVersion', PreviousVersion) then
    RegQueryStringValue(HKLM32, UninstallKey, 'DisplayVersion', PreviousVersion);
  LegacyDriverDetected := (ExistingInstallDir <> '') and LegacyDriverCleanup(PreviousVersion,
    RegKeyExists(HKLM64, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\NpcapInst') or
    RegKeyExists(HKLM32, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\NpcapInst'));
end;

procedure InitializeWizard;
begin
  if ExistingInstallDir <> '' then WizardForm.DirEdit.Text := ExistingInstallDir;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := False;
end;

procedure CurPageChanged(PageID: Integer);
begin
  if (PageID = wpSelectDir) and (ExistingInstallDir <> '') then begin
    WizardForm.DirEdit.Text := ExistingInstallDir;
    WizardForm.DirEdit.Enabled := False;
    WizardForm.DirBrowseButton.Enabled := False;
    WizardForm.SelectDirBrowseLabel.Caption :=
      '检测到已安装版本。重装或升级将沿用现有目录。';
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if StopManagedApp(Result) then Result := '';
end;

procedure DeinitializeSetup;
var Code: Integer;
begin
  RestoreManagedStart;
  if FileExists(ExpandConstant('{app}\sysu_netauth_notify.exe')) then
    RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Run /TN "SYSUNetAuth Notifications"', 15, Code);
end;

procedure DeinitializeUninstall;
begin
  RestoreManagedStart;
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

procedure SetupStage(Message: String);
begin
  Log(Message);
  WizardForm.StatusLabel.Caption := Message;
end;

procedure CurStepChanged(Step: TSetupStep);
var Code: Integer;
begin
  if Step = ssPostInstall then begin
    SetupStage('正在更新登录启动设置…');
    RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Change /TN "SYSUNetAuth GUI" /TR "\"' + ExpandConstant('{app}\sysu_netauth_gui.exe') + '\" --startup"', 15, Code);
    SetupStage('正在设置配置目录权限…');
    SecureDataDirectory(ExpandConstant('{commonappdata}\SYSUNetAuth'));
    SetupStage('正在注册后台认证服务…');
    if not RunSetupCommand(ExpandConstant('{app}\sysu_netauth_service.exe'), '--install', 30, Code) or (Code <> 0) then
      RaiseException('Windows 服务注册失败或超时，请查看安装日志。错误码：' + IntToStr(Code));
    ManagedStartChanged := False; { --install applied the saved service mode. }
    RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'failure SYSUNetAuth reset= 86400 actions= restart/60000', 15, Code);
    // The service starts independently of the GUI.
    SetupStage('正在启动后台认证服务…');
    if RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'start SYSUNetAuth', 30, Code) and (Code = 0) then
      Log('Windows 服务已启动。')
    else
      Log('Windows 服务启动失败或已在运行，代码：' + IntToStr(Code));
    SetPath(True);
    SetupStage('正在注册独立通知助手…');
    if not RunSetupCommand(ExpandConstant('{app}\sysu_netauth_notify.exe'), '--install', 30, Code) or (Code <> 0) then
      RaiseException('通知助手注册失败或超时，错误码：' + IntToStr(Code));
  end;
end;

procedure DeleteSavedData(Directory: String);
begin
  if DirExists(Directory) and not DelTree(Directory, True, True, True) then
    SuppressibleMsgBox('未能完全删除配置目录：' #13#10 + Directory,
      mbError, MB_OK, IDOK);
end;

procedure CurUninstallStepChanged(Step: TUninstallStep);
var Code: Integer; SharedData, Error: String;
begin
  if Step = usUninstall then begin
    if not StopManagedApp(Error) or not RemoveManagedService(Error) then begin
      SuppressibleMsgBox('无法停止或移除认证服务，卸载已取消。' #13#10 + Error,
        mbCriticalError, MB_OK, IDOK);
      Abort;
    end;
    RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Delete /TN "SYSUNetAuth GUI" /F', 15, Code);
    RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Delete /TN "SYSUNetAuth Notifications" /F', 15, Code);
    SetPath(False);
    SharedData := ExpandConstant('{commonappdata}\SYSUNetAuth');
    if (not UninstallSilent) and
      DirExists(SharedData) then
      if SuppressibleMsgBox('是否删除保存的账号、密码和设置？',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES then begin
        DeleteSavedData(SharedData);
      end;
  end;
end;
