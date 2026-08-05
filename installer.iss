#define AppName "HttpSelector"
#define AppVersion "0.0.1"
#define AppPublisher "HttpSelector"
#define AppExeName "HttpSelector.exe"

[Setup]
AppId={{99B54D85-F54C-4A40-8D1D-90992A9BFB25}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=release
OutputBaseFilename={#AppName}_v{#AppVersion}_Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#AppExeName}
ChangesAssociations=yes

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Files]
Source: "target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "docs\USERS_GUIDE.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "docs\FAQ.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\アンインストール"; Filename: "{uninstallexe}"

[Registry]
Root: HKCU; Subkey: "Software\Classes\HttpSelector.Url"; ValueType: string; ValueName: ""; ValueData: "HttpSelector URL"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\HttpSelector.Url"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\HttpSelector.Url\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"",0"
Root: HKCU; Subkey: "Software\Classes\HttpSelector.Url\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"" ""%1"""

Root: HKCU; Subkey: "Software\HttpSelector\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\HttpSelector\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "リンクを開くブラウザプロファイルを選択します"
Root: HKCU; Subkey: "Software\HttpSelector\Capabilities\URLAssociations"; ValueType: string; ValueName: "http"; ValueData: "HttpSelector.Url"
Root: HKCU; Subkey: "Software\HttpSelector\Capabilities\URLAssociations"; ValueType: string; ValueName: "https"; ValueData: "HttpSelector.Url"
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "HttpSelector"; ValueData: "Software\HttpSelector\Capabilities"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{#AppName}を起動してプロファイルを確認"; Flags: nowait postinstall skipifsilent unchecked
Filename: "ms-settings:defaultapps?registeredAppUser=HttpSelector"; Description: "Windowsの既定アプリ設定を開く（関連付けは利用者が選択）"; Flags: shellexec postinstall skipifsilent unchecked

