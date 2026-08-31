#Requires -Version 7.4
[CmdletBinding()]
param(
    [ValidateSet('Plan','Build','Verify')][string]$Mode = 'Plan',
    [ValidateSet('beta','stable')][string]$Channel,
    [switch]$ReleaseIntentConfirmed,
    [switch]$AllowRebuild
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'msix-common.ps1')
. (Join-Path $PSScriptRoot 'msix-assets.ps1')
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')

if ($Mode -ne 'Plan' -and -not $Channel) { throw '変更または検証するChannelをbeta/stableで指定してください。' }
if ($Mode -eq 'Build' -and -not $ReleaseIntentConfirmed) { throw '生成には明示的な -ReleaseIntentConfirmed が必要です。' }
if ($Mode -ne 'Build' -and $AllowRebuild) { throw '-AllowRebuildはBuild専用です。' }
if ($Mode -eq 'Plan' -and -not $Channel) {
    Write-Output 'beta: 開発用Identity / 6か月有効の永続自己署名証明書 / ローカル検証用'
    Write-Output 'stable: Partner Centerの正式Identity / 未署名MSIX / Store提出用'
    Write-Output 'Planはビルド・証明書作成・信頼登録・インストール・公開を行いません。'
    return
}
$context = Get-MsixContext -Root $root -Channel $Channel
$packagePath = Join-Path $context.Output $context.FileName
if ($Mode -eq 'Plan') {
    [pscustomobject]@{
        Channel=$Channel; Version=$context.Version; Name=$context.Identity.name
        Publisher=$context.Identity.publisher; DisplayName=$context.Identity.displayName
        File=$context.FileName; Signing=$(if ($Channel -eq 'beta') {'DevelopmentSelfSigned'} else {'MicrosoftStore'})
        Installation='実行しない'; Publication='実行しない'; UpdateProvider='Microsoft Store（正式版）'
    }
    return
}
if ($Mode -eq 'Verify') {
    Test-MsixArtifact -Path $packagePath -Context $context
    return
}

$makeAppx = Find-WindowsSdkTool 'makeappx.exe'
$signTool = if ($Channel -eq 'beta') { Find-WindowsSdkTool 'signtool.exe' }
Get-Command cargo -ErrorAction Stop | Out-Null
$sourceCommit = (git -C $root rev-parse HEAD) -join ''
if ($LASTEXITCODE -ne 0) { throw 'ビルド元commitを取得できません。' }
$sourceDirty = [bool](git -C $root status --porcelain --untracked-files=no)
if ($LASTEXITCODE -ne 0) { throw '作業ツリーの状態を取得できません。' }
if (Test-Path -LiteralPath $context.Output) {
    if (-not $AllowRebuild) { throw '同一バージョンの出力先があります。再現確認だけの場合は -AllowRebuild、更新の場合はpackageVersionを増やしてください。' }
}
$channelRoot = Split-Path -Parent $context.Output
if (Test-Path -LiteralPath $channelRoot) {
    foreach ($directory in Get-ChildItem -LiteralPath $channelRoot -Directory) {
        $priorVersion = $null
        if ([version]::TryParse($directory.Name, [ref]$priorVersion) -and $priorVersion -gt [version]$context.Version) { throw '既存成果物より低いバージョンは生成しません。' }
    }
}
if ($Channel -eq 'beta') { Get-DevelopmentCertificate -Identity $context.Identity | Out-Null }
$work = Join-Path $root ('target/msix-work/' + [guid]::NewGuid().ToString('N'))
$payload = Join-Path $work 'payload'
[IO.Directory]::CreateDirectory($payload) | Out-Null

Push-Location $root
$savedFlags = $env:CARGO_ENCODED_RUSTFLAGS
$savedDebug = $env:CARGO_PROFILE_RELEASE_DEBUG
$savedStrip = $env:CARGO_PROFILE_RELEASE_STRIP
try {
    Invoke-CheckedNative cargo @('fmt','--all','--','--check')
    Invoke-CheckedNative cargo @('test','--locked')
    Invoke-CheckedNative cargo @('clippy','--locked','--all-targets','--','-D','warnings')
    # static CRTでVCLibsへの依存を避ける。デバッグ情報と個人PCのソースパスを含めない。
    $cargoHomePath = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $rustupHomePath = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $remaps = @(@($env:USERPROFILE,'/build-user'), @($cargoHomePath,'/cargo'), @($rustupHomePath,'/rustup'), @($root,'/src/HttpSelector'))
    $flags = @('-C','target-feature=+crt-static','-C','link-arg=/PDBALTPATH:HttpSelector.pdb')
    foreach ($remap in $remaps) {
        $flags += "--remap-path-prefix=$($remap[0])=$($remap[1])"
        $flags += "--remap-path-prefix=$($remap[0].Replace('\','/'))=$($remap[1])"
    }
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
    $env:CARGO_PROFILE_RELEASE_DEBUG = '0'
    $env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'
    Invoke-CheckedNative cargo @('build','--locked','--release','--bin','HttpSelector','--target','x86_64-pc-windows-msvc','--target-dir','target/msix-cargo')
    $exe = Join-Path $root 'target/msix-cargo/x86_64-pc-windows-msvc/release/HttpSelector.exe'
    [byte[]]$exeBytes = [IO.File]::ReadAllBytes($exe)
    Assert-NoDeveloperPaths $exeBytes
    if ([Text.Encoding]::ASCII.GetString($exeBytes) -match '(?i)(VCRUNTIME\d+|MSVCP\d+)\.dll') { throw '未同梱のVCランタイムへの依存が残っています。' }
    Copy-Item -LiteralPath $exe -Destination (Join-Path $payload 'HttpSelector.exe')
    New-MsixAssets -Directory (Join-Path $payload 'Assets')
    Write-MsixManifest -Root $root -Context $context -Destination (Join-Path $payload 'AppxManifest.xml')
    $temporaryPackage = Join-Path $work $context.FileName
    Invoke-CheckedNative $makeAppx @('pack','/d',$payload,'/p',$temporaryPackage,'/h','SHA256')
    if ($Channel -eq 'beta') {
        $certificate = Get-DevelopmentCertificate -Identity $context.Identity -CreateIfMissing
        # 秘密鍵は非エクスポート可能なCurrentUser\Myに置き、公開CERだけを出力する。
        Export-Certificate -Cert $certificate -FilePath (Join-Path $work 'HttpSelector.Development.cer') -Type CERT | Out-Null
        Invoke-CheckedNative $signTool @('sign','/fd','SHA256','/sha1',$certificate.Thumbprint,'/s','My',$temporaryPackage)
    }
    $report = Test-MsixArtifact -Path $temporaryPackage -Context $context
    $report | Add-Member -NotePropertyName SourceCommit -NotePropertyValue $sourceCommit
    $report | Add-Member -NotePropertyName SourceDirty -NotePropertyValue $sourceDirty
    $report | Add-Member -NotePropertyName CargoLockSHA256 -NotePropertyValue (Get-FileHash -LiteralPath (Join-Path $root 'Cargo.lock')).Hash
    $report | Add-Member -NotePropertyName BuiltAtUtc -NotePropertyValue ([DateTime]::UtcNow.ToString('o'))
    $report | Add-Member -NotePropertyName InstallationTest -NotePropertyValue 'NotPerformed'
    # 全検証が通るまでrelease配下の前回成果物を変更しない。
    [IO.Directory]::CreateDirectory($context.Output) | Out-Null
    Copy-Item -LiteralPath $temporaryPackage -Destination $packagePath -Force
    if ($Channel -eq 'beta') { Copy-Item -LiteralPath (Join-Path $work 'HttpSelector.Development.cer') -Destination $context.Output -Force }
    $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $context.Output 'verification.json') -Encoding utf8NoBOM
    "$($report.SHA256)  $($context.FileName)" | Set-Content -LiteralPath (Join-Path $context.Output 'SHA256SUMS.txt') -Encoding ascii
    $report
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $savedFlags
    $env:CARGO_PROFILE_RELEASE_DEBUG = $savedDebug
    $env:CARGO_PROFILE_RELEASE_STRIP = $savedStrip
    Pop-Location
}
