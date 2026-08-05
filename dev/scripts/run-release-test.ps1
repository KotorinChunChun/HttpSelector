param(
    [string]$Url = "https://example.com/path?a=b%20c"
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..\")
$ExePath = Join-Path $ProjectRoot "target\release\HttpSelector.exe"
if (-not (Test-Path -LiteralPath $ExePath)) {
    throw "リリース実行ファイルがありません。先に cargo build --release を実行してください: $ExePath"
}
Start-Process -FilePath $ExePath -ArgumentList $Url

