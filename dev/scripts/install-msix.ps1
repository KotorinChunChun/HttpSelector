#Requires -Version 7.4
[CmdletBinding()]
param([switch]$Install)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'msix-common.ps1')
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
$context = Get-MsixContext -Root $projectRoot -Channel beta
$package = Join-Path $context.Output $context.FileName
$verification = Test-MsixArtifact -Path $package -Context $context
if ($verification.SignatureTrust -ne 'Trusted') { throw 'Windowsが署名を信頼していません。配布元・公開CERを確認してから導入してください。' }
$installed = @(Get-AppxPackage -Name $context.Identity.name)
foreach ($prior in $installed) {
    if ($prior.Publisher -cne $context.Identity.publisher) { throw '導入済みMSIXのPublisherが一致しません。' }
    if ([version]$prior.Version -gt [version]$context.Version) { throw 'ダウングレードは実行しません。設定を退避したうえで個別に移行してください。' }
}
if (-not $Install) {
    [pscustomobject]@{ Package=$package; Version=$context.Version; Signature=$verification.SignatureTrust; SHA256=$verification.SHA256; InstalledVersions=($installed.Version -join ','); Action='検証のみ。-Installで導入する' }
    return
}
Add-AppxPackage -Path $package
$actual = @(Get-AppxPackage -Name $context.Identity.name | Where-Object { $_.Publisher -ceq $context.Identity.publisher -and [version]$_.Version -eq [version]$context.Version })
if ($actual.Count -ne 1) { throw '導入後の版とPublisherを確認できません。' }
$actual | Select-Object Name,Version,PackageFamilyName,Status
Write-Output '署名付きMSIXを導入しました。Windowsの既定のアプリ設定でHTTP/HTTPSをHttpSelectorへ関連付けてください。'
