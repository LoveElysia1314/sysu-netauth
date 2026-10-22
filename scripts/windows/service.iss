{ Shared by both installers. All layouts below are for 32-bit Inno Setup 6. }
#ifndef ManagedServiceName
  #define ManagedServiceName "SYSUNetAuth"
#endif
#ifndef ManagedGuiName
  #define ManagedGuiName "sysu_netauth_gui.exe"
#endif
#ifndef ManagedCliName
  #define ManagedCliName "sysu_netauth.exe"
#endif
#ifndef ManagedServiceExe
  #define ManagedServiceExe "sysu_netauth_service.exe"
#endif
#ifndef ServiceStopSeconds
  #define ServiceStopSeconds 15
#endif

type
  TManagedServiceStatus = record
    Kind, State, Controls, ExitCode, SpecificExitCode, Checkpoint, WaitHint: Cardinal;
    ProcessId, Flags: Cardinal;
  end;
  TManagedProcessEntry = record
    Size, Usage, ProcessId, Heap, Module, Threads, ParentId: Cardinal;
    Priority: Integer;
    Flags: Cardinal;
    Exe: array[0..259] of Word;
  end;

function ManagedOpenSCManager(Machine, Database: LongWord; Access: Cardinal): THandle;
  external 'OpenSCManagerW@advapi32.dll stdcall';
function ManagedOpenService(Manager: THandle; Name: String; Access: Cardinal): THandle;
  external 'OpenServiceW@advapi32.dll stdcall';
function ManagedCloseService(Service: THandle): Integer;
  external 'CloseServiceHandle@advapi32.dll stdcall';
function ManagedQueryStatus(Service: THandle; Level: Cardinal;
  var Status: TManagedServiceStatus; Size: Cardinal; var Needed: Cardinal): Integer;
  external 'QueryServiceStatusEx@advapi32.dll stdcall';
function ManagedControl(Service: THandle; Control: Cardinal;
  var Status: TManagedServiceStatus): Integer;
  external 'ControlService@advapi32.dll stdcall';
function ManagedChangeConfig(Service: THandle; Kind, Start, Error: Cardinal;
  Binary, Group, Tag, Dependencies, Account, Password, DisplayName: LongWord): Integer;
  external 'ChangeServiceConfigW@advapi32.dll stdcall';
function ManagedDeleteService(Service: THandle): Integer;
  external 'DeleteService@advapi32.dll stdcall';
function ManagedSnapshot(Flags, ProcessId: Cardinal): THandle;
  external 'CreateToolhelp32Snapshot@kernel32.dll stdcall';
function ManagedProcessFirst(Snapshot: THandle; var Entry: TManagedProcessEntry): Integer;
  external 'Process32FirstW@kernel32.dll stdcall';
function ManagedProcessNext(Snapshot: THandle; var Entry: TManagedProcessEntry): Integer;
  external 'Process32NextW@kernel32.dll stdcall';
function ManagedOpenProcess(Access: Cardinal; Inherit: Integer; Id: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall';

var
  ManagedStartChanged: Boolean;
  ManagedOriginalStart: Cardinal;

function ManagedError(Operation: String): String;
var Code: Cardinal;
begin
  Code := SetupLastError;
  Result := Operation + ' (Windows error ' + IntToStr(Code) + ': ' + SysErrorMessage(Code) + ')';
  Log(Result);
end;

function WaitManagedProcess(Process: THandle; Seconds: Integer): Boolean;
var Deadline: Int64; State: Cardinal;
begin
  Deadline := SetupTicks + Int64(Seconds) * 1000;
  repeat
    State := SetupWait(Process, 50);
    Result := State = 0;
    if Result or (State <> $102) then Exit;
    PumpSetupMessages;
  until SetupTicks >= Deadline;
end;

function CloseManagedProcess(Id: Cardinal; var Error: String): Boolean;
var Process: THandle; Code: Cardinal;
begin
  Result := False;
  Process := ManagedOpenProcess($00100001, 0, Id); { SYNCHRONIZE | TERMINATE }
  if Process = 0 then begin
    Code := SetupLastError;
    if Code = 87 then Result := True { Process already exited. }
    else Error := ManagedError('Cannot open application process ' + IntToStr(Id));
    Exit;
  end;
  try
    if SetupWait(Process, 0) <> 0 then begin
      Log('Terminating application process ' + IntToStr(Id));
      if SetupTerminate(Process, 0) = 0 then begin
        Error := ManagedError('Cannot terminate application process ' + IntToStr(Id));
        Exit;
      end;
    end;
    Result := WaitManagedProcess(Process, 5);
    if not Result then Error := 'Application process did not exit: ' + IntToStr(Id);
  finally
    SetupClose(Process);
  end;
end;

function CloseManagedImages(IncludeService: Boolean; var Error: String): Boolean;
var Snapshot: THandle; Entry: TManagedProcessEntry; Name: String; I: Integer;
begin
  Result := False;
  Snapshot := ManagedSnapshot(2, 0); { TH32CS_SNAPPROCESS, across Windows sessions. }
  if Snapshot = THandle(-1) then begin
    Error := ManagedError('Cannot enumerate application processes');
    Exit;
  end;
  try
    Entry.Size := SizeOf(Entry);
    if ManagedProcessFirst(Snapshot, Entry) = 0 then begin
      Error := ManagedError('Cannot read application process list');
      Exit;
    end;
    repeat
      Name := '';
      for I := 0 to 259 do begin
        if Entry.Exe[I] = 0 then Break;
        Name := Name + Chr(Entry.Exe[I]);
      end;
      if (CompareText(Name, '{#ManagedGuiName}') = 0) or
        (CompareText(Name, '{#ManagedCliName}') = 0) or
        (IncludeService and (CompareText(Name, '{#ManagedServiceExe}') = 0)) then
        if not CloseManagedProcess(Entry.ProcessId, Error) then Exit;
    until ManagedProcessNext(Snapshot, Entry) = 0;
    if SetupLastError <> 18 then begin { ERROR_NO_MORE_FILES }
      Error := ManagedError('Cannot finish reading application process list');
      Exit;
    end;
    Result := True;
  finally
    SetupClose(Snapshot);
  end;
end;

function StopManagedApp(var Error: String): Boolean;
var
  Manager, Service: THandle;
  Status: TManagedServiceStatus;
  Needed, Code: Cardinal;
  Deadline: Int64;
begin
  Result := False;
  Error := '';
  Manager := ManagedOpenSCManager(0, 0, 1);
  if Manager = 0 then begin
    Error := ManagedError('Cannot open Windows service manager');
    Exit;
  end;
  try
    Service := ManagedOpenService(Manager, '{#ManagedServiceName}', $26);
    if Service = 0 then begin
      Code := SetupLastError;
      if Code <> 1060 then begin { ERROR_SERVICE_DOES_NOT_EXIST }
        Error := ManagedError('Cannot open authentication service');
        Exit;
      end;
    end else try
      if ManagedQueryStatus(Service, 0, Status, SizeOf(Status), Needed) = 0 then begin
        Error := ManagedError('Cannot query authentication service');
        Exit;
      end;
      { Prevent GUI, logon tasks and SCM recovery from restarting the old binary. }
      if not ManagedStartChanged then begin
        if not RegQueryDWordValue(HKLM64, 'SYSTEM\CurrentControlSet\Services\{#ManagedServiceName}',
          'Start', ManagedOriginalStart) then begin
          Error := 'Cannot read authentication service startup setting';
          Exit;
        end;
        if ManagedChangeConfig(Service, $FFFFFFFF, 4, $FFFFFFFF, 0, 0, 0, 0, 0, 0, 0) = 0 then begin
          Error := ManagedError('Cannot temporarily disable authentication service');
          Exit;
        end;
        ManagedStartChanged := True;
      end;
      if not CloseManagedImages(False, Error) then Exit;
      if (Status.State <> 1) and (Status.State <> 3) then
        if ManagedControl(Service, 1, Status) = 0 then begin
          Code := SetupLastError;
          { START_PENDING may reject STOP; a stuck service is handled below. }
          if (Code <> 1053) and (Code <> 1061) and (Code <> 1062) then begin
            Error := ManagedError('Cannot stop authentication service');
            Exit;
          end;
        end;
      Deadline := SetupTicks + {#ServiceStopSeconds} * 1000;
      repeat
        if ManagedQueryStatus(Service, 0, Status, SizeOf(Status), Needed) = 0 then begin
          Error := ManagedError('Cannot query authentication service shutdown');
          Exit;
        end;
        if Status.State = 1 then Break; { SERVICE_STOPPED }
        Sleep(50);
        PumpSetupMessages;
      until SetupTicks >= Deadline;
      if Status.State <> 1 then begin
        Log('Authentication service stop timed out; terminating its processes');
        if not CloseManagedImages(True, Error) then Exit;
        Deadline := SetupTicks + 5000;
        repeat
          if ManagedQueryStatus(Service, 0, Status, SizeOf(Status), Needed) = 0 then begin
            Error := ManagedError('Cannot verify authentication service shutdown');
            Exit;
          end;
          if Status.State = 1 then Break;
          Sleep(50);
          PumpSetupMessages;
        until SetupTicks >= Deadline;
        if Status.State <> 1 then begin
          Error := 'Authentication service has not stopped. Restart Windows and retry.';
          Exit;
        end;
      end;
    finally
      ManagedCloseService(Service);
    end;
    { Includes foreground services and PyInstaller parent/child processes. }
    Result := CloseManagedImages(True, Error);
    if Result then Log('Authentication service and application processes have stopped');
  finally
    ManagedCloseService(Manager);
  end;
end;

procedure RestoreManagedStart;
var Manager, Service: THandle;
begin
  if not ManagedStartChanged then Exit;
  Manager := ManagedOpenSCManager(0, 0, 1);
  if Manager = 0 then Exit;
  try
    Service := ManagedOpenService(Manager, '{#ManagedServiceName}', 2);
    if Service = 0 then Exit;
    try
      if ManagedChangeConfig(Service, $FFFFFFFF, ManagedOriginalStart, $FFFFFFFF,
        0, 0, 0, 0, 0, 0, 0) = 0 then Log(ManagedError('Cannot restore service startup setting'))
      else ManagedStartChanged := False;
    finally
      ManagedCloseService(Service);
    end;
  finally
    ManagedCloseService(Manager);
  end;
end;

function RemoveManagedService(var Error: String): Boolean;
var Manager, Service: THandle; Code: Cardinal;
begin
  Result := False;
  Manager := ManagedOpenSCManager(0, 0, 1);
  if Manager = 0 then begin
    Error := ManagedError('Cannot open service manager for removal');
    Exit;
  end;
  try
    Service := ManagedOpenService(Manager, '{#ManagedServiceName}', $00010000);
    if Service = 0 then begin
      Code := SetupLastError;
      Result := (Code = 1060) or (Code = 1072);
      if not Result then Error := ManagedError('Cannot open authentication service for removal');
    end else try
      Result := ManagedDeleteService(Service) <> 0;
      if not Result then begin
        Code := SetupLastError;
        Result := Code = 1072; { Already marked for deletion. }
        if not Result then Error := ManagedError('Cannot delete authentication service');
      end;
    finally
      ManagedCloseService(Service);
    end;
    if Result then ManagedStartChanged := False;
  finally
    ManagedCloseService(Manager);
  end;
end;
