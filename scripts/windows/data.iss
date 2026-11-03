{ Restrict the root first, then make existing descendants inherit that ACL.
  Applying (OI)(CI) grants recursively to files can leave their DACL empty. }
procedure SecureDataDirectory(Directory: String);
var Code: Integer; Entry: TFindRec; HasChildren: Boolean;
begin
  if not ForceDirectories(Directory) then
    RaiseException('无法创建配置目录：' + Directory);
  if not RunSetupCommand(ExpandConstant('{sys}\icacls.exe'),
    '"' + Directory + '" /inheritance:r /remove:g *S-1-5-32-545 /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F', 30, Code) or (Code <> 0) then
    RaiseException('无法设置配置目录权限，错误码：' + IntToStr(Code));
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
