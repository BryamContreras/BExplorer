#ifndef MyAppVersion
  #define MyAppVersion "1.0.3"
#endif

#define MyAppName "BExplorer"
#define MyAppPublisher "BExplorer"
#define MyAppExeName "BExplorer.exe"
#define ProjectRoot SourcePath + "\..\.."
#define StagedFiles ProjectRoot + "\dist\bexplorer-windows-x86_64-pc-windows-msvc"

[Setup]
AppId={{8E567F0D-BC79-4AB8-956A-20ED0FAEAD95}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\BExplorer
DefaultGroupName=BExplorer
DisableProgramGroupPage=yes
AllowNoIcons=no
LicenseFile={#ProjectRoot}\LICENSE
OutputDir={#ProjectRoot}\dist
OutputBaseFilename=BExplorer-{#MyAppVersion}-Setup-x64
SetupIconFile={#ProjectRoot}\assets\windows\bexplorer.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ShowLanguageDialog=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
CloseApplications=yes
RestartApplications=no
UsePreviousLanguage=no
UsePreviousTasks=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[CustomMessages]
english.AdditionalOptions=Additional options:
spanish.AdditionalOptions=Opciones adicionales:
english.AddToPath=Add BExplorer to the system PATH
spanish.AddToPath=Agregar BExplorer al PATH del sistema

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "{cm:AddToPath}"; GroupDescription: "{cm:AdditionalOptions}"

[Files]
Source: "{#StagedFiles}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\BExplorer"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{group}\{cm:UninstallProgram,BExplorer}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\BExplorer"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,BExplorer}"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

function NormalizePathPart(const Value: String): String;
var
  Normalized: String;
begin
  Normalized := Trim(Value);
  if (Length(Normalized) >= 2) and
     (Normalized[1] = '"') and
     (Normalized[Length(Normalized)] = '"') then
  begin
    Delete(Normalized, Length(Normalized), 1);
    Delete(Normalized, 1, 1);
  end;

  while (Length(Normalized) > 3) and
        (Normalized[Length(Normalized)] = '\') do
    Delete(Normalized, Length(Normalized), 1);

  Result := Lowercase(Normalized);
end;

procedure AppendPathPart(var PathValue: String; const Part: String);
begin
  if Trim(Part) = '' then
    Exit;

  if PathValue <> '' then
    PathValue := PathValue + ';';
  PathValue := PathValue + Trim(Part);
end;

function PathWithoutApp(const OriginalPath, AppPath: String): String;
var
  Remaining: String;
  Part: String;
  Separator: Integer;
begin
  Result := '';
  Remaining := OriginalPath;

  while Remaining <> '' do
  begin
    Separator := Pos(';', Remaining);
    if Separator = 0 then
    begin
      Part := Remaining;
      Remaining := '';
    end
    else
    begin
      Part := Copy(Remaining, 1, Separator - 1);
      Delete(Remaining, 1, Separator);
    end;

    if NormalizePathPart(Part) <> NormalizePathPart(AppPath) then
      AppendPathPart(Result, Part);
  end;
end;

procedure ConfigureSystemPath(const AddApplication: Boolean);
var
  CurrentPath: String;
  NewPath: String;
  AppPath: String;
begin
  AppPath := ExpandConstant('{app}');
  if not RegQueryStringValue(
    HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', CurrentPath) then
    CurrentPath := '';

  NewPath := PathWithoutApp(CurrentPath, AppPath);
  if AddApplication then
    AppendPathPart(NewPath, AppPath);

  if NewPath <> CurrentPath then
  begin
    if not RegWriteExpandStringValue(
      HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', NewPath) then
      Log('BExplorer could not update the system PATH')
    else
      Log('BExplorer updated the system PATH');
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    ConfigureSystemPath(WizardIsTaskSelected('addtopath'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    ConfigureSystemPath(False);
end;
