#Requires -Version 7.4
[CmdletBinding()]
param([ValidateSet('beta')][string]$Channel = 'beta')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-site-common.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
$context = Get-DistributionContext -Root $root -Channel $Channel
Assert-DistributionArtifacts -Context $context | Out-Null
if (-not (Test-Path -LiteralPath $context.SiteSourceDirectory -PathType Container)) { throw '公開ページのソースがありません。' }
[IO.Directory]::CreateDirectory($context.SiteRoot) | Out-Null
foreach ($source in Get-ChildItem -LiteralPath $context.SiteSourceDirectory -File -Recurse -Force) {
    $relative = [IO.Path]::GetRelativePath($context.SiteSourceDirectory, $source.FullName)
    $destination = [IO.Path]::GetFullPath((Join-Path $context.SiteRoot $relative))
    if (-not $destination.StartsWith($context.SiteRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'サイト出力先の外へコピーしようとしました。' }
    [IO.Directory]::CreateDirectory((Split-Path -Parent $destination)) | Out-Null
    Copy-Item -LiteralPath $source.FullName -Destination $destination -Force
}
[IO.Directory]::CreateDirectory($context.ChannelSiteDirectory) | Out-Null
Copy-Item -LiteralPath $context.PackagePath -Destination $context.StagedPackagePath -Force
Copy-Item -LiteralPath $context.CertificatePath -Destination $context.StagedCertificatePath -Force
Copy-Item -LiteralPath $context.AppInstallerPath -Destination $context.StagedAppInstallerPath -Force
Write-Output "公開サイトへステージしました: $($context.SiteRoot)"
