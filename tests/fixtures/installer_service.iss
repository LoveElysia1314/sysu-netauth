[Setup]
AppId={#ManagedServiceName}
AppName=Installer service regression checks
AppVersion={#TestVersion}
DefaultDirName={tmp}\netauth-service-check
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64os
DisableWelcomePage=yes
DisableReadyPage=yes
DisableFinishedPage=yes
OutputBaseFilename=installer-service-check-{#TestVersion}
UninstallLogging=yes

[Files]
Source: "{#TestBinary}"; DestDir: "{app}"; DestName: "{#ManagedServiceExe}"; Flags: ignoreversion
Source: "{#TestBinary}"; DestDir: "{app}"; DestName: "{#ManagedGuiName}"; Flags: ignoreversion
Source: "{#TestBinary}"; DestDir: "{app}"; DestName: "{#ManagedCliName}"; Flags: ignoreversion
Source: "{#TestPayload}"; DestDir: "{app}"; DestName: "payload.txt"; Flags: ignoreversion

[Code]
#include "../../scripts/windows/process.iss"
#include "../../scripts/windows/service.iss"

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if StopManagedApp(Result) then Result := '';
#ifdef TestCancelAfterStop
  if Result = '' then Result := 'Deliberate failure after service stop';
#endif
end;

procedure DeinitializeSetup;
begin
  RestoreManagedStart;
end;

procedure DeinitializeUninstall;
begin
  RestoreManagedStart;
end;

procedure CurUninstallStepChanged(Step: TUninstallStep);
var Error: String;
begin
  if Step = usUninstall then
    if not StopManagedApp(Error) or not RemoveManagedService(Error) then
      RaiseException(Error);
end;
