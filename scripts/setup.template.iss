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
VersionInfoVersion=@APP_VERSION@
VersionInfoDescription=SYSU 802.1X/EAPOL 认证客户端
OutputBaseFilename=@APP_ID@_Setup_v@APP_VERSION@
Compression=lzma2/max
SolidCompression=yes
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64os
ArchitecturesAllowed=x64compatible
WizardStyle=modern
SetupLogging=yes
SetupIconFile=..\sysu_netauth\assets\icon-ethernet.ico
ChangesEnvironment=yes
UninstallDisplayIcon={app}\{#MyAppIconName}
; 本项目自行静默停止服务和 GUI，避免 Inno Restart Manager 弹出占用文件询问页。
CloseApplications=no
RestartApplications=no
DisableProgramGroupPage=yes

[Languages]
Name: "chinesesimplified"; MessagesFile: "Languages/ChineseSimplified.isl"

[Files]
Source: "@APP_DIR_RELATIVE@\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\sysu_netauth\assets\icon-ethernet.ico"; DestDir: "{app}"; DestName: "{#MyAppIconName}"; Flags: ignoreversion

[Dirs]
Name: "{commonappdata}\{#MyAppId}"; Permissions: users-modify

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppIconName}"; AppUserModelID: "{#MyAppUserModelId}"
Name: "{group}\卸载 {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppIconName}"; AppUserModelID: "{#MyAppUserModelId}"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "启动 {#MyAppName}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[Code]

const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';
  NpcapUrlConst = '{#NpcapUrl}';

var
  ExistingInstallDirExists: Boolean;
  ExistingInstallDir: string;

// ── 初始化：检测旧版本状态、运行实例 ──

// 通过 tasklist 检测程序是否正在运行（跨会话安全）
function IsAppRunning: Boolean;
var
  ResultCode: Integer;
begin
  Result :=
    Exec(
      'cmd.exe',
      '/c tasklist /fi "imagename eq {#MyAppExeName}" /nh | findstr /i "{#MyAppExeName}"',
      '',
      SW_HIDE,
      ewWaitUntilTerminated,
      ResultCode
    ) and (ResultCode = 0);
end;

procedure StopManagedProcesses;
var
  ResultCode: Integer;
  MaxWait: Integer;
begin
  // 原地升级前先停止本项目托管的运行实例；配置仍保留在 ProgramData。
  Exec('sc.exe', 'stop "{#MyServiceName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  // 轮询等待服务进程优雅退出，最多等 10 秒；超时后 force-kill
  MaxWait := 10;
  while MaxWait > 0 do
  begin
    Sleep(1000);
    if Exec('cmd.exe', '/c tasklist /fi "imagename eq {#MyAppServiceExeName}" /nh | findstr /i "{#MyAppServiceExeName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    begin
      if ResultCode <> 0 then
        Break; // 进程已优雅退出
    end;
    Dec(MaxWait);
  end;

  Exec('taskkill', '/f /im "{#MyAppServiceExeName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);

  if IsAppRunning then
  begin
    Exec('taskkill', '/f /im "{#MyAppExeName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Log('已关闭运行中的配置面板');
    Sleep(500);
  end;
end;

function InitializeSetup: Boolean;
var
  ResultCode: Integer;
begin
  ExistingInstallDirExists := RegQueryStringValue(
    HKLM,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#MyAppId}_is1',
    'InstallLocation',
    ExistingInstallDir
  ) and (ExistingInstallDir <> '');
  if ExistingInstallDirExists then
    Log('检测到现有安装目录，将锁定安装路径: ' + ExistingInstallDir);

  Result := True;
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

procedure StopServiceIfExists;
var
  ResultCode: Integer;
  MaxWait: Integer;
begin
  Exec('sc.exe', 'stop "{#MyServiceName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  // 轮询等待服务进程真正退出，最多等 15 秒
  MaxWait := 15;
  while MaxWait > 0 do
  begin
    Sleep(1000);
    if Exec('cmd.exe', '/c tasklist /fi "imagename eq {#MyAppServiceExeName}" /nh | findstr /i "{#MyAppServiceExeName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    begin
      if ResultCode <> 0 then
        Exit; // findstr 未找到 → 进程已退出
    end;
    Dec(MaxWait);
  end;
  // 超时未退出 → 强制终止，防止 {app} 文件被占用
  Exec('taskkill', '/f /im "{#MyAppServiceExeName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
end;

procedure RemoveServiceIfExists;
var
  ResultCode: Integer;
begin
  StopServiceIfExists;
  Exec('sc.exe', 'delete "{#MyServiceName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

procedure InstallAuthService;
var
  ServiceExePath: string;
  ResultCode: Integer;
begin
  ServiceExePath := ExpandConstant('{app}\{#MyAppServiceExeName}');
  RemoveServiceIfExists;
  if Exec('sc.exe', 'create "{#MyServiceName}" binPath= "' + ServiceExePath + '" start= auto depend= npcap DisplayName= "{#MyAppName}"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode) and (ResultCode = 0) then
    Log('Windows 服务已安装: {#MyServiceName}')
  else
    Log('Windows 服务安装失败（错误码: ' + IntToStr(ResultCode) + '）');
  Exec('sc.exe', 'description "{#MyServiceName}" "SYSU wired campus network 802.1X authentication service"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('sc.exe', 'failure "{#MyServiceName}" reset= 86400 actions= restart/60000/none/0/none/0',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  if Exec('sc.exe', 'start "{#MyServiceName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) and (ResultCode = 0) then
    Log('Windows 服务已启动')
  else
    Log('Windows 服务启动失败或已在运行（错误码: ' + IntToStr(ResultCode) + '）');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  NpcapMsg: string;
begin
  if CurStep = ssInstall then
  begin
    // 用户已点击安装 → 此时关闭旧版本进程/服务，准备覆盖文件
    StopManagedProcesses;
  end;

  if CurStep = ssPostInstall then
  begin
    AddToPath(ExpandConstant('{app}'));

    InstallAuthService;

    // Npcap 未安装 → 弹窗提示用户
    if not IsNpcapInstalled then
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
  choice: Integer;
begin
  case CurUninstallStep of
    usPostUninstall:
      begin
        // 清理系统 PATH
        RemoveFromPath(ExpandConstant('{app}'));

        // 清理 Windows 认证服务
        RemoveServiceIfExists;

        // 清理可能的开机自启残留
        RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#MyAppId}');
        DeleteFile(ExpandConstant('{userstartup}\{#MyAppName}.lnk'));

        // 清理原子写入残留的 .json.tmp 临时文件（程序崩溃可能产生）
        Exec('cmd.exe', '/c del /q "' + ExpandConstant('{commonappdata}\{#MyAppId}') + '\*.json.tmp" 2>nul', '', SW_HIDE, ewWaitUntilTerminated, choice);
        Exec('cmd.exe', '/c del /q "' + ExpandConstant('{userappdata}\{#MyAppId}') + '\*.json.tmp" 2>nul', '', SW_HIDE, ewWaitUntilTerminated, choice);
        Exec('cmd.exe', '/c del /q "' + ExpandConstant('{userappdata}\CampusNetAuth') + '\*.json.tmp" 2>nul', '', SW_HIDE, ewWaitUntilTerminated, choice);

        // 询问删除用户配置数据
        appDataPath := ExpandConstant('{userappdata}');
        userDataPath := appDataPath + '\{#MyAppId}';
        legacyUserDataPath := appDataPath + '\CampusNetAuth';
        if DirExists(userDataPath) or DirExists(legacyUserDataPath) then
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
        if DirExists(userDataPath) then
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
        if IsNpcapInstalled then
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
