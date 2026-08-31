#Requires -Version 7.4
[CmdletBinding()]
param([ValidateSet('beta')][string]$Channel = 'beta')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-site-common.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
$context = Get-DistributionContext -Root $root -Channel $Channel
$result = Assert-DistributionArtifacts -Context $context -Staged
if ((Get-FileHash $context.PackagePath).Hash -ne (Get-FileHash $context.StagedPackagePath).Hash -or (Get-FileHash $context.CertificatePath).Hash -ne (Get-FileHash $context.StagedCertificatePath).Hash -or (Get-FileHash $context.AppInstallerPath).Hash -ne (Get-FileHash $context.StagedAppInstallerPath).Hash) { throw 'ステージ済み成果物が生成元と一致しません。' }
foreach ($source in Get-ChildItem -LiteralPath $context.SiteSourceDirectory -File -Recurse -Force) {
    $relative = [IO.Path]::GetRelativePath($context.SiteSourceDirectory, $source.FullName)
    $staged = Join-Path $context.SiteRoot $relative
    if (-not (Test-Path -LiteralPath $staged -PathType Leaf)) { throw "公開ページがありません: $relative" }
    if ($relative -eq '.htaccess') {
        if ((Get-Content $source.FullName -Raw).Replace("`r`n","`n") -ne (Get-Content $staged -Raw).Replace("`r`n","`n")) { throw '既存.htaccessの内容が公開設定と一致しません。' }
    } elseif ((Get-FileHash $source.FullName).Hash -ne (Get-FileHash $staged).Hash) { throw "公開ページがステージ元と一致しません: $relative" }
}
$styles = Join-Path $context.SiteRoot 'assets/styles.css'
if (-not (Test-Path -LiteralPath $styles -PathType Leaf)) { throw '共有スタイルシートがありません。' }
foreach ($htmlFile in @('index.html','HttpSelector.html','httpselector-certificate/index.html')) {
    $path = Join-Path $context.SiteRoot $htmlFile
    $html = Get-Content -LiteralPath $path -Raw
    if ($html -notmatch '<html\s+lang="ja"' -or $html -notmatch '<meta\s+name="viewport"' -or [regex]::Matches($html,'<h1(?:\s[^>]*)?>').Count -ne 1) { throw "HTML構造が不正です: $htmlFile" }
    foreach ($match in [regex]::Matches($html,'(?:href|src)="([^"]+)"')) {
        $reference = $match.Groups[1].Value
        if ($reference -match '^(https?://|#|mailto:)') { continue }
        $clean = ($reference -split '[?#]',2)[0]
        if (-not $clean) { continue }
        $target = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $path) $clean))
        if (Test-Path -LiteralPath $target -PathType Container) { $target = Join-Path $target 'index.html' }
        if (-not $target.StartsWith($context.SiteRoot, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "ローカルリンクが切れています: $htmlFile -> $reference" }
    }
}
$htaccess = Get-Content -LiteralPath (Join-Path $context.SiteRoot '.htaccess') -Raw
foreach ($mapping in @('application/msix .msix','application/appinstaller .appinstaller','application/pkix-cert .cer')) { if (-not $htaccess.Contains($mapping)) { throw "MIME設定がありません: $mapping" } }
$private = Get-ChildItem -LiteralPath $context.SiteRoot -File -Recurse -Force | Where-Object { $_.Extension -in @('.pfx','.p12','.key') -or $_.Name -eq '.env' }
if ($private) { throw '公開領域に秘密鍵または環境設定があります。' }
Write-Output 'ステージ済みHttpSelector公開物の検証に成功しました。'
$result
