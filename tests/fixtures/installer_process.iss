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
#include "../../apps/rust/scripts/process.iss"

#ifndef TestPrefix
  #define TestPrefix ""
#endif

function InitializeSetup: Boolean;
var
  Code: Integer;
  Started: Int64;
begin
  Started := SetupTicks;
  if RunSetupCommand('{#TestCommand}', '{#TestPrefix} hang', 1, Code) or
    (Code <> 1460) then RaiseException('Hung process was not timed out');
  if SetupTicks - Started > 5000 then RaiseException('Process timeout was not bounded');
  if not RunSetupCommand('{#TestCommand}', '{#TestPrefix} stdin', 5, Code) or
    (Code <> 0) then RaiseException('Child stdin did not close immediately');
  if not RunSetupCommand('{#TestCommand}', '{#TestPrefix} exit', 5, Code) or
    (Code <> 7) then RaiseException('Child exit code was lost');
  if RunSetupCommand(ExpandConstant('{tmp}\missing-netauth-process.exe'), '', 1, Code) or
    (Code <> 2) then RaiseException('Missing executable error was lost');
  Log('Installer process regression checks passed');
  Result := True;
end;
