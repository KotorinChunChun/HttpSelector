#Requires -Version 7.4
# MSIX生成・秘密鍵輸出・ストア変更をしない署名方針の回帰検証。
[CmdletBinding()]
param([switch]$CheckLocalCertificate)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'msix-common.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\', '/')
$script:checks = 0
function Assert-Policy([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "署名方針の検証失敗: $Message" }
    $script:checks++
}
function Assert-PolicyRejected([scriptblock]$Action, [string]$Expected) {
    $message = ''
    try { & $Action | Out-Null } catch { $message = $_.Exception.Message }
    Assert-Policy ($message.Contains($Expected)) "拒否理由: $Expected / $message"
}
foreach ($name in @('MsixSigning.psm1', 'msix-common.ps1', 'release-msix.ps1', 'test-msix-signing-policy.ps1')) {
    $tokens = $null; $errors = $null
    [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $name), [ref]$tokens, [ref]$errors) | Out-Null
    Assert-Policy ($errors.Count -eq 0) "構文: $name"
}
$context = Get-MsixContext -Root $root -Channel beta
Assert-Policy ($context.Identity.name -ceq 'HttpSelector.Development') 'アプリ固有Nameを維持'
Assert-Policy ($context.Identity.publisher -ceq 'CN=KotorinChunChun') '共通Publisher'
Assert-Policy ($context.Config.stable.identityFile -ceq 'packaging/store-identity.local.json') 'Store Identity経路を維持'
$entry = Join-Path $PSScriptRoot 'release-msix.ps1'
Assert-PolicyRejected { & $entry -Mode Build -Channel beta } 'ReleaseIntentConfirmed'
Assert-PolicyRejected { & $entry -Mode Build -ReleaseIntentConfirmed } 'Channel'
$plan = & $entry -Mode Plan -Channel beta
Assert-Policy ($plan.Publisher -ceq 'CN=KotorinChunChun') 'Planが共通Publisherを表示'
Assert-Policy ($plan.Signing -like '*PC共通証明書*') 'Planが共通設定を表示'
if ($CheckLocalCertificate) {
    $profile = Read-MsixSigningProfile
    $certificate = Get-DevelopmentCertificate -Identity $context.Identity
    try { Assert-Policy ($certificate.Thumbprint -ceq $profile.thumbprint) '実証明書とPC共通拇印の一致' }
    finally { $certificate.Dispose() }
    Assert-PolicyRejected { Get-DevelopmentCertificate -Identity ([pscustomobject]@{publisher='CN=HttpSelector Development'}) } 'Publisher'
}
Write-Output "HttpSelector共通MSIX署名方針: $script:checks 件成功（ビルド・署名・インストールなし）"
