{ Dependency guidance only. Npcap is installed through its official installer. }
const
  NpcapUrlConst = '{#NpcapUrl}';

var
  NpcapPage: TWizardPage;
  NpcapStatusLabel: TNewStaticText;

function HasNpcapDriver: Boolean;
begin
  Result := RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\npcap') or
            RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\npcap_flt');
end;

function IsNpcapInstalled: Boolean;
begin
  { Check the library the Rust client actually loads, as well as the driver. }
  Result := HasNpcapDriver and FileExists(ExpandConstant('{sys}\Npcap\wpcap.dll'));
end;

function NpcapRequired: Boolean;
begin
  Result := not IsNpcapInstalled;
end;

procedure RefreshNpcapStatus;
begin
  if IsNpcapInstalled then
    NpcapStatusLabel.Caption := '已检测到 Npcap，可以继续安装。'
  else
    NpcapStatusLabel.Caption :=
      '尚未检测到完整的 Npcap 安装。网络认证前，请先安装 Npcap。';
  Log(NpcapStatusLabel.Caption);
end;

procedure OpenNpcapDownload(Sender: TObject);
var
  Code: Integer;
begin
  if not ShellExecAsOriginalUser('open', NpcapUrlConst, '', '', SW_SHOWNORMAL,
    ewNoWait, Code) then
    SuppressibleMsgBox('无法打开浏览器，请访问：' #13#10 + NpcapUrlConst,
      mbInformation, MB_OK, IDOK);
end;

procedure RecheckNpcap(Sender: TObject);
begin
  RefreshNpcapStatus;
end;

procedure InitializeNpcapPage;
var
  Instructions: TNewStaticText;
  DownloadButton, RecheckButton: TNewButton;
begin
  NpcapPage := CreateCustomPage(wpSelectDir, 'Npcap 网络驱动',
    'SYSU NetAuth 需要 Npcap 才能进行有线网络认证。');

  NpcapStatusLabel := TNewStaticText.Create(NpcapPage);
  NpcapStatusLabel.Parent := NpcapPage.Surface;
  NpcapStatusLabel.SetBounds(0, 0, NpcapPage.SurfaceWidth, ScaleY(42));
  NpcapStatusLabel.AutoSize := False;
  NpcapStatusLabel.WordWrap := True;

  Instructions := TNewStaticText.Create(NpcapPage);
  Instructions.Parent := NpcapPage.Surface;
  Instructions.SetBounds(0, ScaleY(54), NpcapPage.SurfaceWidth, ScaleY(116));
  Instructions.AutoSize := False;
  Instructions.WordWrap := True;
  Instructions.Caption :=
    '1. 打开 Npcap 官网并下载 Windows 安装程序。' #13#10#13#10 +
    '2. 运行安装程序，保持默认选项，按提示完成安装。' #13#10#13#10 +
    '3. 返回此窗口，点击“重新检测”确认安装状态。' #13#10#13#10 +
    '也可以稍后安装：完成本向导后，在配置面板点击“安装 Npcap”。';

  DownloadButton := TNewButton.Create(NpcapPage);
  DownloadButton.Parent := NpcapPage.Surface;
  DownloadButton.SetBounds(0, ScaleY(182), ScaleX(180), ScaleY(28));
  DownloadButton.Caption := '打开 Npcap 官网下载页';
  DownloadButton.OnClick := @OpenNpcapDownload;

  RecheckButton := TNewButton.Create(NpcapPage);
  RecheckButton.Parent := NpcapPage.Surface;
  RecheckButton.SetBounds(ScaleX(192), ScaleY(182), ScaleX(108), ScaleY(28));
  RecheckButton.Caption := '重新检测';
  RecheckButton.OnClick := @RecheckNpcap;
  RefreshNpcapStatus;
end;

function SkipNpcapPage(PageID: Integer): Boolean;
begin
  Result := (PageID = NpcapPage.ID) and (WizardSilent or IsNpcapInstalled);
end;

procedure OfferNpcapUninstall;
var
  Uninstaller: String;
  Code: Integer;
begin
  if UninstallSilent then Exit;
  Uninstaller := ExpandConstant('{pf64}\Npcap\uninstall.exe');
  if not FileExists(Uninstaller) then
    Uninstaller := ExpandConstant('{pf32}\Npcap\uninstall.exe');
  if not HasNpcapDriver and not FileExists(Uninstaller) then Exit;
  if SuppressibleMsgBox(
    '是否打开 Npcap 卸载向导？' #13#10#13#10 +
    'Wireshark、Nmap 等其他网络工具也可能需要 Npcap。默认保留驱动。',
    mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) <> IDYES then Exit;
  if FileExists(Uninstaller) then begin
    if Exec(Uninstaller, '', '', SW_SHOW, ewWaitUntilTerminated, Code) then begin
      Log('Npcap 卸载向导已退出，代码：' + IntToStr(Code));
      Exit;
    end;
  end;
  SuppressibleMsgBox(
    '请在 Windows“设置 → 应用 → 已安装的应用”中搜索 Npcap 并卸载。',
    mbInformation, MB_OK, IDOK);
end;
