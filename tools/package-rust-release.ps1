#Requires -Version 7.0
[CmdletBinding()]
param([string]$OutputDirectory)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$repo = Split-Path -Parent $PSScriptRoot
$triple = "x86_64-pc-windows-msvc"
$manifest = Join-Path $repo "Cargo.toml"
$metadata = cargo metadata --locked --no-deps --format-version 1 --manifest-path $manifest | ConvertFrom-Json
$version = ($metadata.packages | Where-Object name -eq "yisou").version
if (!$version) { throw "Cannot read the YiSou release version" }

$target = Join-Path $repo "target"
$output = New-Item -ItemType Directory -Force ($OutputDirectory ? $OutputDirectory : (Join-Path $target "release/$version"))
$staging = New-Item -ItemType Directory (Join-Path $output.FullName ([guid]::NewGuid().ToString("N")))

try {
    $editions = "lite", "full"
    foreach ($edition in $editions) {
        $licenses = New-Item -ItemType Directory (Join-Path $staging.FullName "$edition-licenses")
        Copy-Item -LiteralPath (Join-Path $repo "LICENSE") -Destination (Join-Path $licenses.FullName "YiSou.txt")
        Copy-Item -LiteralPath (Join-Path $repo "assets/icons/LICENSE") -Destination (Join-Path $licenses.FullName "theSVG.txt")
        Copy-Item -LiteralPath (Join-Path $repo "assets/icons/tabler/LICENSE") -Destination (Join-Path $licenses.FullName "Tabler-Icons.txt")

        & (Join-Path $PSScriptRoot "write-rust-notices.ps1") -Manifest $manifest -Target $triple -Edition $edition -Destination $licenses.FullName

        if ($edition -eq "full") {
            $onnx = & (Join-Path $PSScriptRoot "get-onnxruntime-package.ps1")
            tar -xf $onnx -C $licenses.FullName onnxruntime/LICENSE onnxruntime/ThirdPartyNotices.txt
        }

        [IO.Compression.ZipFile]::CreateFromDirectory($licenses.FullName, (Join-Path $staging.FullName "$edition.zip"))
    }

    & (Join-Path $PSScriptRoot "build-rust-editions.ps1") -NoticesDirectory $staging.FullName

    $executables = foreach ($edition in $editions) {
        $exe = Get-Item -LiteralPath (Join-Path $target "editions/$edition/YiSou.exe")
        if ($exe.VersionInfo.ProductVersion -ne $version -or
            $exe.VersionInfo.FileDescription -ine "YiSou $edition") {
            throw "Unexpected application metadata for $edition"
        }
        $destination = Join-Path $staging.FullName "YiSou-$version-windows-x64-$edition.exe"
        Copy-Item -LiteralPath $exe.FullName -Destination $destination
        Get-Item -LiteralPath $destination
    }

    $hashes = foreach ($exe in $executables) {
        $hash = (Get-FileHash -LiteralPath $exe.FullName).Hash.ToLowerInvariant()
        "$hash  $($exe.Name)"
    }
    $checksum = Join-Path $staging.FullName "SHA256SUMS.txt"
    Set-Content -LiteralPath $checksum -Value $hashes -Encoding ascii
    foreach ($exe in $executables) {
        Move-Item -LiteralPath $exe.FullName -Destination (Join-Path $output.FullName $exe.Name) -Force
    }
    Move-Item -LiteralPath $checksum -Destination (Join-Path $output.FullName "SHA256SUMS.txt") -Force
    Copy-Item -LiteralPath (Join-Path $repo "docs/release-notes/$version.md") -Destination (Join-Path $output.FullName "RELEASE-NOTES.md") -Force
    Get-ChildItem -LiteralPath $output.FullName -File |
        Where-Object { $_.Name -in $executables.Name -or $_.Name -in ("SHA256SUMS.txt", "RELEASE-NOTES.md") } |
        Select-Object Name, Length
}
finally {
    if ($staging.Parent.FullName -eq $output.FullName) {
        Remove-Item -LiteralPath $staging.FullName -Recurse -Force
    }
}
