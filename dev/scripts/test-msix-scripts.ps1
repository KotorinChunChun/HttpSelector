#Requires -Version 7.4
# 署名・信頼登録・インストールを行わず、誤配布防止ガードとパッケージ破損検知を検証する。
param([string]$SignedPackagePath)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'msix-common.ps1')
. (Join-Path $PSScriptRoot 'msix-assets.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
$fixture = Join-Path $root ('target/msix-script-tests/' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $fixture 'packaging')) | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'packaging/release-config.json') -Destination (Join-Path $fixture 'packaging')
Copy-Item -LiteralPath (Join-Path $root 'packaging/AppxManifest.xml.template') -Destination (Join-Path $fixture 'packaging')
$script:checks = 0
function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "検証失敗: $Message" }
    $script:checks++
}
function Assert-Rejected([scriptblock]$Action, [string]$Expected) {
    $message = ''
    try { & $Action | Out-Null } catch { $message = $_.Exception.Message }
    Assert-True ($message -like "*$Expected*") "拒否理由: $Expected / 実際: $message"
}

foreach ($script in Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*msix*.ps1') {
    $tokens = $null; $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$tokens, [ref]$errors) | Out-Null
    Assert-True ($errors.Count -eq 0) "PowerShell構文: $($script.Name)"
}
Assert-Rejected { & (Join-Path $PSScriptRoot 'release-msix.ps1') -Mode Build -Channel beta } 'ReleaseIntentConfirmed'
Assert-Rejected { & (Join-Path $PSScriptRoot 'release-msix.ps1') -Mode Build -ReleaseIntentConfirmed } 'Channel'
Assert-Rejected { Get-MsixContext $fixture stable } '正式Identityが未設定'
$configPath = Join-Path $fixture 'packaging/release-config.json'
$config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
foreach ($invalid in @('0.0.1.0','1.0.1.1','1.65536.1.0','1.0.1')) {
    $config.packageVersion = $invalid
    $config | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $configPath
    Assert-Rejected { Get-MsixContext $fixture beta } 'バージョン'
}
$config.packageVersion = '1.0.1.0'
$config | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $configPath
$identityPath = Join-Path $fixture 'packaging/store-identity.local.json'
$config.beta | ConvertTo-Json | Set-Content -LiteralPath $identityPath
Assert-Rejected { Get-MsixContext $fixture stable } '開発用Identity'
@{name='Example.HttpSelector';publisher='CN=TestIdentityOnly';publisherDisplayName='Example & Tools';displayName='Test "Application"'} | ConvertTo-Json | Set-Content -LiteralPath $identityPath
$context = Get-MsixContext $fixture stable
$payload = Join-Path $fixture 'payload'
[IO.Directory]::CreateDirectory($payload) | Out-Null
Write-MsixManifest $fixture $context (Join-Path $payload 'AppxManifest.xml')
[xml]$manifest = Get-Content -LiteralPath (Join-Path $payload 'AppxManifest.xml') -Raw
Assert-True ($manifest.Package.Properties.PublisherDisplayName -ceq 'Example & Tools') 'XML属性とテキストのエスケープ'
[IO.File]::WriteAllBytes((Join-Path $payload 'HttpSelector.exe'), [Text.Encoding]::ASCII.GetBytes(('fixture only. '*6000)))
New-MsixAssets (Join-Path $payload 'Assets')
$package = Join-Path $fixture 'test-unsigned.msix'
Invoke-CheckedNative (Find-WindowsSdkTool 'makeappx.exe') @('pack','/d',$payload,'/p',$package,'/h','SHA256')
$result = Test-MsixArtifact $package $context
Assert-True ($result.SignatureTrust -eq 'UnsignedForStore') 'Storeの未署名構成'
$wrong = Get-MsixContext $fixture stable
$wrong.Version = '1.0.2.0'
Assert-Rejected { Test-MsixArtifact $package $wrong } 'Identityが設定と一致'
Assert-Rejected { Assert-NoDeveloperPaths ([Text.Encoding]::UTF8.GetBytes('C:\Users\PrivateName\src\main.rs')) } '絶対パス'
Assert-Rejected { Assert-NoDeveloperPaths ([Text.Encoding]::Unicode.GetBytes('C:\Users\PrivateName\src\main.rs')) } '絶対パス'
$zip = [IO.Compression.ZipFile]::Open($package, [IO.Compression.ZipArchiveMode]::Update)
try {
    $entry = $zip.GetEntry('HttpSelector.exe')
    $stream = $entry.Open()
    try { $stream.Position = 70000; $stream.WriteByte(99) } finally { $stream.Dispose() }
} finally { $zip.Dispose() }
Assert-Rejected { Test-MsixArtifact $package $context } 'ハッシュが不一致'
$zip = [IO.Compression.ZipFile]::Open($package, [IO.Compression.ZipArchiveMode]::Update)
try { $zip.CreateEntry('private.pfx') | Out-Null } finally { $zip.Dispose() }
Assert-Rejected { Test-MsixArtifact $package $context } '未許可'
if ($SignedPackagePath) {
    $signedContext = Get-MsixContext $root beta
    $signedCopy = Join-Path $fixture 'test-signed.msix'
    Copy-Item -LiteralPath $SignedPackagePath -Destination $signedCopy
    Copy-Item -LiteralPath (Join-Path (Split-Path -Parent $SignedPackagePath) 'HttpSelector.Development.cer') -Destination $fixture
    $signedResult = Test-MsixArtifact $signedCopy $signedContext
    $existingCertificate = Get-DevelopmentCertificate $signedContext.Identity
    Assert-True ($existingCertificate.Thumbprint -eq $signedResult.SignerThumbprint) '永続証明書の再利用'
    $zip = [IO.Compression.ZipFile]::Open($signedCopy, [IO.Compression.ZipArchiveMode]::Update)
    try {
        $binary = $zip.GetEntry('HttpSelector.exe')
        [byte[]]$binaryBytes = Read-ZipEntryBytes $binary
        $binaryBytes[100] = $binaryBytes[100] -bxor 1
        $stream = $binary.Open()
        try { $stream.Write($binaryBytes,0,$binaryBytes.Length) } finally { $stream.Dispose() }
        $mapEntry = $zip.GetEntry('AppxBlockMap.xml')
        [xml]$map = [Text.Encoding]::UTF8.GetString((Read-ZipEntryBytes $mapEntry))
        $binaryMap = $map.BlockMap.File | Where-Object Name -EQ 'HttpSelector.exe'
        $binaryMap.Block[0].Hash = [Convert]::ToBase64String([Security.Cryptography.SHA256]::HashData([byte[]]$binaryBytes[0..65535]))
        [byte[]]$mapBytes = [Text.UTF8Encoding]::new($false).GetBytes($map.OuterXml)
        $stream = $mapEntry.Open()
        try { $stream.SetLength(0); $stream.Write($mapBytes,0,$mapBytes.Length) } finally { $stream.Dispose() }
    } finally { $zip.Dispose() }
    # ペイロードとBlockMapを同時に改変しても署名済みメタデータとの比較で拒否する。
    Assert-Rejected { Test-MsixArtifact $signedCopy $signedContext } '署名済みメタデータが不一致'
}
Write-Output "MSIXスクリプト検証: $script:checks 件成功（証明書・信頼ストア・インストール変更なし）"
