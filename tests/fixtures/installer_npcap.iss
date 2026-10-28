[Setup]
AppName=Npcap architecture regression checks
AppVersion=1.0
DefaultDirName={tmp}\netauth-npcap-check
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
DisableWelcomePage=yes
DisableReadyPage=yes
DisableFinishedPage=yes
OutputBaseFilename=installer-npcap-check-{#MyAppArch}

[Code]
#define NpcapUrl "https://npcap.com/#download"
#include "../../apps/rust/scripts/npcap.iss"

procedure CheckLibrary(Name: String; Expected: Boolean);
begin
  if NpcapLibraryMatches('{#TestFixtureDir}\' + Name + '.dll') <> Expected then
    RaiseException('Incorrect Npcap compatibility result: ' + Name);
end;

function InitializeSetup: Boolean;
begin
#if MyAppArch == "arm64"
  CheckLibrary('arm64', True);
  CheckLibrary('arm64x', True);
  CheckLibrary('x64', False);
#else
  CheckLibrary('arm64', False);
  CheckLibrary('arm64x', False);
  CheckLibrary('x64', True);
#endif
  CheckLibrary('x86', False);
  CheckLibrary('truncated', False);
  CheckLibrary('bad-offset', False);
  CheckLibrary('bad-signature', False);
  CheckLibrary('missing', False);
  Log('Npcap architecture regression checks passed');
  Result := True;
end;
