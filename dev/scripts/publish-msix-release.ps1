#Requires -Version 7.4
[CmdletBinding()]
param(
    [ValidateSet('Plan','Build','Stage','Publish','Verify','Full')][string]$Mode = 'Plan',
    [ValidateSet('beta','stable')][string]$Channel,
    [string]$PythonPath,
    [switch]$ReleaseIntentConfirmed,
    [switch]$PublishConfirmed,
    [switch]$AllowRebuild
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-site-common.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
if ($Mode -ne 'Plan' -and -not $Channel) { throw 'Channelをbeta/stableで指定してください。' }
if ($Mode -in @('Build','Stage','Publish','Full') -and -not $ReleaseIntentConfirmed) { throw "$Mode には明示的な -ReleaseIntentConfirmed が必要です。" }
if ($Mode -in @('Publish','Full') -and -not $PublishConfirmed) { throw "$Mode には明示的な -PublishConfirmed が必要です。" }
if ($AllowRebuild -and $Mode -notin @('Build','Full')) { throw '-AllowRebuildはBuildまたはFull専用です。' }
$releaseConfig = Get-Content -LiteralPath (Join-Path $root 'packaging/release-config.json') -Raw | ConvertFrom-Json
if ($releaseConfig.distribution.PSObject.Properties['mode'] -and $releaseConfig.distribution.mode -eq 'github' -and $Mode -in @('Stage','Publish','Verify','Full')) {
    throw '通常配布はGitHubへ移行済みです。docs/GITHUB_RELEASE.mdを参照してください。旧サイトは移行用3ファイルだけを更新します。'
}

function Invoke-Build([string]$SelectedChannel) {
    $context = Get-DistributionContext -Root $root -Channel $SelectedChannel
    if (Test-Path -LiteralPath $context.PackagePath -PathType Leaf) {
        & (Join-Path $PSScriptRoot 'release-msix.ps1') -Mode Verify -Channel $SelectedChannel
    } else {
        & (Join-Path $PSScriptRoot 'release-msix.ps1') -Mode Build -Channel $SelectedChannel -ReleaseIntentConfirmed -AllowRebuild:$AllowRebuild
    }
    & (Join-Path $PSScriptRoot 'build-appinstaller.ps1') -Channel $SelectedChannel
}
function Invoke-Stage([string]$SelectedChannel) {
    & (Join-Path $PSScriptRoot 'stage-msix-release.ps1') -Channel $SelectedChannel
    & (Join-Path $PSScriptRoot 'verify-staged-msix-release.ps1') -Channel $SelectedChannel
}
function Resolve-Python {
    $command = Get-Command python -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $candidate = if ($PythonPath) { [IO.Path]::GetFullPath($PythonPath) } elseif ($command) { $command.Source } else { $null }
    if (-not $candidate -or -not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw 'FTP公開に使用するPythonの実体が見つかりません。-PythonPathで実行ファイルを指定してください。' }
    & $candidate --version 2>$null | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'PATH上のpythonは実行できません。-PythonPathで実体のあるPythonを指定してください。' }
    $candidate
}
function Invoke-Publish([string]$SelectedChannel) {
    $context = Get-DistributionContext -Root $root -Channel $SelectedChannel
    & (Join-Path $PSScriptRoot 'verify-staged-msix-release.ps1') -Channel $SelectedChannel
    if (-not (Test-Path -LiteralPath $context.FtpUploadScript -PathType Leaf)) { throw 'FTPアップロードスクリプトがありません。' }
    $python = Resolve-Python
    Invoke-CheckedProcess -FilePath $python -ArgumentList @($context.FtpUploadScript,'--dry-run') -WorkingDirectory $root
    Write-Output 'FTPS dry-runが成功しました。公開前バックアップを取得し、対象を限定して公開します。'
    Invoke-CheckedProcess -FilePath $python -ArgumentList @($context.FtpUploadScript,'--execute') -WorkingDirectory $root
    & (Join-Path $PSScriptRoot 'verify-published-msix-release.ps1') -Channel $SelectedChannel
}

switch ($Mode) {
    'Plan' {
        if (-not $Channel) { Write-Output 'beta: GitHub ReleasesのApp Installer配布 / stable: Microsoft Store'; Write-Output 'Planは生成・ステージ・公開・インストールを行いません。'; break }
        if ($Channel -eq 'stable') { & (Join-Path $PSScriptRoot 'release-msix.ps1') -Mode Plan -Channel stable; break }
        $context = Get-DistributionContext -Root $root -Channel $Channel
        [pscustomobject]@{ Channel=$Channel; Version=$context.Msix.Version; Identity=$context.Msix.Identity.name; Publisher=$context.Msix.Identity.publisher; AppInstaller=$context.AppInstallerUri; Package=$context.PackageUri; ProductPage=$context.ProductPageUri; Installation='実行しない'; Publication='実行しない' }
    }
    'Build' { Invoke-Build $Channel }
    'Stage' { Invoke-Stage $Channel }
    'Publish' { Invoke-Publish $Channel }
    'Verify' { & (Join-Path $PSScriptRoot 'verify-staged-msix-release.ps1') -Channel $Channel; & (Join-Path $PSScriptRoot 'verify-published-msix-release.ps1') -Channel $Channel }
    'Full' { Invoke-Build $Channel; Invoke-Stage $Channel; Invoke-Publish $Channel }
}
