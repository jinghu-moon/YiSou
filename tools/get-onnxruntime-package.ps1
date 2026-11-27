#Requires -Version 7.0

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$downloads = New-Item -ItemType Directory -Force (Join-Path $repo "target/release-downloads")
$name = "onnxruntime-1.28.0-cp313-cp313-win_amd64.whl"
$url = "https://files.pythonhosted.org/packages/56/f1/51225c202edba4dfc94e1ea03f3d78f1aaf307da75fd792c0ce1946b2514/$name"
$sha256 = "1A1A19175464665C9B8D50BC916F216CC0B569110045B7BBCA8F9F290B186F58"
$path = Join-Path $downloads.FullName $name
if (!(Test-Path -LiteralPath $path) -or (Get-FileHash -LiteralPath $path).Hash -ne $sha256) {
    Invoke-WebRequest -Uri $url -OutFile $path -TimeoutSec 120
}
if ((Get-FileHash -LiteralPath $path).Hash -ne $sha256) {
    throw "Unexpected SHA-256 for $name"
}
$path
