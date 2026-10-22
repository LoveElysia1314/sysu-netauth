; Rust distribution. Build with apps/rust/scripts/build.ps1 from a VS developer shell.
#ifndef MyAppVersion
  #error Pass the release version using /DMyAppVersion=version
#endif
#ifndef MyAppFileVersion
  #error Pass numeric MyAppFileVersion using /DMyAppFileVersion=major.minor.patch
#endif
#ifndef AppSourceDir
  #define AppSourceDir "..\dist\sysu_netauth"
#endif
#ifndef MyAppAbi
  #define MyAppAbi "msvc"
#endif
#define MyAppName "SYSU NetAuth"
#define MyAppId "SYSUNetAuth"
#define MyAppGuid "7E9B2C3A-1D4F-4E8A-9B6C-2F3D0E5A7C1B"
#define NpcapUrl "https://npcap.com/#download"

[Setup]
AppId={#MyAppGuid}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=SYSU NetAuth Contributors
VersionInfoVersion={#MyAppFileVersion}
VersionInfoDescription=SYSU 802.1X/EAPOL 认证客户端（Rust）
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64os
OutputDir=..\dist
OutputBaseFilename=SYSUNetAuth_Rust_{#MyAppAbi}_x64_Setup_v{#MyAppVersion}
Compression=lzma2/max
SolidCompression=yes
SetupIconFile=..\assets\icon-ethernet.ico
UninstallDisplayIcon={app}\sysu_netauth_gui.exe
UninstallLogging=yes
WizardStyle=modern
SetupLogging=yes
CloseApplications=force
CloseApplicationsFilter=*.exe,*.dll,*.pyd
RestartApplications=no
ChangesEnvironment=yes
DisableProgramGroupPage=yes

[Languages]
Name: "chinesesimplified"; MessagesFile: "Languages\ChineseSimplified.isl"

[Files]
Source: "{#AppSourceDir}\sysu_netauth.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\sysu_netauth_service.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\sysu_netauth_gui.exe"; DestDir: "{app}"; Flags: ignoreversion restartreplace

[Dirs]
Name: "{app}"; Permissions: admins-full system-full users-readexec
Name: "{commonappdata}\SYSUNetAuth"; Permissions: admins-full system-full

[Icons]
Name: "{group}\SYSU NetAuth"; Filename: "{app}\sysu_netauth_gui.exe"
Name: "{group}\卸载 SYSU NetAuth"; Filename: "{uninstallexe}"
Name: "{autodesktop}\SYSU NetAuth"; Filename: "{app}\sysu_netauth_gui.exe"

[Run]
Filename: "{app}\sysu_netauth_gui.exe"; Description: "启动 SYSU NetAuth"; Flags: nowait postinstall skipifsilent runascurrentuser
Filename: "{#NpcapUrl}"; Description: "打开 Npcap 官网下载页（网络认证需要安装驱动）"; Flags: shellexec nowait postinstall skipifsilent runasoriginaluser unchecked; Check: NpcapRequired

[Code]
const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#MyAppGuid}_is1';

#include "npcap.iss"
#include "../../../scripts/windows/process.iss"
#include "../../../scripts/windows/service.iss"

var
  ExistingInstallDir: String;

function InitializeSetup: Boolean;
begin
  Result := IsAdminInstallMode;
  if not Result then begin
    SuppressibleMsgBox('安装程序必须以管理员身份运行。', mbCriticalError, MB_OK, IDOK);
    Exit;
  end;
  if not RegQueryStringValue(HKLM64, UninstallKey, 'InstallLocation', ExistingInstallDir) then
    RegQueryStringValue(HKLM32, UninstallKey, 'InstallLocation', ExistingInstallDir);
  if not DirExists(ExistingInstallDir) then ExistingInstallDir := '';
end;

procedure InitializeWizard;
begin
  if ExistingInstallDir <> '' then WizardForm.DirEdit.Text := ExistingInstallDir;
  InitializeNpcapPage;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := SkipNpcapPage(PageID);
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
  if PageID = NpcapPage.ID then RefreshNpcapStatus;
end;

procedure ClearPendingCommands;
begin
  DeleteFile(ExpandConstant('{commonappdata}\SYSUNetAuth\command.json'));
  DelTree(ExpandConstant('{commonappdata}\SYSUNetAuth\commands'), True, True, True);
end;

procedure RemoveLegacyStartup;
begin
  RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#MyAppId}');
  DeleteFile(ExpandConstant('{userstartup}\SYSU NetAuth.lnk'));
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if StopManagedApp(Result) then Result := '';
end;

procedure DeinitializeSetup;
begin
  RestoreManagedStart;
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
    // Retire Python/Qt bundles while retaining ProgramData settings for migration.
    SetupStage('正在清理旧版程序文件…');
    DelTree(ExpandConstant('{app}\_internal'), True, True, True);
    SetupStage('正在更新登录启动设置…');
    RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Change /TN "SYSUNetAuth GUI" /TR "\"' + ExpandConstant('{app}\sysu_netauth_gui.exe') + '\" --startup"', 15, Code);
    RegDeleteValue(HKLM, 'SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers', ExpandConstant('{app}\sysu_netauth.exe'));
    RemoveLegacyStartup;
    SetupStage('正在设置配置目录权限…');
    RunSetupCommand(ExpandConstant('{sys}\icacls.exe'), '"' + ExpandConstant('{commonappdata}\SYSUNetAuth') + '" /inheritance:r /remove:g *S-1-5-32-545 /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F /T', 30, Code);
    ClearPendingCommands;
    SetupStage('正在注册后台认证服务…');
    if not RunSetupCommand(ExpandConstant('{app}\sysu_netauth_service.exe'), '--install', 30, Code) or (Code <> 0) then
      RaiseException('Windows 服务注册失败或超时，请查看安装日志。错误码：' + IntToStr(Code));
    ManagedStartChanged := False; { --install applied the saved service mode. }
    RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'failure SYSUNetAuth reset= 86400 actions= restart/60000', 15, Code);
    // The service must start without Npcap so configuration and setup remain available.
    SetupStage('正在启动后台认证服务…');
    if RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'start SYSUNetAuth', 30, Code) and (Code = 0) then
      Log('Windows 服务已启动。')
    else
      Log('Windows 服务启动失败或已在运行，代码：' + IntToStr(Code));
    SetPath(True);
  end;
end;

procedure DeleteSavedData(Directory: String);
begin
  if DirExists(Directory) and not DelTree(Directory, True, True, True) then
    SuppressibleMsgBox('未能完全删除配置目录：' #13#10 + Directory,
      mbError, MB_OK, IDOK);
end;

procedure CurUninstallStepChanged(Step: TUninstallStep);
var Code: Integer; SharedData, UserData, LegacyData, Error: String;
begin
  if Step = usUninstall then begin
    if not StopManagedApp(Error) or not RemoveManagedService(Error) then begin
      SuppressibleMsgBox('无法停止或移除认证服务，卸载已取消。' #13#10 + Error,
        mbCriticalError, MB_OK, IDOK);
      Abort;
    end;
    RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Delete /TN "SYSUNetAuth GUI" /F', 15, Code);
    RemoveLegacyStartup;
    ClearPendingCommands;
    SetPath(False);
    SharedData := ExpandConstant('{commonappdata}\SYSUNetAuth');
    UserData := ExpandConstant('{userappdata}\SYSUNetAuth');
    LegacyData := ExpandConstant('{userappdata}\CampusNetAuth');
    if (not UninstallSilent) and
      (DirExists(SharedData) or DirExists(UserData) or DirExists(LegacyData)) then
      if SuppressibleMsgBox('是否删除保存的账号、密码和设置（包括旧版配置）？',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES then begin
        DeleteSavedData(SharedData);
        DeleteSavedData(UserData);
        DeleteSavedData(LegacyData);
      end;
    OfferNpcapUninstall;
  end;
end;
