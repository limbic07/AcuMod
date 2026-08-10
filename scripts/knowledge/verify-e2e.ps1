param()

$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$manifest = Join-Path $projectRoot "src-tauri\windows-common-controls.manifest"
if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
    throw "Missing Windows test manifest: $manifest"
}

# The Tauri application embeds this manifest itself, but cargo test does not.
# Limit the workaround to this verification process instead of changing app linking.
$linkArguments = @(
    "-C",
    "link-arg=/MANIFEST:EMBED",
    "-C",
    "link-arg=/MANIFESTINPUT:$manifest",
    "-C",
    "link-arg=/WX"
)
$escapedArguments = $linkArguments | ForEach-Object {
    if ($_ -match '[\s"]') {
        '"' + ($_ -replace '"', '\"') + '"'
    }
    else {
        $_
    }
}
$previousRustFlags = $env:RUSTFLAGS
$rustFlagParts = @($previousRustFlags, ($escapedArguments -join " ")) | Where-Object { $_ }
$env:RUSTFLAGS = ($rustFlagParts -join " ").Trim()

try {
    & cargo test --manifest-path (Join-Path $projectRoot "src-tauri\Cargo.toml") `
        services::mhwdata::tests::installed_database_returns_raw_weapon_rows `
        --lib -- --exact
    exit $LASTEXITCODE
}
finally {
    $env:RUSTFLAGS = $previousRustFlags
}
