param(
    [string]$ExePath = "",
    [int]$Runs = 5
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..\")
if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $ExePath = Join-Path $ProjectRoot "target\release\HttpSelector.exe"
}
if (-not (Test-Path -LiteralPath $ExePath)) {
    throw "実行ファイルがありません: $ExePath"
}

$measurements = for ($run = 1; $run -le $Runs; $run++) {
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $ExePath -ArgumentList "https://example.com/performance?run=$run" -PassThru
    do {
        Start-Sleep -Milliseconds 10
        $process.Refresh()
    } while ($process.MainWindowHandle -eq 0 -and $watch.ElapsedMilliseconds -lt 10000)
    $watch.Stop()
    if ($process.MainWindowHandle -eq 0) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw "Run ${run}: 10秒以内に画面が表示されませんでした"
    }
    $workingSetMb = [math]::Round($process.WorkingSet64 / 1MB, 1)
    Stop-Process -Id $process.Id -Force
    Start-Sleep -Milliseconds 200
    [PSCustomObject]@{
        Run = $run
        StartupMs = $watch.ElapsedMilliseconds
        WorkingSetMB = $workingSetMb
    }
}

$measurements | Format-Table -AutoSize
[PSCustomObject]@{
    StartupMedianMs = ($measurements.StartupMs | Sort-Object)[[math]::Floor(($measurements.Count - 1) / 2)]
    StartupMaxMs = ($measurements.StartupMs | Measure-Object -Maximum).Maximum
    WorkingSetMaxMB = ($measurements.WorkingSetMB | Measure-Object -Maximum).Maximum
} | Format-List
