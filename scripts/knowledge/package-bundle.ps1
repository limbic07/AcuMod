param(
  [string]$OutputPath = "references/knowledge/release/acumod-knowledge-15.23.zip",
  [switch]$ModdingOnly
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$buildRoot = Join-Path $root "references/knowledge/build"
[string[]]$files = if ($ModdingOnly) {
  @((Join-Path $buildRoot "acumod-dev-modding.acukb"))
} else {
  @(
    (Join-Path $buildRoot "acumod-mhwdata-15.10.acumhwdb"),
    (Join-Path $buildRoot "acumod-dev-modding.acukb"),
    (Join-Path $buildRoot "acumod-dev-game-guides.acukb"),
    (Join-Path $buildRoot "acumod-dev-acumod-help.acukb")
  )
}

$missing = $files | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) }
if ($missing.Count -gt 0) {
  $buildCommand = if ($ModdingOnly) { "npm.cmd run knowledge:build-modding-dev" } else { "npm.cmd run knowledge:build-dev" }
  throw "缺少开发知识资料，请先运行 $buildCommand：$($missing -join ', ')"
}

$output = Join-Path $root $OutputPath
$outputDirectory = Split-Path -Parent $output
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
Compress-Archive -LiteralPath $files -DestinationPath $output -CompressionLevel Optimal -Force

$archive = Get-Item -LiteralPath $output
$fileCount = @($files).Length
Write-Output "知识包整套 ZIP 已生成：$($archive.FullName)"
Write-Output ("Entries: " + $fileCount + "; bytes: " + $archive.Length)
