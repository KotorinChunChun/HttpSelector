param(
    [switch]$SkipInstaller
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..\")
Push-Location $ProjectRoot
try {
    cargo fmt --all -- --check
    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo build --release

    if (-not $SkipInstaller) {
        $CompilerCandidates = @(
            (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
            "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
            "C:\Program Files\Inno Setup 6\ISCC.exe"
        )
        $Compiler = $CompilerCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        if (-not $Compiler) {
            throw "Inno Setup 6が見つかりません。-SkipInstallerでexeだけを作成できます。"
        }
        & $Compiler "installer.iss"
    }
} finally {
    Pop-Location
}
