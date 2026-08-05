param(
    [string]$Url = "https://example.com/path?a=b%20c"
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..\")
Push-Location $ProjectRoot
try {
    cargo run -- $Url
} finally {
    Pop-Location
}

