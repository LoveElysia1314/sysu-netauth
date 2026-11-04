[Setup]
AppId=NetAuthLegacyDriverTest
AppName=Legacy driver cleanup regression checks
AppVersion=1.0
DefaultDirName={tmp}\netauth-legacy-driver-check
PrivilegesRequired=admin
Uninstallable=no
CreateAppDir=no
OutputBaseFilename=installer-legacy-driver-check

[Code]
#include "../../scripts/windows/legacy-driver.iss"
procedure Check(Version: String; DriverPresent, Expected: Boolean);
begin
  if LegacyDriverCleanup(Version, DriverPresent) <> Expected then
    RaiseException('Incorrect driver cleanup policy for ' + Version);
end;
function InitializeSetup: Boolean;
begin
  Check('0.6.3', True, True);
  Check('0.7.0-alpha.0', True, True);
  Check('0.7.0-beta.1', True, True);
  Check('0.7.0-beta.2', True, True);
  Check('0.6.3', False, False);
  Check('', True, False);
  Check('unknown', True, False);
  Check('0.7.0-beta.3', True, False);
  Check('0.7.0', True, False);
  Check('0.7.1', True, False);
  Check('1.0.0', True, False);
  Log('Legacy driver cleanup regression checks passed');
  Result := False;
end;
