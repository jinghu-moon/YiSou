$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$target = Join-Path $repo "build/rust-native"
$output = Join-Path $repo "build/editions"

foreach ($edition in "lite", "full") {
    $options = @(
        "build", "--locked", "--release", "--no-default-features",
        "--manifest-path", (Join-Path $repo "rust/Cargo.toml"),
        "--target-dir", $target, "--bin", "YiSou"
    )
    if ($edition -eq "full") {
        $options += "--features", "ocr"
    }
    & cargo @options
    if ($LASTEXITCODE -ne 0) {
        throw "YiSou $edition build failed (exit code $LASTEXITCODE)"
    }

    $destination = Join-Path $output $edition
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $target "release/YiSou.exe") -Destination $destination -Force
    Get-Item -LiteralPath (Join-Path $destination "YiSou.exe") | Select-Object FullName, Length
}
