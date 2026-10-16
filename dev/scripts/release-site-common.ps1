#Requires -Version 7.4
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'msix-common.ps1')

function Get-DistributionContext {
    param(
        [Parameter(Mandatory)][string]$Root,
        [ValidateSet('beta','stable')][string]$Channel
    )
    if ($Channel -ne 'beta') { throw 'Web配布はbeta専用です。stableはMicrosoft Storeの経路を使用してください。' }
    $msix = Get-MsixContext -Root $Root -Channel $Channel
    $config = $msix.Config
    if (-not $config.PSObject.Properties['distribution']) { throw 'distribution設定がありません。' }
    $distribution = $config.distribution
    foreach ($key in @('siteSourceDirectory','websiteProjectDirectory','siteRelativeDirectory','ftpUploadScript','publicDownloadPage','productPage')) {
        if (-not $distribution.PSObject.Properties[$key] -or [string]::IsNullOrWhiteSpace([string]$distribution.$key)) { throw "distribution.$key が未設定です。" }
    }
    foreach ($key in @('relativeDirectory','baseUri','appInstallerFileName')) {
        if (-not $config.beta.PSObject.Properties[$key] -or [string]::IsNullOrWhiteSpace([string]$config.beta.$key)) { throw "beta.$key が未設定です。" }
    }
    $baseUri = [uri]$config.beta.baseUri
    if (-not $baseUri.IsAbsoluteUri -or $baseUri.Scheme -ne 'https') { throw 'beta.baseUriは絶対HTTPS URLにしてください。' }
    $normalizedBaseUri = $baseUri.AbsoluteUri.TrimEnd('/') + '/'
    $appInstallerUri = $normalizedBaseUri + $config.beta.appInstallerFileName
    if ($config.beta.PSObject.Properties['appInstallerUri']) {
        $appInstallerUri = [uri]$config.beta.appInstallerUri
        if (-not $appInstallerUri.IsAbsoluteUri -or $appInstallerUri.Scheme -ne 'https') { throw 'beta.appInstallerUriは絶対HTTPS URLにしてください。' }
        $appInstallerUri = $appInstallerUri.AbsoluteUri
    }
    $publicDownloadBaseUri = ([uri]$distribution.publicDownloadPage).AbsoluteUri.TrimEnd('/') + '/'
    $websiteRoot = [IO.Path]::GetFullPath($distribution.websiteProjectDirectory).TrimEnd('\','/')
    $siteRoot = [IO.Path]::GetFullPath((Join-Path $websiteRoot $distribution.siteRelativeDirectory))
    if (-not $siteRoot.StartsWith($websiteRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'サイト出力先がWebsiteプロジェクト外です。' }
    $channelSiteDirectory = Join-Path $siteRoot $config.beta.relativeDirectory
    $packagePath = Join-Path $msix.Output $msix.FileName
    $certificateFileName = 'HttpSelector.Development.cer'
    $appInstallerPath = Join-Path $msix.Output $config.beta.appInstallerFileName
    [pscustomobject]@{
        Root = $Root; Msix = $msix; Config = $config; Distribution = $distribution
        WebsiteRoot = $websiteRoot; SiteRoot = $siteRoot
        SiteSourceDirectory = [IO.Path]::GetFullPath((Join-Path $Root $distribution.siteSourceDirectory))
        FtpUploadScript = [IO.Path]::GetFullPath((Join-Path $Root $distribution.ftpsUploadScript))
        ChannelSiteDirectory = $channelSiteDirectory
        PackagePath = $packagePath; CertificatePath = Join-Path $msix.Output $certificateFileName
        AppInstallerPath = $appInstallerPath
        StagedPackagePath = Join-Path $channelSiteDirectory $msix.FileName
        StagedCertificatePath = Join-Path $channelSiteDirectory $certificateFileName
        StagedAppInstallerPath = Join-Path $channelSiteDirectory $config.beta.appInstallerFileName
        PackageUri = $normalizedBaseUri + $msix.FileName
        CertificateUri = $normalizedBaseUri + $certificateFileName
        AppInstallerUri = $appInstallerUri
        ProductPageUri = [uri]$distribution.productPage
        PublicDownloadPageUri = [uri]$distribution.publicDownloadPage
        CertificateGuideUri = [uri]($publicDownloadBaseUri + 'httpselector-certificate/')
        IconUri = [uri]($publicDownloadBaseUri + 'assets/httpselector-icon.png')
    }
}

function Get-PackageIdentity {
    param([Parameter(Mandatory)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "MSIXがありません: $Path" }
    $zip = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $entry = $zip.Entries | Where-Object FullName -EQ 'AppxManifest.xml' | Select-Object -First 1
        if (-not $entry) { throw 'MSIXにAppxManifest.xmlがありません。' }
        $reader = [IO.StreamReader]::new($entry.Open(), [Text.Encoding]::UTF8, $true)
        try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $identity = $manifest.Package.Identity
        [pscustomobject]@{ Name=[string]$identity.Name; Publisher=[string]$identity.Publisher; Version=[string]$identity.Version; Architecture=[string]$identity.ProcessorArchitecture }
    } finally { $zip.Dispose() }
}

function Assert-DistributionArtifacts {
    param([Parameter(Mandatory)]$Context, [switch]$Staged)
    $package = if ($Staged) { $Context.StagedPackagePath } else { $Context.PackagePath }
    $certificate = if ($Staged) { $Context.StagedCertificatePath } else { $Context.CertificatePath }
    $appInstaller = if ($Staged) { $Context.StagedAppInstallerPath } else { $Context.AppInstallerPath }
    foreach ($path in @($package,$certificate,$appInstaller)) { if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -le 0) { throw "配布成果物がありません: $path" } }
    $identity = Get-PackageIdentity -Path $package
    if ($identity.Name -cne $Context.Msix.Identity.name -or $identity.Publisher -cne $Context.Msix.Identity.publisher -or $identity.Version -ne $Context.Msix.Version -or $identity.Architecture -ne 'x64') { throw '配布MSIXのIdentityが設定と一致しません。' }
    $signature = Get-AuthenticodeSignature -LiteralPath $package
    if (-not $signature.SignerCertificate -or $signature.Status -ne 'Valid') { throw "MSIX署名が有効ではありません: $($signature.Status)" }
    $cer = [Security.Cryptography.X509Certificates.X509Certificate2]::new($certificate)
    try { if ($cer.Thumbprint -ne $signature.SignerCertificate.Thumbprint) { throw 'CERとMSIX署名者が一致しません。' } } finally { $cer.Dispose() }
    [xml]$document = Get-Content -LiteralPath $appInstaller -Raw
    $rootNode = $document.SelectSingleNode("/*[local-name()='AppInstaller']")
    $main = $document.SelectSingleNode("/*[local-name()='AppInstaller']/*[local-name()='MainPackage']")
    $onLaunch = $document.SelectSingleNode("/*[local-name()='AppInstaller']/*[local-name()='UpdateSettings']/*[local-name()='OnLaunch']")
    if (-not $rootNode -or -not $main -or -not $onLaunch -or $rootNode.NamespaceURI -ne 'http://schemas.microsoft.com/appx/appinstaller/2021') { throw 'App Installerの構造が不正です。' }
    if ($rootNode.GetAttribute('Uri') -ne $Context.AppInstallerUri -or $main.GetAttribute('Uri') -ne $Context.PackageUri -or $main.GetAttribute('Name') -cne $identity.Name -or $main.GetAttribute('Publisher') -cne $identity.Publisher -or $main.GetAttribute('Version') -ne $identity.Version -or $main.GetAttribute('ProcessorArchitecture') -ne $identity.Architecture) { throw 'App InstallerとMSIXの参照情報が一致しません。' }
    if ($onLaunch.GetAttribute('HoursBetweenUpdateChecks') -ne '0' -or $onLaunch.GetAttribute('ShowPrompt') -ne 'false' -or $onLaunch.GetAttribute('UpdateBlocksActivation') -ne 'false') { throw 'App Installerの更新設定が不正です。' }
    [pscustomobject]@{ Identity=$identity; PackageSHA256=(Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash; CertificateSHA256=(Get-FileHash -LiteralPath $certificate -Algorithm SHA256).Hash; SignerThumbprint=$signature.SignerCertificate.Thumbprint }
}

function Invoke-CheckedProcess {
    param([Parameter(Mandatory)][string]$FilePath, [Parameter(Mandatory)][string[]]$ArgumentList, [Parameter(Mandatory)][string]$WorkingDirectory)
    Push-Location $WorkingDirectory
    try { & $FilePath @ArgumentList | Out-Host; if ($LASTEXITCODE -ne 0) { throw "外部コマンドが失敗しました: $([IO.Path]::GetFileName($FilePath)) (exit=$LASTEXITCODE)" } } finally { Pop-Location }
}
