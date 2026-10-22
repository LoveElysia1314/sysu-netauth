{ Native process waits are bounded and pump messages so Setup stays responsive. }
type
  { Inno Setup 6 uses Win32 API layouts, including on x64 installations. }
  TSetupSecurityAttributes = record
    Size: Cardinal;
    Descriptor: LongWord;
    Inherit: Integer;
  end;
  TSetupStartupInfo = record
    cb: Cardinal;
    Reserved, Desktop, Title: LongWord;
    X, Y, XSize, YSize, XChars, YChars, Fill, Flags: Cardinal;
    ShowWindow, ReservedSize: Word;
    ReservedData, StdInput, StdOutput, StdError: LongWord;
  end;
  TSetupProcessInfo = record
    Process, Thread: THandle;
    ProcessId, ThreadId: Cardinal;
  end;
  TSetupMessage = array[0..15] of Cardinal;

function SetupCreateProcess(Application, CommandLine: String;
  ProcessAttributes, ThreadAttributes: LongWord; InheritHandles: Integer;
  Flags: Cardinal; Environment, Directory: LongWord;
  var Startup: TSetupStartupInfo; var Process: TSetupProcessInfo): Integer;
  external 'CreateProcessW@kernel32.dll stdcall';
function SetupWait(Process: THandle; Milliseconds: Cardinal): Cardinal;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function SetupExitCode(Process: THandle; var Code: Cardinal): Integer;
  external 'GetExitCodeProcess@kernel32.dll stdcall';
function SetupTerminate(Process: THandle; Code: Cardinal): Integer;
  external 'TerminateProcess@kernel32.dll stdcall';
function SetupClose(Handle: THandle): Integer;
  external 'CloseHandle@kernel32.dll stdcall';
function SetupLastError: Cardinal;
  external 'GetLastError@kernel32.dll stdcall';
function SetupOpenFile(Name: String; Access, Share: Cardinal;
  var Security: TSetupSecurityAttributes; Creation, Flags: Cardinal;
  Template: THandle): THandle;
  external 'CreateFileW@kernel32.dll stdcall';
function SetupTicks: Int64;
  external 'GetTickCount64@kernel32.dll stdcall';
function SetupPeek(var Message: TSetupMessage; Window: HWND;
  First, Last, Remove: Cardinal): Integer;
  external 'PeekMessageW@user32.dll stdcall';
function SetupTranslate(var Message: TSetupMessage): Integer;
  external 'TranslateMessage@user32.dll stdcall';
function SetupDispatch(var Message: TSetupMessage): Integer;
  external 'DispatchMessageW@user32.dll stdcall';

procedure PumpSetupMessages;
var Message: TSetupMessage; Count: Integer;
begin
  for Count := 1 to 32 do begin
    if SetupPeek(Message, 0, 0, 0, 1) = 0 then Break;
    SetupTranslate(Message);
    SetupDispatch(Message);
  end;
end;

function RunSetupCommand(Filename, Params: String; TimeoutSeconds: Integer;
  var Code: Integer): Boolean;
var
  Startup: TSetupStartupInfo;
  Security: TSetupSecurityAttributes;
  Process: TSetupProcessInfo;
  CommandLine: String;
  Deadline: Int64;
  WaitResult, ExitCode: Cardinal;
  Created: Integer;
  NullHandle: THandle;
begin
  Result := False;
  Log('Starting setup command: ' + Filename + ' ' + Params);
  Startup.cb := SizeOf(Startup);
  { A real inherited NUL handle is needed; null handles can attach to a console. }
  Security.Size := SizeOf(Security);
  Security.Inherit := 1;
  NullHandle := SetupOpenFile('NUL', $C0000000, 3, Security, 3, 0, 0);
  if NullHandle = THandle(-1) then begin
    Code := SetupLastError;
    Log('Unable to open NUL for setup command, error ' + IntToStr(Code));
    Exit;
  end;
  Startup.StdInput := NullHandle;
  Startup.StdOutput := NullHandle;
  Startup.StdError := NullHandle;
  Startup.Flags := $101; { STARTF_USESHOWWINDOW | STARTF_USESTDHANDLES }
  CommandLine := '"' + Filename + '" ' + Params;
  Created := SetupCreateProcess(Filename, CommandLine, 0, 0, 1, $08000000,
    0, 0, Startup, Process);
  if Created = 0 then Code := SetupLastError;
  SetupClose(NullHandle);
  if Created = 0 then begin
    Log('Unable to start setup command, error ' + IntToStr(Code));
    Exit;
  end;
  SetupClose(Process.Thread);
  try
    Deadline := SetupTicks + Int64(TimeoutSeconds) * 1000;
    repeat
      WaitResult := SetupWait(Process.Process, 50);
      if WaitResult = 0 then begin
        if SetupExitCode(Process.Process, ExitCode) <> 0 then begin
          Code := ExitCode;
          Result := True;
        end else
          Code := SetupLastError;
        Log('Setup command exited, code ' + IntToStr(Code));
        Exit;
      end;
      if WaitResult <> $102 then begin { WAIT_TIMEOUT }
        Code := SetupLastError;
        Log('Setup command wait failed, error ' + IntToStr(Code));
        Exit;
      end;
      { Bound the message batch too, so window traffic cannot defeat the timeout. }
      PumpSetupMessages;
    until SetupTicks >= Deadline;
    Code := 1460; { ERROR_TIMEOUT }
    Log('Setup command timed out: ' + Filename);
    SetupTerminate(Process.Process, Code);
    SetupWait(Process.Process, 1000);
  finally
    SetupClose(Process.Process);
  end;
end;
