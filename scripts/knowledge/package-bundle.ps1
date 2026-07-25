param(
  [string]$OutputPath = "references/knowledge/release/acumod-knowledge-15.23.zip"
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$buildRoot = Join-Path $root "references/knowledge/build"
$files = @(
  (Join-Path $buildRoot "acumod-dev-game-facts.acukb"),
  (Join-Path $buildRoot "acumod-dev-modding.acukb"),
  (Join-Path $buildRoot "acumod-dev-game-guides.acukb"),
  (Join-Path $buildRoot "acumod-dev-acumod-help.acukb")
)

$missing = $files | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) }
if ($missing.Count -gt 0) {
  throw "缺少开发知识包，请先运行 npm.cmd run knowledge:build-dev：$($missing -join ', ')"
}

$output = Join-Path $root $OutputPath
$outputDirectory = Split-Path -Parent $output
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
Compress-Archive -LiteralPath $files -DestinationPath $output -CompressionLevel Optimal -Force

$archive = Get-Item -LiteralPath $output
Write-Output "知识包整套 ZIP 已生成：$($archive.FullName)"
Write-Output "文件数量：4，大小：$($archive.Length) bytes"
