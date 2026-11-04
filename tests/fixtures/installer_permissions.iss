[Setup]
AppId=NetAuthPermissionsTest
AppName=Installer data permissions regression checks
AppVersion=1.0
DefaultDirName={tmp}\netauth-permissions-check
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64os
Uninstallable=no
CreateAppDir=no
OutputBaseFilename=installer-permissions-check

[Code]
#include "../../scripts/windows/process.iss"
#include "../../scripts/windows/data.iss"

function InitializeSetup: Boolean;
begin
  SecureDataDirectory('{#TestDirectory}');
  Log('Installer data permissions regression checks passed');
  Result := False;
end;
