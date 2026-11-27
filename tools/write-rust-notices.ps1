param(
    [Parameter(Mandatory)][string]$Manifest,
    [Parameter(Mandatory)][string]$Target,
    [Parameter(Mandatory)][ValidateSet("lite", "full")][string]$Edition,
    [Parameter(Mandatory)][string]$Destination
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$features = @("--no-default-features")
if ($Edition -eq "full") { $features += "--features", "ocr" }
$metadata = cargo metadata --locked --format-version 1 --filter-platform $Target --manifest-path $Manifest @features | ConvertFrom-Json
$tree = cargo tree --locked --package yisou --target $Target --edges normal --prefix none --format '{p}' --manifest-path $Manifest @features
$dependencies = @{}
foreach ($line in $tree) {
    if ($line -match '^([^ ]+ v[^ ]+)') { $dependencies[$Matches[1]] = $true }
}

$repo = Split-Path -Parent $Manifest
$supplements = Join-Path $repo "assets/licenses"
$rustDoc = Join-Path (rustc --print sysroot) "share/doc/rust"
$rust = New-Item -ItemType Directory (Join-Path $Destination "rust-standard-library")
Copy-Item -LiteralPath (Join-Path $rustDoc "COPYRIGHT-library.html"), (Join-Path $rustDoc "licenses") -Destination $rust.FullName -Recurse

$crates = New-Item -ItemType Directory (Join-Path $Destination "crates")
$pathsByHash = @{}
$index = foreach ($package in ($metadata.packages | Sort-Object name, version)) {
    if (!$package.source -or !$dependencies.ContainsKey("$($package.name) v$($package.version)")) { continue }
    $source = Split-Path -Parent $package.manifest_path
    $licenseFiles = @(Get-ChildItem -LiteralPath $source -File |
        Where-Object Name -Match '^(LICENSE|COPYING|COPYRIGHT|NOTICE)([-_.]|$)')
    if ($package.license_file) {
        $declared = Get-Item -LiteralPath (Join-Path $source $package.license_file)
        if ($declared.FullName -notin $licenseFiles.FullName) { $licenseFiles += $declared }
    }
    $label = "$($package.name)-$($package.version)"
    if (!$licenseFiles) {
        $supplement = Join-Path $supplements "$label.txt"
        if (!(Test-Path -LiteralPath $supplement)) { throw "Missing license text for $label" }
        $licenseFiles = @(Get-Item -LiteralPath $supplement)
    }

    $terms = if ($package.license) { $package.license } else { $package.license_file }
    "$label | $terms | $($package.repository ?? $package.homepage ?? $package.source)"
    foreach ($file in ($licenseFiles | Sort-Object Name, FullName)) {
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
        if (!$pathsByHash.ContainsKey($hash)) {
            $folder = New-Item -ItemType Directory -Force (Join-Path $crates.FullName $label)
            Copy-Item -LiteralPath $file.FullName -Destination $folder.FullName
            $pathsByHash[$hash] = "crates/$label/$($file.Name)"
        }
        "  $($file.Name) -> $($pathsByHash[$hash])"
    }
}

$notices = @(
    "YiSou third-party notices",
    "",
    "Brand marks identify their respective services. Their trademarks belong to their respective owners.",
    "theSVG legal terms: https://github.com/glincker/thesvg/blob/main/LEGAL.md",
    "Tabler Icons: https://github.com/tabler/tabler-icons",
    "Rust standard library: https://github.com/rust-lang/rust"
)
if ($Edition -eq "full") {
    $notices += @(
        "ONNX Runtime 1.28.0: https://onnxruntime.ai/"
    )
}
$notices += @(
    "",
    "Cargo dependencies for the $Edition edition",
    "Paths below refer to files in this archive. Identical files share a path.",
    ""
) + $index
Set-Content -LiteralPath (Join-Path $Destination "THIRD-PARTY.txt") -Value $notices -Encoding utf8
