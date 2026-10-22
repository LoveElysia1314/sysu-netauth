[Setup]
AppName=Installer process regression checks
AppVersion=1.0
DefaultDirName={tmp}\netauth-process-check
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
DisableWelcomePage=yes
DisableReadyPage=yes
DisableFinishedPage=yes
OutputBaseFilename=installer-process-check

[Code]
#include "../../scripts/windows/process.iss"

function InitializeSetup: Boolean;
var
  Code: Integer;
  Started: Int64;
  ChildArgs: String;
begin
  { Quote here: embedded quotes in ISCC /D arguments are parsed differently
    from the Windows C runtime convention used by Python subprocess. }
  ChildArgs := '"{#TestScript}" ';
  Started := SetupTicks;
  if RunSetupCommand('{#TestCommand}', ChildArgs + 'hang', 1, Code) or
    (Code <> 1460) then RaiseException('Hung process was not timed out');
  if SetupTicks - Started > 5000 then RaiseException('Process timeout was not bounded');
  if not RunSetupCommand('{#TestCommand}', ChildArgs + 'stdin', 5, Code) or
    (Code <> 0) then RaiseException('Child stdin did not close immediately');
  if not RunSetupCommand('{#TestCommand}', ChildArgs + 'exit', 5, Code) or
    (Code <> 7) then RaiseException('Child exit code was lost');
  if RunSetupCommand(ExpandConstant('{tmp}\missing-netauth-process.exe'), '', 1, Code) or
    (Code <> 2) then RaiseException('Missing executable error was lost');
  Log('Installer process regression checks passed');
  Result := True;
end;
