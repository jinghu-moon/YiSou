param([string]$Output = "target/ocr-fixtures")

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
New-Item -Path $Output -ItemType Directory -Force | Out-Null

$samples = @(
    @{ file = "terminal-small.png"; font = "Consolas"; size = 15; bg = "#171717"; fg = "#e6e6e6"
       lines = @('error[E0425]: cannot find value', '  --> src\main.rs:18:5'); tokens = @('E0425', 'src\main.rs:18:5') },
    @{ file = "windows-error.png"; font = "Microsoft YaHei UI"; size = 16; bg = "#f4f4f4"; fg = "#222222"
       lines = @('系统找不到指定路径。错误码 0x80070003', 'D:\Projects\YiSou\target\release'); tokens = @('0x80070003', 'D:\Projects\YiSou\target\release') },
    @{ file = "mixed-dark.png"; font = "Microsoft YaHei UI"; size = 19; bg = "#1f2331"; fg = "#e5e9f0"
       lines = @('构建失败：找不到依赖 rapid-structure-rs', '运行 cargo test --release 后重试'); tokens = @('rapid-structure-rs', 'cargo test --release') },
    @{ file = "punctuation.png"; font = "Consolas"; size = 17; bg = "#ffffff"; fg = "#323232"
       lines = @('#g C# NullReferenceException', 'git diff -- src/app.rs'); tokens = @('#g', 'C#', 'NullReferenceException', 'src/app.rs') },
    @{ file = "low-contrast.png"; font = "Microsoft YaHei UI"; size = 14; bg = "#292e38"; fg = "#acafbb"
       lines = @('连接超时，请检查代理设置。', 'HTTP 504 Gateway Timeout'); tokens = @('HTTP 504') },
    @{ file = "single-line.png"; font = "Segoe UI"; size = 22; bg = "#ffffff"; fg = "#101010"
       lines = @('Windows Update error 0x8024402c'); tokens = @('0x8024402c') },
    @{ file = "blank.png"; font = "Segoe UI"; size = 22; bg = "#eef0f3"; fg = "#101010"
       lines = @(); tokens = @() }
)

$manifest = foreach ($sample in $samples) {
    $font = [Drawing.Font]::new($sample.font, $sample.size, [Drawing.GraphicsUnit]::Pixel)
    $brush = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml($sample.fg))
    $lineHeight = $sample.size + 9
    $bitmap = [Drawing.Bitmap]::new(470, [Math]::Max(42, 16 + $sample.lines.Count * $lineHeight))
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.Clear([Drawing.ColorTranslator]::FromHtml($sample.bg))
        $graphics.TextRenderingHint = [Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        for ($line = 0; $line -lt $sample.lines.Count; $line++) {
            $graphics.DrawString($sample.lines[$line], $font, $brush, 8, 8 + $line * $lineHeight)
        }
        $bitmap.Save((Join-Path $Output $sample.file), [Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
        $brush.Dispose()
        $font.Dispose()
    }
    @{ image = $sample.file; text = $sample.lines -join " "; exact_tokens = $sample.tokens }
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $Output "manifest.json") -Encoding utf8NoBOM
