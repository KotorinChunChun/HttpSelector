#Requires -Version 7.4
[CmdletBinding()]
param([ValidateSet('beta')][string]$Channel = 'beta')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-site-common.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
$context = Get-DistributionContext -Root $root -Channel $Channel
& (Join-Path $PSScriptRoot 'release-msix.ps1') -Mode Verify -Channel $Channel
$identity = Get-PackageIdentity -Path $context.PackagePath
$template = Get-Content -LiteralPath (Join-Path $root 'packaging/AppInstaller.xml.template') -Raw
$values = @{ NAME=$identity.Name; PUBLISHER=$identity.Publisher; VERSION=$identity.Version; ARCHITECTURE=$identity.Architecture; APPINSTALLER_URI=$context.AppInstallerUri; PACKAGE_URI=$context.PackageUri }
foreach ($key in $values.Keys) { $template = $template.Replace("@@$key@@", [Security.SecurityElement]::Escape([string]$values[$key])) }
if ($template -match '@@') { throw 'App Installerテンプレートに未解決項目があります。' }
[xml]$template | Out-Null
[IO.File]::WriteAllText($context.AppInstallerPath, $template, [Text.UTF8Encoding]::new($false))
$result = Assert-DistributionArtifacts -Context $context
Write-Output "App Installerを生成しました: $($context.AppInstallerPath)"
$result
