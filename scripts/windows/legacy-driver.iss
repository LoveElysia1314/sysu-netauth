{ Only known clients predating the native backend should offer driver cleanup. }
function LegacyNpcapVersion(Version: String): Boolean;
var Minor, Patch, Dot, Beta: Integer; Tail: String;
begin
  Result := False;
  Version := Trim(Version);
  if Pos('0.7.0-alpha.', Version) = 1 then begin
    Result := StrToIntDef(Copy(Version, 13, 32), -1) >= 0;
    Exit;
  end;
  if Pos('0.7.0-beta.', Version) = 1 then begin
    Beta := StrToIntDef(Copy(Version, 12, 32), -1);
    Result := (Beta >= 0) and (Beta < 3);
    Exit;
  end;
  if Pos('0.', Version) <> 1 then Exit;
  Tail := Copy(Version, 3, 64);
  Dot := Pos('.', Tail);
  if Dot = 0 then Exit;
  Minor := StrToIntDef(Copy(Tail, 1, Dot - 1), -1);
  Patch := StrToIntDef(Copy(Tail, Dot + 1, 32), -1);
  Result := (Minor >= 0) and (Minor < 7) and (Patch >= 0);
end;

function LegacyDriverCleanup(Version: String; DriverPresent: Boolean): Boolean;
begin
  Result := DriverPresent and LegacyNpcapVersion(Version);
end;
