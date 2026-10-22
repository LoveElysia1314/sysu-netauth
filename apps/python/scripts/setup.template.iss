; SYSU NetAuth 安装包脚本 — 模板文件
; 由 build.py 读取后填充 @PLACEHOLDER@ 并生成临时 setup.iss
; 模板本身不含本机路径信息，可安全提交到仓库
;
; ═══════════════════════════════════════════════════════════════
; Npcap 依赖处理说明
; ═══════════════════════════════════════════════════════════════
; Npcap 免费版（Free Edition）禁止重新分发（License:
; https://github.com/nmap/npcap/blob/master/LICENSE），
; 且免费版不支持静默安装（/S 参数为 OEM 专有）。
; 因此安装包不内置也不自动安装 Npcap，改为：
;   1. 安装结束时检测 Npcap 是否已安装
;   2. 若未安装，弹窗提示用户安装
;   3. 程序设置与维护页内置 Npcap 安装指引，首次运行时引导用户安装
; 如需商业重新分发，请购买 Npcap OEM 授权：
;   https://npcap.com/oem/redist.html
; ═══════════════════════════════════════════════════════════════

#define MyAppName "@APP_NAME@"
#define MyAppId "@APP_ID@"
#define MyAppVersion "@APP_VERSION@"
#define MyAppExeName "@APP_EXE_NAME@"
#define MyAppServiceExeName "@APP_SERVICE_EXE_NAME@"
#define MyAppIconName "@APP_ID@.ico"
#define MyAppUserModelId "@APP_ID@.Application"
#define MyServiceName "@APP_ID@"

#define NpcapUrl "https://npcap.com/#download"

[Setup]
AppId=7E9B2C3A-1D4F-4E8A-9B6C-2F3D0E5A7C1B
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=SYSU NetAuth Contributors
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
VersionInfoVersion=@APP_FILE_VERSION@
VersionInfoDescription=SYSU 802.1X/EAPOL 认证客户端
OutputBaseFilename=@APP_ID@_Setup_v@APP_VERSION@
Compression=lzma2/max
SolidCompression=yes
; 安装程序始终请求管理员权限；不允许降级为当前用户安装。
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=
ArchitecturesInstallIn64BitMode=x64os
ArchitecturesAllowed=x64compatible
WizardStyle=modern
SetupLogging=yes
UninstallLogging=yes
SetupIconFile=..\sysu_netauth\assets\icon-ethernet.ico
ChangesEnvironment=yes
UninstallDisplayIcon={app}\{#MyAppIconName}
; 手动停止已知进程，并启用 Restart Manager 兜底处理跨会话进程和短暂文件占用。
CloseApplications=force
CloseApplicationsFilter=*.exe,*.dll,*.pyd
RestartApplications=no
DisableProgramGroupPage=yes

[Languages]
Name: "chinesesimplified"; MessagesFile: "Languages/ChineseSimplified.isl"

[Files]
; 主 GUI 若仍被短暂占用，允许 Windows 在重启时完成替换，避免 MoveFile code 5
; 直接中断安装。服务 EXE 仍要求安装前完全停止，不使用延迟替换。
Source: "@APP_DIR_RELATIVE@\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion restartreplace
Source: "@APP_DIR_RELATIVE@\*"; DestDir: "{app}"; Excludes: "{#MyAppExeName}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\sysu_netauth\assets\icon-ethernet.ico"; DestDir: "{app}"; DestName: "{#MyAppIconName}"; Flags: ignoreversion

[Dirs]
Name: "{app}"; Permissions: admins-full system-full users-readexec
Name: "{commonappdata}\{#MyAppId}"; Permissions: admins-full system-full

[Registry]
; 除 EXE 自带 requireAdministrator 清单外，再设置 Windows 兼容层的
; “以管理员身份运行此程序”，兼容旧快捷方式及升级残留。
Root: HKLM; Subkey: "SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers"; ValueType: string; ValueName: "{app}\{#MyAppExeName}"; ValueData: "~ RUNASADMIN"; Flags: uninsdeletevalue

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppIconName}"; AppUserModelID: "{#MyAppUserModelId}"
Name: "{group}\卸载 {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppIconName}"; AppUserModelID: "{#MyAppUserModelId}"

[Run]
; postinstall 默认使用提权前的用户令牌；显式 runascurrentuser 才会继承
; Setup 的管理员令牌，从而有权访问仅管理员可读的 ProgramData 配置。
Filename: "{app}\{#MyAppExeName}"; Description: "启动 {#MyAppName}"; Flags: nowait postinstall skipifsilent runascurrentuser

[Code]

const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';
  NpcapUrlConst = '{#NpcapUrl}';
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\7E9B2C3A-1D4F-4E8A-9B6C-2F3D0E5A7C1B_is1';

#include "../../../scripts/windows/process.iss"
#include "../../../scripts/windows/service.iss"

var
  ExistingInstallDirExists: Boolean;
  ExistingInstallDir: string;

// ── 初始化：检测旧版本状态、运行实例 ──

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  // 在 Inno 检查占用文件之前先停止服务和已知 GUI；随后 Restart Manager
  // 再扫描 [Files] 的实际目标，覆盖跨会话或短暂占用场景。
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

function InitializeSetup: Boolean;
begin
  if not IsAdminInstallMode then
  begin
    SuppressibleMsgBox(
      '安装程序必须以管理员身份运行。',
      mbCriticalError,
      MB_OK,
      IDOK
    );
    Result := False;
    Exit;
  end;

  if not RegQueryStringValue(HKLM64, UninstallKey, 'InstallLocation', ExistingInstallDir) then
    RegQueryStringValue(HKLM32, UninstallKey, 'InstallLocation', ExistingInstallDir);
  ExistingInstallDirExists := DirExists(ExistingInstallDir);
  if ExistingInstallDirExists then
    Log('检测到现有安装目录，将锁定安装路径: ' + ExistingInstallDir);

  Result := True;
end;

procedure InitializeWizard;
begin
  if ExistingInstallDirExists then WizardForm.DirEdit.Text := ExistingInstallDir;
end;

// ── 升级安装时锁定安装目录 ──

procedure CurPageChanged(CurPage: Integer);
begin
  if CurPage = wpSelectDir then
  begin
    if ExistingInstallDirExists then
    begin
      WizardForm.DirEdit.Text := ExistingInstallDir;
      WizardForm.DirEdit.Enabled := False;
      WizardForm.DirBrowseButton.Enabled := False;
      WizardForm.SelectDirBrowseLabel.Caption :=
        '检测到已安装版本。为保证重装/升级一致性，安装路径将沿用现有位置。';
    end
    else
    begin
      WizardForm.DirEdit.Enabled := True;
      WizardForm.DirBrowseButton.Enabled := True;
    end;
  end;
end;

// ── Npcap 检测 ──

function IsNpcapInstalled: Boolean;
begin
  Result := RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\npcap') or
            RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\npcap_flt');
  if Result then
    Log('Npcap 驱动已安装');
end;

// ── PATH 管理 ──

procedure AddToPath(InstallDir: string);
var
  PathStr: string;
begin
  if not RegQueryStringValue(HKLM, EnvironmentKey, 'Path', PathStr) then
    PathStr := '';
  if Pos(LowerCase(InstallDir), LowerCase(PathStr)) = 0 then
  begin
    PathStr := PathStr + ';' + InstallDir;
    if RegWriteExpandStringValue(HKLM, EnvironmentKey, 'Path', PathStr) then
      Log('PATH 已添加: ' + InstallDir)
    else
      Log('PATH 写入失败');
  end;
end;

procedure RemoveFromPath(InstallDir: string);
var
  PathStr: string;
  P: Integer;
begin
  if RegQueryStringValue(HKLM, EnvironmentKey, 'Path', PathStr) then
  begin
    P := Pos(';' + LowerCase(InstallDir), LowerCase(PathStr));
    if P > 0 then
      Delete(PathStr, P, Length(InstallDir) + 1)
    else
    begin
      P := Pos(LowerCase(InstallDir) + ';', LowerCase(PathStr));
      if P > 0 then
        Delete(PathStr, P, Length(InstallDir) + 1)
      else
      begin
        P := Pos(LowerCase(InstallDir), LowerCase(PathStr));
        if P > 0 then
          Delete(PathStr, P, Length(InstallDir));
      end;
    end;
    RegWriteExpandStringValue(HKLM, EnvironmentKey, 'Path', PathStr);
  end;
end;

// ── 安装后处理 ──

procedure InstallAuthService;
var
  ServiceExePath: string;
  ResultCode: Integer;
begin
  ServiceExePath := ExpandConstant('{app}\{#MyAppServiceExeName}');
  // Update in place; deleting/recreating a service races SCM's pending deletion.
  if not RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'create "{#MyServiceName}" binPath= "\"' + ServiceExePath + '\"" start= auto depend= npcap DisplayName= "{#MyAppName}"',
    15, ResultCode) then RaiseException('Windows 服务注册超时：' + IntToStr(ResultCode));
  if ResultCode = 1073 then begin
    if not RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'config "{#MyServiceName}" binPath= "\"' + ServiceExePath + '\"" start= auto depend= npcap',
      15, ResultCode) then RaiseException('Windows 服务更新超时：' + IntToStr(ResultCode));
  end;
  if ResultCode <> 0 then RaiseException('Windows 服务注册失败：' + IntToStr(ResultCode));
  ManagedStartChanged := False;
  RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'description "{#MyServiceName}" "SYSU wired campus network 802.1X authentication service"',
    15, ResultCode);
  RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'failure "{#MyServiceName}" reset= 86400 actions= restart/60000/none/0/none/0',
    15, ResultCode);
  if RunSetupCommand(ExpandConstant('{sys}\sc.exe'), 'start "{#MyServiceName}"', 30, ResultCode) and (ResultCode = 0) then
    Log('Windows 服务已启动')
  else
    Log('Windows 服务启动失败或已在运行（错误码: ' + IntToStr(ResultCode) + '）');
end;

procedure ClearPendingCommands;
begin
  DeleteFile(ExpandConstant('{commonappdata}\{#MyAppId}\command.json'));
  DelTree(
    ExpandConstant('{commonappdata}\{#MyAppId}\commands'),
    True,
    True,
    True
  );
end;

procedure HardenDataDirectory;
var
  ResultCode: Integer;
  DataDir: string;
begin
  DataDir := ExpandConstant('{commonappdata}\{#MyAppId}');
  // 移除旧版本遗留的 Users 权限，仅保留 SYSTEM 与管理员完全控制。
  RunSetupCommand(ExpandConstant('{sys}\icacls.exe'),
    '"' + DataDir + '" /inheritance:r /remove:g *S-1-5-32-545 /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F',
    30, ResultCode);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  NpcapMsg: string;
  Code: Integer;
begin
  if CurStep = ssPostInstall then
  begin
    // Switching from Rust keeps the same logon task and removes its old GUI.
    DeleteFile(ExpandConstant('{app}\sysu_netauth_gui.exe'));
    RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Change /TN "SYSUNetAuth GUI" /TR "\"' + ExpandConstant('{app}\{#MyAppExeName}') + '\" --startup"', 15, Code);
    AddToPath(ExpandConstant('{app}'));

    HardenDataDirectory;
    ClearPendingCommands;
    InstallAuthService;

    // Npcap 未安装 → 弹窗提示用户
    if (not WizardSilent) and (not IsNpcapInstalled) then
    begin
      NpcapMsg :=
        '检测到系统中未安装 Npcap 网络驱动。' #13#13
        '{#MyAppName} 需要 Npcap 才能进行 802.1X/EAPOL 认证。' #13#13
        '首次运行程序时会自动引导您下载并安装 Npcap。' #13
        '您也可以立即从官网下载安装：' #13 +
        '  ' + NpcapUrlConst + #13#13 +
        '安装提示：启动安装程序后保持默认选项，' #13 +
        '一路点击 Next/下一步即可，无需修改任何选项。';
      SuppressibleMsgBox(NpcapMsg, mbInformation, MB_OK, IDOK);
    end;
  end;
end;

// ── 卸载时清理 ──

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  appDataPath: string;
  userDataPath: string;
  legacyUserDataPath: string;
  Error: string;
  choice: Integer;
begin
  case CurUninstallStep of
    usUninstall:
      begin
        // Stop and verify before Inno attempts to remove any executable or DLL.
        if not StopManagedApp(Error) or not RemoveManagedService(Error) then begin
          SuppressibleMsgBox('无法停止或移除认证服务，卸载已取消。' #13#10 + Error,
            mbCriticalError, MB_OK, IDOK);
          Abort;
        end;
      end;
    usPostUninstall:
      begin
        // 清理系统 PATH
        RemoveFromPath(ExpandConstant('{app}'));

        // 清理旧命令
        ClearPendingCommands;

        // 清理可能的开机自启残留
        RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#MyAppId}');
        DeleteFile(ExpandConstant('{userstartup}\{#MyAppName}.lnk'));
        RunSetupCommand(ExpandConstant('{sys}\schtasks.exe'), '/Delete /TN "{#MyAppId} GUI" /F', 15, choice);

        // 同时兼容旧版 config.json.tmp 与新版 config.json.<随机值>.tmp。
        RunSetupCommand(ExpandConstant('{sys}\cmd.exe'), '/d /c del /q "' + ExpandConstant('{commonappdata}\{#MyAppId}') + '\*.json.tmp" "' + ExpandConstant('{commonappdata}\{#MyAppId}') + '\*.json.*.tmp" 2>nul', 10, choice);
        RunSetupCommand(ExpandConstant('{sys}\cmd.exe'), '/d /c del /q "' + ExpandConstant('{userappdata}\{#MyAppId}') + '\*.json.tmp" "' + ExpandConstant('{userappdata}\{#MyAppId}') + '\*.json.*.tmp" 2>nul', 10, choice);
        RunSetupCommand(ExpandConstant('{sys}\cmd.exe'), '/d /c del /q "' + ExpandConstant('{userappdata}\CampusNetAuth') + '\*.json.tmp" "' + ExpandConstant('{userappdata}\CampusNetAuth') + '\*.json.*.tmp" 2>nul', 10, choice);

        // 询问删除用户配置数据
        appDataPath := ExpandConstant('{userappdata}');
        userDataPath := appDataPath + '\{#MyAppId}';
        legacyUserDataPath := appDataPath + '\CampusNetAuth';
        if (not UninstallSilent) and (DirExists(userDataPath) or DirExists(legacyUserDataPath)) then
        begin
          choice := SuppressibleMsgBox(
            '是否删除用户配置数据（NetID/密码/认证设置）？' #13#13 '路径: ' + userDataPath,
            mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO);
          if choice = IDYES then
          begin
            if DirExists(userDataPath) and not DelTree(userDataPath, True, True, True) then
              SuppressibleMsgBox('无法完全删除用户数据，部分文件可能被占用。', mbError, MB_OK, IDOK);
            if DirExists(legacyUserDataPath) then
              DelTree(legacyUserDataPath, True, True, True);
          end;
        end;

        userDataPath := ExpandConstant('{commonappdata}\{#MyAppId}');
        if (not UninstallSilent) and DirExists(userDataPath) then
        begin
          choice := SuppressibleMsgBox(
            '是否删除共享配置数据（NetID/密码/服务状态）？' #13#13 '路径: ' + userDataPath,
            mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO);
          if choice = IDYES then
          begin
            if not DelTree(userDataPath, True, True, True) then
              SuppressibleMsgBox('无法完全删除共享配置数据，部分文件可能被占用。', mbError, MB_OK, IDOK);
          end;
        end;

        // 询问卸载 Npcap — 启动 Npcap 卸载向导
        if (not UninstallSilent) and IsNpcapInstalled then
        begin
          choice := SuppressibleMsgBox(
            '是否同时卸载 Npcap 网络驱动？' #13#13
            '注意：其他网络工具（如 Wireshark、Nmap）也可能依赖 Npcap。',
            mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO);
          if choice = IDYES then
          begin
            // 启动 Npcap 卸载程序（如存在）
            if FileExists('C:\Program Files\Npcap\uninstall.exe') then
            begin
              if Exec('C:\Program Files\Npcap\uninstall.exe', '',
                '', SW_SHOW, ewWaitUntilTerminated, choice) then
                Log('Npcap 卸载向导已启动');
            end
            else
            begin
              SuppressibleMsgBox(
                '请通过系统「设置 → 应用 → 已安装的应用」手动搜索并卸载 Npcap。',
                mbInformation, MB_OK, IDOK);
            end;
          end;
        end;
      end;
  end;
end;
