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
    # 共有メニューは他アプリの編集中データを保持する。公開対象に指定した場合だけ更新する。
    if ($relative -eq 'index.html' -and 'index.html' -notin $context.Distribution.publishRelativePaths) { continue }
    $destination = [IO.Path]::GetFullPath((Join-Path $context.SiteRoot $relative))
    if (-not $destination.StartsWith($context.SiteRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'サイト出力先の外へコピーしようとしました。' }
    if ($relative -eq '.htaccess' -and (Test-Path -LiteralPath $destination -PathType Leaf)) {
        $sourceText = (Get-Content -LiteralPath $source.FullName -Raw).Replace("`r`n","`n")
        $destinationText = (Get-Content -LiteralPath $destination -Raw).Replace("`r`n","`n")
        if ($sourceText -ne $destinationText) { throw '既存.htaccessのMIME・キャッシュ設定が公開設定と一致しません。' }
        continue
    }
    [IO.Directory]::CreateDirectory((Split-Path -Parent $destination)) | Out-Null
    Copy-Item -LiteralPath $source.FullName -Destination $destination -Force
}
[IO.Directory]::CreateDirectory($context.ChannelSiteDirectory) | Out-Null
Copy-Item -LiteralPath $context.PackagePath -Destination $context.StagedPackagePath -Force
Copy-Item -LiteralPath $context.CertificatePath -Destination $context.StagedCertificatePath -Force
Copy-Item -LiteralPath $context.AppInstallerPath -Destination $context.StagedAppInstallerPath -Force
Write-Output "公開サイトへステージしました: $($context.SiteRoot)"
