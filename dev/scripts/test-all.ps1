#Requires -Version 7.4
param(
    [string]$PythonPath,
    [string]$CargoPath,
    [ValidateRange(1,7200)][int]$TimeoutSeconds = 600,
    [switch]$LiveQuad9,
    [switch]$InteractiveDesktop,
    [string]$SignedPackagePath,
    [switch]$ReleaseBuild
)
$ErrorActionPreference = 'Stop'
if (-not $PythonPath) {
    $pythonCommand = Get-Command python -ErrorAction SilentlyContinue
    $bundledPython = Join-Path $env:USERPROFILE '.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe'
    if ($pythonCommand -and $pythonCommand.Source -notlike '*WindowsApps*') { $PythonPath = $pythonCommand.Source }
    elseif (Test-Path -LiteralPath $bundledPython -PathType Leaf) { $PythonPath = $bundledPython }
    else { throw 'Python 3を導入するか、-PythonPathで実行ファイルを指定してください。' }
}
$runnerArguments = @((Join-Path $PSScriptRoot 'test-all.py'), '--pwsh', (Get-Process -Id $PID).Path, '--timeout', "$TimeoutSeconds")
if ($CargoPath) { $runnerArguments += @('--cargo', $CargoPath) }
if ($LiveQuad9) { $runnerArguments += '--live-quad9' }
if ($InteractiveDesktop) { $runnerArguments += '--interactive-desktop' }
if ($SignedPackagePath) { $runnerArguments += @('--signed-package', (Resolve-Path -LiteralPath $SignedPackagePath).Path) }
if ($ReleaseBuild) { $runnerArguments += '--release-build' }
$previousPythonEncoding = [Environment]::GetEnvironmentVariable('PYTHONIOENCODING', 'Process')
try {
    $env:PYTHONIOENCODING = 'utf-8'
    & $PythonPath @runnerArguments
    $testExitCode = $LASTEXITCODE
} finally {
    [Environment]::SetEnvironmentVariable('PYTHONIOENCODING', $previousPythonEncoding, 'Process')
}
if ($testExitCode -ne 0) { throw "一括試験に失敗しました（終了コード $testExitCode）。表示された結果JSONとログを確認してください。" }
