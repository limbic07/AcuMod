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
    $tests = @(
        "services::mhwdata::tests::installed_database_returns_raw_weapon_rows",
        "services::agent::deepseek::tests::final_evidence_keeps_the_last_returned_recipe_row",
        "services::agent::source_search::tests::mod_knowledge_search_only_accepts_specific_wiki_pages"
    )
    foreach ($test in $tests) {
        & cargo test --manifest-path (Join-Path $projectRoot "src-tauri\Cargo.toml") `
            $test --lib -- --exact
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    }
}
finally {
    $env:RUSTFLAGS = $previousRustFlags
}
