param(
    [Parameter(Mandatory)][string[]]$Binary,
    [Parameter(Mandatory)][string]$Models,
    [ValidateRange(3, 100)][int]$Cycles = 13,
    [ValidateSet('Small', 'FullScreen')][string]$Selection = 'Small',
    [switch]$MeasureCooldown
)
$ErrorActionPreference = 'Stop'
$binaries = @($Binary | ForEach-Object { (Resolve-Path -LiteralPath $_).Path })
$model = (Resolve-Path -LiteralPath $Models).Path
if (Get-Process YiSou -ErrorAction SilentlyContinue) {
    throw 'Exit the running YiSou before benchmarking.'
}
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class YiSouOverlayPerf {
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, IntPtr title);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint message, IntPtr wp, IntPtr lp, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("dwmapi.dll")] public static extern int DwmFlush();

}
'@
$z = [IntPtr]::Zero
[void][YiSouOverlayPerf]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$originalCursor = [Windows.Forms.Cursor]::Position
$testDir = Join-Path ([IO.Path]::GetTempPath()) ("yisou-ocr-perf-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDir | Out-Null
$trace = Join-Path $testDir 'trace.csv'
$config = Join-Path $testDir 'settings.json'
@{schema=1; custom_engines=@(); preferences=@{ocr_enabled=$true; ocr_model_dir=$model}} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $config -Encoding utf8NoBOM
$oldTrace = $env:YISOU_MEMORY_TRACE
$oldTiming = $env:YISOU_OCR_TRACE
$timing = Join-Path $testDir 'timing.csv'
$env:YISOU_OCR_TRACE = $timing
$env:YISOU_MEMORY_TRACE = $trace
$oldData = $env:LOCALAPPDATA
$env:LOCALAPPDATA = $testDir
function Post-TestMessage($hwnd, $message, $parameter) {
    $result = $z
    if ([YiSouOverlayPerf]::SendMessageTimeout($hwnd, $message, [IntPtr]::new($parameter), $z, 2, 2500, [ref]$result) -eq $z) {
        throw "Window did not process message $message"
    }
}
function Await-Window($name, $pidExpected) {
    for ($attempt=0; $attempt -lt 50; $attempt++) {
        $hwnd = [YiSouOverlayPerf]::FindWindow($name, $z)
        if ($hwnd -ne $z) {
            $windowPid = [uint32]0
            [void][YiSouOverlayPerf]::GetWindowThreadProcessId($hwnd, [ref]$windowPid)
            if ($windowPid -eq $pidExpected) { return $hwnd }
        }
        Start-Sleep -Milliseconds 20
    }
    throw "Window not found: $name"
}
function Place-Cursor($left, $top) {
    [void][YiSouOverlayPerf]::SetCursorPos($left, $top)
}
function Report($series, $quantile) {
    $sorted = @($series | Sort-Object)
    return [Math]::Round($sorted[[Math]::Ceiling($sorted.Count * $quantile) - 1], 2)
}
try {
    $screen = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $fixtureBounds = if ($Selection -eq 'FullScreen') {
        $screen
    } else {
        [Drawing.Rectangle]::new($screen.Left, $screen.Top, 850, 460)
    }
    $fixtureText = @('易搜截图文字测试', '第二行可以单独拖选', 'Rust error E0425') -join [Environment]::NewLine
    $fixtureReady = Join-Path $testDir 'fixture.ready'
    $fixtureScript = {
        Add-Type -AssemblyName System.Drawing,System.Windows.Forms
        [void][Windows.Forms.Application]::SetHighDpiMode([Windows.Forms.HighDpiMode]::PerMonitorV2)
        $window = [Windows.Forms.Form]::new()
        $window.FormBorderStyle = 'None'
        $window.TopMost = $true
        $window.ShowInTaskbar = $false
        $window.StartPosition = 'Manual'
        $window.Bounds = [Drawing.Rectangle]::new($Left, $Top, $Width, $Height)
        $window.BackColor = [Drawing.Color]::White
        $label = [Windows.Forms.Label]::new()
        $label.Bounds = [Drawing.Rectangle]::new(73, 96, 530, 140)
        $label.Font = [Drawing.Font]::new('Microsoft YaHei UI', 27, [Drawing.GraphicsUnit]::Pixel)
        $label.Text = $Text
        $window.Controls.Add($label)
        $window.Show()
        [Windows.Forms.Application]::DoEvents()
        [IO.File]::WriteAllText($ReadyPath, '')
        [Windows.Forms.Application]::Run($window)
    }
    $fixtureSetup = @"
`$Left = $($fixtureBounds.X)
`$Top = $($fixtureBounds.Y)
`$Width = $($fixtureBounds.Width)
`$Height = $($fixtureBounds.Height)
`$Text = '$($fixtureText.Replace("'", "''"))'
`$ReadyPath = '$($fixtureReady.Replace("'", "''"))'
"@
    $fixtureCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("$fixtureSetup`n$fixtureScript"))
    $fixture = Start-Process -FilePath (Join-Path $PSHOME 'pwsh.exe') -ArgumentList @('-NoProfile', '-STA', '-EncodedCommand', $fixtureCommand) -WindowStyle Hidden -PassThru
    $fixtureDeadline = [Diagnostics.Stopwatch]::StartNew()
    while (!(Test-Path -LiteralPath $fixtureReady)) {
        if ($fixture.HasExited -or $fixtureDeadline.ElapsedMilliseconds -gt 8000) {
            throw 'Benchmark fixture did not open.'
        }
        Start-Sleep -Milliseconds 20
    }
    foreach ($executable in $binaries) {
        foreach ($csv in $trace,$timing) { if (Test-Path -LiteralPath $csv) { Remove-Item -LiteralPath $csv } }
        $exe = Start-Process -FilePath $executable -ArgumentList @('--config', ('"' + $config + '"'), '--startup') -WindowStyle Hidden -PassThru
        try {
            $main = Await-Window 'YiSou.Rust.Search' $exe.Id
            $open = [Collections.Generic.List[double]]::new()
            $drag = [Collections.Generic.List[double]]::new()
            $release = [Collections.Generic.List[double]]::new()
            $ready = [Collections.Generic.List[double]]::new()
            $settledWorking = [Collections.Generic.List[double]]::new()
            $settledPrivate = [Collections.Generic.List[double]]::new()
            $exe.Refresh()
            $idleWorking = $exe.WorkingSet64 / 1MB
            for ($cycle = 0; $cycle -lt $Cycles; $cycle++) {
                [void][YiSouOverlayPerf]::DwmFlush()
                $watch = [Diagnostics.Stopwatch]::StartNew()
                Post-TestMessage $main 0x312 1
                $overlay = Await-Window 'YiSou.Rust.OcrSelection' $exe.Id
                Post-TestMessage $overlay 0xf 0
                [void][YiSouOverlayPerf]::DwmFlush()
                if ($cycle -gt 0) { $open.Add($watch.Elapsed.TotalMilliseconds) }
                $left = $screen.Left + 53
                $top = $screen.Top + 84
                if ($Selection -eq 'FullScreen') {
                    $left = $screen.Left
                    $top = $screen.Top
                }
                Place-Cursor $left $top
                Post-TestMessage $overlay 0x201 1
                for ($step = 0; $step -lt 30; $step++) {
                    $watch.Restart()
                    Place-Cursor ($screen.Left + 350 + $step * 9) ($screen.Top + 210 + $step * 2)
                    Post-TestMessage $overlay 0x200 1
                    Post-TestMessage $overlay 0xf 0
                    [void][YiSouOverlayPerf]::DwmFlush()
                    if ($cycle -gt 0) { $drag.Add($watch.Elapsed.TotalMilliseconds) }
                }
                if ($Selection -eq 'FullScreen') {
                    Place-Cursor ($screen.Right - 1) ($screen.Bottom - 1)
                } else {
                    Place-Cursor ($screen.Left + 640) ($screen.Top + 281)
                }
                $watch.Restart()
                Post-TestMessage $overlay 0x202 0
                $release.Add($watch.Elapsed.TotalMilliseconds)
                do {
                    Start-Sleep -Milliseconds 2
                    $logNow = Import-Csv -LiteralPath $trace
                    $readyCount = @($logNow | Where-Object event -eq 'ocr_text_ready').Count
                } until ($watch.ElapsedMilliseconds -gt 2500 -or $readyCount -gt $cycle)
                if ($readyCount -le $cycle) {
                    throw "OCR was not ready in $executable cycle ${cycle}: $(($logNow | Select-Object -Last 6).event -join ', ')"
                }
                $ready.Add($watch.Elapsed.TotalMilliseconds)
                Post-TestMessage $overlay 0xf 0
                [void][YiSouOverlayPerf]::DwmFlush()
                $exe.Refresh()
                $settledWorking.Add($exe.WorkingSet64 / 1MB)
                $settledPrivate.Add($exe.PrivateMemorySize64 / 1MB)
                Post-TestMessage $overlay 0x100 0x1b
            }
            if ($MeasureCooldown) { Start-Sleep -Milliseconds 11200 }
            $exe.Refresh()
            $log = Import-Csv -LiteralPath $trace
            $opens = @($log | Where-Object event -eq 'ocr_overlay_shown')
            $ws = @($opens | ForEach-Object { [double]$_.working_set_bytes / 1MB } | Select-Object -Skip 1)
            $events = Import-Csv -LiteralPath $timing
            $spans = [ordered]@{}
            foreach ($group in ($events | Group-Object event)) {
                $values = @($group.Group | ForEach-Object { [double]$_.duration_us / 1000 })
                $spans[$group.Name] = @($values.Count, (Report $values 0.5), (Report $values 0.95))
            }
            [pscustomobject]@{
                Binary = $executable
                Selection = $Selection
                IdleWorkingSetMiB = [Math]::Round($idleWorking, 2)
                ReadyWorkingSetMiB = Report $settledWorking 0.5
                ReadyPrivateMiB = Report $settledPrivate 0.5
                PeakWorkingSetMiB = [Math]::Round($exe.PeakWorkingSet64 / 1MB, 2)
                AfterCancelWorkingSetMiB = [Math]::Round($exe.WorkingSet64 / 1MB, 2)
                AfterCancelPrivateMiB = [Math]::Round($exe.PrivateMemorySize64 / 1MB, 2)
                OpenMsMedian = Report $open 0.5
                DragMsMedian = Report $drag 0.5
                DragMsP95 = Report $drag 0.95
                ColdOverlayWorkingSetMiB = [Math]::Round([double]$opens[0].working_set_bytes / 1MB, 2)
                WarmOverlayWorkingSetMiB = Report $ws 0.5
                WarmOverlayPrivateMiB = Report @($opens | Select-Object -Skip 1 | ForEach-Object { [double]$_.private_bytes / 1MB }) 0.5
                Capture = ($log.event | Where-Object { $_ -like 'ocr_capture_*' } | Sort-Object -Unique) -join ', '
                ReleaseMsMedian = Report @($release | Select-Object -Skip 1) 0.5
                ReleaseMsP95 = Report @($release | Select-Object -Skip 1) 0.95
                TextReadyMsMedian = Report @($ready | Select-Object -Skip 1) 0.5
                SpansCountP50P95Ms = $spans
            } | ConvertTo-Json -Compress -Depth 6
        } finally {
            if (-not $exe.HasExited) { Stop-Process -Id $exe.Id -Force; $exe.WaitForExit() }
        }
    }
} finally {
    if ($null -ne $fixture -and !$fixture.HasExited) {
        Stop-Process -Id $fixture.Id
        $fixture.WaitForExit()
    }
    [Windows.Forms.Cursor]::Position = $originalCursor
    $env:LOCALAPPDATA = $oldData
    $env:YISOU_MEMORY_TRACE = $oldTrace
    $env:YISOU_OCR_TRACE = $oldTiming
    foreach ($temporary in $config,$trace,$timing,$fixtureReady,(Join-Path $testDir 'YiSouRust/last-config.txt')) {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
    }
}
