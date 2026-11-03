{ Restrict the root first, then make existing descendants inherit that ACL.
  Applying (OI)(CI) grants recursively to files can leave their DACL empty. }
function DataSecurityDescriptor(Text: String; Revision: Cardinal;
  var Descriptor: LongWord; Size: LongWord): Integer;
  external 'ConvertStringSecurityDescriptorToSecurityDescriptorW@advapi32.dll stdcall';
function DataSetSecurity(Name: String; Information: Cardinal; Descriptor: LongWord): Integer;
  external 'SetFileSecurityW@advapi32.dll stdcall';
function DataFree(Descriptor: LongWord): LongWord;
  external 'LocalFree@kernel32.dll stdcall';

procedure SecureDataDirectory(Directory: String);
var Code: Integer; Entry: TFindRec; HasChildren: Boolean; Descriptor: LongWord;
begin
  if not ForceDirectories(Directory) then
    RaiseException('无法创建配置目录：' + Directory);
  Descriptor := 0;
  if DataSecurityDescriptor('D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)', 1, Descriptor, 0) = 0 then
    RaiseException('无法创建配置目录安全描述符，错误码：' + IntToStr(SetupLastError));
  try
    { Replace the complete ACL, including any pre-existing explicit grants. }
    if DataSetSecurity(Directory, $80000004, Descriptor) = 0 then
      RaiseException('无法设置配置目录权限，错误码：' + IntToStr(SetupLastError));
  finally
    DataFree(Descriptor);
  end;
  HasChildren := False;
  if FindFirst(AddBackslash(Directory) + '*', Entry) then begin
    try
      repeat
        if (Entry.Name <> '.') and (Entry.Name <> '..') then HasChildren := True;
      until HasChildren or not FindNext(Entry);
    finally
      FindClose(Entry);
    end;
  end;
  if HasChildren then
    if not RunSetupCommand(ExpandConstant('{sys}\icacls.exe'),
      '"' + AddBackslash(Directory) + '*" /reset /T /L /Q', 30, Code) or (Code <> 0) then
      RaiseException('无法恢复配置文件权限，错误码：' + IntToStr(Code));
end;
