#Requires -Version 7.0
[CmdletBinding()]
param([string]$NoticesDirectory)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$repo = Split-Path -Parent $PSScriptRoot
$target = Join-Path $repo "target"
$output = Join-Path $repo "target/editions"
$triple = "x86_64-pc-windows-msvc"

$buildVariables = "ORT_LIB_PATH", "ORT_LIB_LOCATION", "ORT_PREFER_DYNAMIC_LINK", "YISOU_RELEASE_NOTICES"
$previousEnvironment = @{}
foreach ($name in $buildVariables) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    if ($null -ne $previousEnvironment[$name]) {
        Remove-Item -LiteralPath "Env:$name"
    }
}

try {
    foreach ($edition in "lite", "full") {
        $options = @(
            "build", "--locked", "--release", "--no-default-features",
            "--manifest-path", (Join-Path $repo "Cargo.toml"),
            "--target", $triple, "--target-dir", $target, "--bin", "YiSou"
        )
        if ($edition -eq "full") {
            $options += "--features", "ocr"
        }
        if ($NoticesDirectory) {
            $env:YISOU_RELEASE_NOTICES = (Resolve-Path -LiteralPath (Join-Path $NoticesDirectory "$edition.zip")).Path
        }
        & cargo @options

        $destination = Join-Path $output $edition
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $target "$triple/release/YiSou.exe") -Destination $destination -Force
        foreach ($name in "onnxruntime.dll", "DirectML.dll") {
            $obsoleteRuntime = Join-Path $destination $name
            if (Test-Path -LiteralPath $obsoleteRuntime -PathType Leaf) {
                Remove-Item -LiteralPath $obsoleteRuntime
            }
        }
        Get-Item -LiteralPath (Join-Path $destination "YiSou.exe") | Select-Object FullName, Length
    }
}
finally {
    foreach ($name in $buildVariables) {
        if ($null -ne $previousEnvironment[$name]) {
            [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name])
        }
        elseif (Test-Path -LiteralPath "Env:$name") {
            Remove-Item -LiteralPath "Env:$name"
        }
    }
}
