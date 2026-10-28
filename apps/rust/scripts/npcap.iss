{ Dependency guidance only. Npcap is installed through its official installer. }
const
  NpcapUrlConst = '{#NpcapUrl}';

var
  NpcapPage: TWizardPage;
  NpcapStatusLabel: TNewStaticText;

function NpcapLibraryMatches(Filename: String): Boolean;
var
  Data: AnsiString;
  Header: Int64;
  Machine: Integer;
begin
  Result := False;
  if not LoadStringFromFile(Filename, Data) then Exit;
  if Length(Data) < 64 then Exit;
  if Copy(Data, 1, 2) <> 'MZ' then Exit;
  { PE offsets are zero-based; Pascal strings are one-based. }
  Header := Int64(Ord(Data[61])) + Int64(Ord(Data[62])) * 256 +
    Int64(Ord(Data[63])) * 65536 + Int64(Ord(Data[64])) * 16777216;
  if (Header < 64) or (Header > Length(Data) - 6) then Exit;
  if Copy(Data, Header + 1, 4) <> 'PE' + #0#0 then Exit;
  Machine := Ord(Data[Header + 5]) + Ord(Data[Header + 6]) * 256;
#if MyAppArch == "arm64"
  Result := (Machine = $AA64) or (Machine = $A64E); { ARM64 / ARM64X }
#else
  Result := Machine = $8664;
#endif
end;

function HasNpcapDriver: Boolean;
begin
  Result := RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\npcap') or
            RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\npcap_flt');
end;

function IsNpcapInstalled: Boolean;
begin
  { Check both libraries in the native system directory, as well as the driver. }
  Result := HasNpcapDriver and
    NpcapLibraryMatches(ExpandConstant('{sys}\Npcap\wpcap.dll')) and
    NpcapLibraryMatches(ExpandConstant('{sys}\Npcap\Packet.dll'));
end;

function NpcapRequired: Boolean;
begin
  Result := not IsNpcapInstalled;
end;

procedure RefreshNpcapStatus;
begin
  if IsNpcapInstalled then
    NpcapStatusLabel.Caption := '已检测到兼容的 Npcap（{#MyAppArch}），可以继续安装。'
  else
    NpcapStatusLabel.Caption :=
      '尚未检测到适用于 {#MyAppArch} 的完整 Npcap 安装。请使用官网安装程序安装或更新 Npcap。';
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
    'SYSU NetAuth（{#MyAppArch}）需要对应架构的 Npcap 才能进行有线网络认证。');

  NpcapStatusLabel := TNewStaticText.Create(NpcapPage);
  NpcapStatusLabel.Parent := NpcapPage.Surface;
  NpcapStatusLabel.SetBounds(0, 0, NpcapPage.SurfaceWidth, ScaleY(42));
  NpcapStatusLabel.AutoSize := False;
  NpcapStatusLabel.WordWrap := True;

  Instructions := TNewStaticText.Create(NpcapPage);
  Instructions.Parent := NpcapPage.Surface;
  Instructions.SetBounds(0, ScaleY(54), NpcapPage.SurfaceWidth, ScaleY(144));
  Instructions.AutoSize := False;
  Instructions.WordWrap := True;
  Instructions.Caption :=
    '1. 打开 Npcap 官网并下载最新的 Windows 安装程序。' #13#10#13#10 +
    '2. 官方安装程序会自动选择本机架构（x64 或 ARM64）的驱动和 DLL。保持默认选项完成安装；不要单独复制其他架构的 DLL。' #13#10#13#10 +
    '3. 返回此窗口，点击“重新检测”确认安装状态。' #13#10#13#10 +
    '也可以稍后安装：完成本向导后，在配置面板点击“安装 Npcap”。';

  DownloadButton := TNewButton.Create(NpcapPage);
  DownloadButton.Parent := NpcapPage.Surface;
  DownloadButton.SetBounds(0, ScaleY(210), ScaleX(180), ScaleY(28));
  DownloadButton.Caption := '打开 Npcap 官网下载页';
  DownloadButton.OnClick := @OpenNpcapDownload;

  RecheckButton := TNewButton.Create(NpcapPage);
  RecheckButton.Parent := NpcapPage.Surface;
  RecheckButton.SetBounds(ScaleX(192), ScaleY(210), ScaleX(108), ScaleY(28));
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
