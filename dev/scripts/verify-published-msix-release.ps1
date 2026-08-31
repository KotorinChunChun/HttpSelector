#Requires -Version 7.4
[CmdletBinding()]
param([ValidateSet('beta')][string]$Channel = 'beta')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-site-common.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
$context = Get-DistributionContext -Root $root -Channel $Channel
$local = Assert-DistributionArtifacts -Context $context -Staged
$client = [Net.Http.HttpClient]::new()
try {
    foreach ($check in @(
        @($context.PublicDownloadPageUri,'text/html'), @($context.ProductPageUri,'text/html'), @($context.CertificateGuideUri,'text/html'), @($context.IconUri,'image/png'),
        @([uri]$context.AppInstallerUri,'application/appinstaller'), @([uri]$context.PackageUri,'application/msix'), @([uri]$context.CertificateUri,'application/pkix-cert')
    )) {
        $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Head,[uri]$check[0])
        try { $response = $client.SendAsync($request).GetAwaiter().GetResult(); try { if (-not $response.IsSuccessStatusCode) { throw "公開URLがHTTP $([int]$response.StatusCode)を返しました: $($check[0])" }; if ($response.Content.Headers.ContentType.MediaType -ne $check[1]) { throw "Content-Typeが不正です: $($check[0])" }; if (-not $response.Content.Headers.ContentLength -or $response.Content.Headers.ContentLength -le 0) { throw "Content-Lengthが不正です: $($check[0])" }; Write-Output "HTTP OK: $($check[0]) [$($check[1])] $($response.Content.Headers.ContentLength) bytes" } finally { $response.Dispose() } } finally { $request.Dispose() }
    }
    $packageBytes = $client.GetByteArrayAsync($context.PackageUri).GetAwaiter().GetResult()
    $certificateBytes = $client.GetByteArrayAsync($context.CertificateUri).GetAwaiter().GetResult()
    $appInstallerBytes = $client.GetByteArrayAsync($context.AppInstallerUri).GetAwaiter().GetResult()
    $packageHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($packageBytes))
    $certificateHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($certificateBytes))
    if ($packageHash -ne $local.PackageSHA256 -or $certificateHash -ne $local.CertificateSHA256) { throw '公開成果物のSHA-256がステージ済み成果物と一致しません。' }
    [xml]$appInstaller = [Text.Encoding]::UTF8.GetString($appInstallerBytes)
    $rootNode = $appInstaller.SelectSingleNode("/*[local-name()='AppInstaller']")
    $main = $appInstaller.SelectSingleNode("/*[local-name()='AppInstaller']/*[local-name()='MainPackage']")
    if (-not $rootNode -or -not $main -or $rootNode.GetAttribute('Uri') -ne $context.AppInstallerUri -or $main.GetAttribute('Uri') -ne $context.PackageUri -or $main.GetAttribute('Version') -ne $context.Msix.Version) { throw '公開App Installerの参照情報が不正です。' }
    $page = $client.GetStringAsync($context.ProductPageUri).GetAwaiter().GetResult()
    if (-not $page.Contains($context.Msix.Version) -or -not $page.Contains($context.Msix.FileName) -or -not $page.Contains($context.Config.beta.appInstallerFileName)) { throw '公開製品ページに現行版へのリンクがありません。' }
    $menu = $client.GetStringAsync($context.PublicDownloadPageUri).GetAwaiter().GetResult()
    if (-not $menu.Contains('HttpSelector.html') -or -not $menu.Contains('httpselector-icon.png')) { throw '公開ダウンロードメニューにHttpSelectorがありません。' }
    $guide = $client.GetStringAsync($context.CertificateGuideUri).GetAwaiter().GetResult()
    if (-not $guide.Contains('475B ABC3 C3D1 E871 D905 03D4 5CC5 2362 16D8 5EE8') -or -not $guide.Contains('HttpSelector.Development.cer') -or -not $guide.Contains('HttpSelector.appinstaller')) { throw '公開証明書ページの確認情報またはリンクが不正です。' }
    Write-Output "公開後検証に成功しました。MSIX SHA-256: $packageHash"
} finally { $client.Dispose() }
