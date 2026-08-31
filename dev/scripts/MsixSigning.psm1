# PC共通のMSIX署名設定。秘密鍵の作成・輸出・信頼登録は行わない。
#Requires -Version 7.4
Set-StrictMode -Version Latest

function Get-MsixSigningProfilePath {
    [CmdletBinding()]
    param()
    if (-not $IsWindows) { throw 'MSIX署名設定はWindows専用です。' }
    Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'KotorinChunChun/MSIX/signing.json'
}

function Read-MsixSigningProfile {
    [CmdletBinding()]
    param([string]$Path = (Get-MsixSigningProfilePath))
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw 'PC共通のMSIX署名設定がありません。既存証明書を確認してinitialize-msix-signing.ps1で登録してください。自動発行はしません。'
    }
    try { $profile = Get-Content -LiteralPath $Path -Raw -ErrorAction Stop | ConvertFrom-Json -AsHashtable -ErrorAction Stop }
    catch { throw 'PC共通のMSIX署名設定をJSONとして読み込めません。' }
    $fields = @('schemaVersion', 'storeLocation', 'storeName', 'thumbprint', 'publisher')
    if ($profile -isnot [System.Collections.IDictionary] -or $profile.Count -ne $fields.Count) {
        throw 'MSIX署名設定の項目が不正です。秘密情報や追加項目は保存しないでください。'
    }
    foreach ($field in $fields) {
        if (-not $profile.Contains($field)) { throw "MSIX署名設定の必須項目がありません: $field" }
    }
    if ($profile.schemaVersion -ne 1 -or $profile.storeLocation -cne 'CurrentUser' -or $profile.storeName -cne 'My') {
        throw 'MSIX署名設定の形式または証明書ストアが不正です。CurrentUser/Myだけを使用します。'
    }
    if ($profile.thumbprint -isnot [string] -or $profile.thumbprint -cnotmatch '^[A-F0-9]{40}$' -or
        $profile.publisher -isnot [string] -or $profile.publisher -notmatch '^CN=.+') {
        throw 'MSIX署名設定の拇印またはPublisherが不正です。'
    }
    [pscustomobject]$profile
}

function Assert-MsixSigningCertificate {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
        [Parameter(Mandatory)][string]$Publisher,
        [Parameter(Mandatory)][string]$Thumbprint,
        [datetime]$Now = (Get-Date)
    )
    if ($Certificate.Subject -cne $Publisher -or $Certificate.Thumbprint -cne $Thumbprint) {
        throw '共通証明書のPublisherまたは拇印が一致しません。自動切替はしません。'
    }
    if (-not $Certificate.HasPrivateKey) { throw '共通証明書の秘密鍵がありません。CERだけでは署名できません。' }
    if ($Certificate.NotBefore -gt $Now -or $Certificate.NotAfter -le $Now.AddDays(7)) {
        throw '共通証明書が有効期間外、または期限まで7日以下です。更新・信頼移行を確認してください。'
    }
    $usages = @($Certificate.Extensions | Where-Object { $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } |
        ForEach-Object { $_.EnhancedKeyUsages } | ForEach-Object Value)
    if ('1.3.6.1.5.5.7.3.3' -notin $usages) { throw '共通証明書にCode Signing用途がありません。' }
    foreach ($extension in $Certificate.Extensions) {
        if ($extension -is [System.Security.Cryptography.X509Certificates.X509KeyUsageExtension] -and
            -not ($extension.KeyUsages -band [System.Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature)) {
            throw '共通証明書がデジタル署名を許可していません。'
        }
    }
}

function Get-MsixSigningCertificate {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Publisher,
        [string]$ProfilePath = (Get-MsixSigningProfilePath),
        [string]$CertificateThumbprint
    )
    $profile = Read-MsixSigningProfile -Path $ProfilePath
    if ($profile.publisher -cne $Publisher) {
        throw 'アプリのPublisherがPC共通署名設定と一致しません。既存アプリの移行を確認してから設定を変更してください。'
    }
    if ($CertificateThumbprint -and $CertificateThumbprint.Replace(' ', '').ToUpperInvariant() -cne $profile.thumbprint) {
        throw '指定された拇印がPC共通署名設定と一致しません。別証明書への上書き指定は使用できません。'
    }
    $certificatePath = "Cert:\CurrentUser\My\$($profile.thumbprint)"
    if (-not (Test-Path -LiteralPath $certificatePath)) {
        throw 'PC共通設定で指定した証明書がCurrentUser/Myにありません。自動発行はしません。'
    }
    $certificate = Get-Item -LiteralPath $certificatePath -ErrorAction Stop
    try {
        Assert-MsixSigningCertificate -Certificate $certificate -Publisher $Publisher -Thumbprint $profile.thumbprint
        if ($certificate.NotAfter -le (Get-Date).AddDays(90)) { Write-Warning '共通MSIX証明書の期限まで90日以下です。全アプリの移行計画が必要です。' }
        return $certificate
    }
    catch { $certificate.Dispose(); throw }
}

Export-ModuleMember -Function Get-MsixSigningProfilePath, Read-MsixSigningProfile, Assert-MsixSigningCertificate, Get-MsixSigningCertificate
