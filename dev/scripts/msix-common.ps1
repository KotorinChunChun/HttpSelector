#Requires -Version 7.4
Set-StrictMode -Version Latest

function Invoke-CheckedNative {
    param([string]$File, [string[]]$Arguments)
    & $File @Arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "外部コマンドが失敗しました: $([IO.Path]::GetFileName($File)) (exit=$LASTEXITCODE)" }
}

function Get-MsixContext {
    param([string]$Root, [ValidateSet('beta','stable')][string]$Channel)
    $config = Get-Content -LiteralPath (Join-Path $Root 'packaging/release-config.json') -Raw | ConvertFrom-Json
    if ($config.schemaVersion -ne 1 -or $config.architecture -ne 'x64') { throw '未対応のリリース構成です。' }
    if ($config.packageVersion -notmatch '^[1-9][0-9]*\.[0-9]+\.[0-9]+\.0$') { throw 'MSIXバージョンは先頭1以上・4桁目0の4部形式にしてください。' }
    $version = [version]$config.packageVersion
    foreach ($part in @($version.Major,$version.Minor,$version.Build,$version.Revision)) {
        if ($part -gt 65535) { throw 'MSIXバージョンの各部は65535以下にしてください。' }
    }
    $identity = $config.beta
    if ($Channel -eq 'stable') {
        $identityPath = [IO.Path]::GetFullPath((Join-Path $Root $config.stable.identityFile))
        if (-not $identityPath.StartsWith($Root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Identity設定はプロジェクト内に配置してください。' }
        if (-not (Test-Path -LiteralPath $identityPath)) { throw 'Partner Centerの正式Identityが未設定です。store-identity.example.jsonをstore-identity.local.jsonへコピーして設定してください。' }
        $identity = Get-Content -LiteralPath $identityPath -Raw | ConvertFrom-Json
    }
    foreach ($key in @('name','publisher','publisherDisplayName','displayName')) {
        if (-not $identity.PSObject.Properties[$key] -or [string]::IsNullOrWhiteSpace($identity.$key) -or $identity.$key -match '@@|TODO|REPLACE|[<>]') { throw "Identityの必須項目が未設定です: $key" }
    }
    if ($identity.name -notmatch '^[A-Za-z0-9.-]{3,50}$' -or $identity.publisher -notmatch '^CN=') { throw 'NameまたはPublisherの形式が不正です。Partner Centerの値を使用してください。' }
    if ($Channel -eq 'stable' -and ($identity.name -eq $config.beta.name -or $identity.publisher -eq $config.beta.publisher)) { throw 'Store提出に開発用Identityは使用できません。' }
    $suffix = if ($Channel -eq 'beta') { 'dev' } else { 'store-unsigned' }
    [pscustomobject]@{
        Config = $config; Identity = $identity; Channel = $Channel; Version = $version.ToString()
        FileName = "HttpSelector_$($version)_x64_$suffix.msix"
        Output = Join-Path $Root "release/msix/$Channel/$version"
    }
}

function Find-WindowsSdkTool {
    param([string]$Name)
    $base = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
    $versions = Get-ChildItem -LiteralPath $base -Directory | Where-Object Name -Match '^10\.0\.\d+\.0$' | Sort-Object { [version]$_.Name } -Descending
    foreach ($version in $versions) {
        $tool = Join-Path $version.FullName "x64/$Name"
        if (Test-Path -LiteralPath $tool) { return $tool }
    }
    throw "Windows SDKの$Name が見つかりません。Windows SDKを導入してください。"
}

function Write-MsixManifest {
    param([string]$Root, $Context, [string]$Destination)
    $manifest = Get-Content -LiteralPath (Join-Path $Root 'packaging/AppxManifest.xml.template') -Raw
    $values = @{
        Name=$Context.Identity.name; Publisher=$Context.Identity.publisher
        DisplayName=$Context.Identity.displayName; PublisherDisplayName=$Context.Identity.publisherDisplayName
        Version=$Context.Version; MinVersion=$Context.Config.minimumOSVersion; MaxVersion=$Context.Config.maximumTestedOSVersion
    }
    foreach ($key in $values.Keys) { $manifest = $manifest.Replace("@@$key@@", [Security.SecurityElement]::Escape([string]$values[$key])) }
    if ($manifest -match '@@') { throw 'Manifestに未解決の置換項目があります。' }
    [xml]$manifest | Out-Null
    [IO.File]::WriteAllText($Destination, $manifest, [Text.UTF8Encoding]::new($false))
}

function Get-DevelopmentCertificate {
    param($Identity, [switch]$CreateIfMissing)
    $certificates = @(Get-ChildItem Cert:/CurrentUser/My | Where-Object { $_.FriendlyName -eq $Identity.certificateFriendlyName })
    if ($certificates.Count -gt 1) { throw '開発用証明書が複数あります。証明書の移行方針を確認してください。' }
    if ($certificates.Count -eq 0) {
        if (-not $CreateIfMissing) { return $null }
        $certificate = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Identity.publisher `
            -FriendlyName $Identity.certificateFriendlyName -CertStoreLocation Cert:/CurrentUser/My `
            -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -KeyExportPolicy NonExportable `
            -NotAfter (Get-Date).AddMonths(6)
    } else { $certificate = $certificates[0] }
    if ($certificate.Subject -ne $Identity.publisher -or -not $certificate.HasPrivateKey `
        -or $certificate.NotBefore -gt (Get-Date) -or $certificate.NotAfter -le (Get-Date).AddDays(7) `
        -or '1.3.6.1.5.5.7.3.3' -notin @($certificate.EnhancedKeyUsageList.ObjectId.Value)) {
        throw '開発用証明書がIdentity・秘密鍵・有効期限・Code Signing要件を満たしません。自動再発行はしません。'
    }
    $certificate
}

function Read-ZipEntryBytes {
    param($Entry)
    $stream = $Entry.Open(); $memory = [IO.MemoryStream]::new()
    try { $stream.CopyTo($memory); return ,$memory.ToArray() }
    finally { $stream.Dispose(); $memory.Dispose() }
}

function Assert-NoDeveloperPaths {
    param([byte[]]$Bytes)
    $text8 = [Text.Encoding]::UTF8.GetString($Bytes)
    $text16 = [Text.Encoding]::Unicode.GetString($Bytes)
    # 配布物にはソースの絶対パスやビルド担当者名を残さない。
    foreach ($text in @($text8, $text16)) {
        if ($text -match '(?i)[A-Z]:[\\/]Users[\\/]' -or $text -match '(?i)[A-Z]:[\\/](develop|repos|agent|work)[\\/]') { throw '配布物に開発PCの絶対パスが含まれています。' }
        foreach ($path in @($env:USERPROFILE, $env:CARGO_HOME, $env:RUSTUP_HOME)) {
            if ($path -and ($text.Contains($path, [StringComparison]::OrdinalIgnoreCase) -or $text.Contains($path.Replace('\','/'), [StringComparison]::OrdinalIgnoreCase))) { throw '配布物にビルド環境の個人パスが含まれています。' }
        }
    }
}

function Test-MsixArtifact {
    param([string]$Path, $Context)
    if (-not (Test-Path -LiteralPath $Path)) { throw '検証対象MSIXがありません。' }
    $zip = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $payload = @('HttpSelector.exe','Assets/StoreLogo.png','Assets/Square44x44Logo.png','Assets/Square150x150Logo.png','AppxManifest.xml')
        $metadata = @('AppxBlockMap.xml','[Content_Types].xml')
        if ($Context.Channel -eq 'beta') { $metadata += 'AppxSignature.p7x' }
        $entries = @{}
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('\','/')
            if ($name -notin ($payload + $metadata) -or $entries.ContainsKey($name)) { throw "パッケージに未許可または重複ファイルがあります: $name" }
            $entries[$name] = Read-ZipEntryBytes $entry
            if ($name -in $payload) { Assert-NoDeveloperPaths $entries[$name] }
        }
        foreach ($name in ($payload + $metadata)) { if (-not $entries.ContainsKey($name)) { throw "パッケージファイルがありません: $name" } }
        [xml]$manifest = [Text.Encoding]::UTF8.GetString($entries['AppxManifest.xml'])
        $id = $manifest.Package.Identity
        if ($id.Name -cne $Context.Identity.name -or $id.Publisher -cne $Context.Identity.publisher -or $id.Version -ne $Context.Version -or $id.ProcessorArchitecture -ne 'x64') { throw 'MSIX Identityが設定と一致しません。' }
        if ($manifest.Package.Properties.PublisherDisplayName -cne $Context.Identity.publisherDisplayName -or $manifest.Package.Properties.DisplayName -cne $Context.Identity.displayName) { throw 'MSIXの公開表示名が設定と一致しません。' }
        $app = $manifest.Package.Applications.Application
        if ($app.Id -ne 'App' -or $app.Executable -ne 'HttpSelector.exe' -or $app.EntryPoint -ne 'Windows.FullTrustApplication') { throw 'アプリの起動設定が不正です。' }
        $protocols = @($app.Extensions.Extension.Protocol)
        if (($protocols.Name | Sort-Object) -join ',' -ne 'http,https') { throw 'HTTP/HTTPSの候補登録が不正です。' }
        foreach ($protocol in $protocols) { if ($protocol.Parameters -cne '"%1"') { throw 'URL起動引数が引用されていません。' } }
        if (@($manifest.Package.Capabilities.ChildNodes).Count -ne 1 -or $manifest.Package.Capabilities.Capability.Name -ne 'runFullTrust') { throw '許可していないCapabilityがあります。' }
        [xml]$blockMap = [Text.Encoding]::UTF8.GetString($entries['AppxBlockMap.xml'])
        if ($blockMap.BlockMap.HashMethod -ne 'http://www.w3.org/2001/04/xmlenc#sha256') { throw 'BlockMapのハッシュ方式が不正です。' }
        $seen = @{}
        foreach ($file in $blockMap.BlockMap.File) {
            $name = $file.Name.Replace('\','/')
            if ($name -notin $payload -or $seen.ContainsKey($name)) { throw 'BlockMapのファイル構成が不正です。' }
            $seen[$name] = $true
            [byte[]]$bytes = $entries[$name]
            if ($bytes.Length -ne [long]$file.Size) { throw "BlockMapのサイズが不一致: $name" }
            $offset = 0
            foreach ($block in $file.Block) {
                $count = [Math]::Min(65536, $bytes.Length - $offset)
                if ($count -le 0) { throw 'BlockMapのブロック数が不正です。' }
                $hash = [Security.Cryptography.SHA256]::HashData([byte[]]$bytes[$offset..($offset+$count-1)])
                if ([Convert]::ToBase64String($hash) -cne $block.Hash) { throw "BlockMapのハッシュが不一致: $name" }
                $offset += $count
            }
            if ($offset -ne $bytes.Length) { throw 'BlockMapに未検証の領域があります。' }
        }
        if ($seen.Count -ne $payload.Count) { throw 'BlockMapに未登録のファイルがあります。' }
        $signer = $null; $trust = 'UnsignedForStore'
        if ($Context.Channel -eq 'beta') {
            $cms = [Security.Cryptography.Pkcs.SignedCms]::new()
            [byte[]]$signatureBytes = $entries['AppxSignature.p7x']
            if ([Text.Encoding]::ASCII.GetString($signatureBytes,0,4) -ne 'PKCX') { throw 'MSIX署名形式が不正です。' }
            $cms.Decode([byte[]]$signatureBytes[4..($signatureBytes.Length-1)])
            $cms.CheckSignature($true)
            if ($cms.SignerInfos.Count -ne 1) { throw '署名者数が不正です。' }
            $signer = $cms.SignerInfos[0].Certificate
            $cerPath = Join-Path ([IO.Path]::GetDirectoryName($Path)) 'HttpSelector.Development.cer'
            $cer = [Security.Cryptography.X509Certificates.X509Certificate2]::new($cerPath)
            try {
                if ($signer.Thumbprint -ne $cer.Thumbprint -or $signer.Subject -cne $Context.Identity.publisher) { throw '署名者・配布CER・Publisherが一致しません。' }
            } finally { $cer.Dispose() }
            if ($signer.NotAfter -le (Get-Date) -or $signer.NotBefore -gt (Get-Date)) { throw '署名証明書が有効期間外です。' }
            $auth = Get-AuthenticodeSignature -LiteralPath $Path
            if (-not $auth.SignerCertificate -or $auth.SignerCertificate.Thumbprint -ne $signer.Thumbprint) { throw 'Windowsが認識した署名者と一致しません。' }
            if ($auth.Status -eq 'Valid') { $trust = 'Trusted' }
            elseif ($auth.Status -in @('UnknownError','NotTrusted')) {
                # SignToolでも許容するエラーを未信頼ルートの1件だけに限定する。
                $verifyText = (& (Find-WindowsSdkTool 'signtool.exe') verify /pa /v $Path 2>&1 | Out-String)
                if ($LASTEXITCODE -ne 1 -or $verifyText -notmatch '(?i)0x800B0109|root certificate which is not trusted|信頼されていないルート証明書' -or $verifyText -notmatch 'Number of errors:\s+1') { throw "Windows署名検証に予期しない失敗があります: $($auth.Status)" }
                $trust = 'UntrustedDevelopmentCertificate'
            } else { throw "Windows署名検証に失敗しました: $($auth.Status)" }
        }
        [pscustomobject]@{
            File = [IO.Path]::GetFileName($Path); Channel = $Context.Channel; Version = $Context.Version
            IdentityName = $id.Name; Publisher = $id.Publisher; Architecture = $id.ProcessorArchitecture
            SHA256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
            SignatureTrust = $trust; SignerThumbprint = $(if ($signer) { $signer.Thumbprint } else { $null })
            CertificateExpires = $(if ($signer) { $signer.NotAfter.ToUniversalTime().ToString('o') } else { $null })
            PayloadAndBlockMap = 'Passed'; DeveloperPathScan = 'Passed'
        }
    } finally { $zip.Dispose() }
}
