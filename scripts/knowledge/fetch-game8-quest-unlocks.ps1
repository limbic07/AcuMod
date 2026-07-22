param()

$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$pageDirectory = Join-Path $projectRoot "references\knowledge\raw\game8-quest-unlocks\pages"
New-Item -ItemType Directory -Force -Path $pageDirectory | Out-Null

$sources = @(
    @{ Id = "game8-assigned-base"; Url = "https://game8.co/games/Monster-Hunter-World/archives/292425" },
    @{ Id = "game8-assigned-iceborne"; Url = "https://game8.co/games/Monster-Hunter-World/archives/292419" },
    @{ Id = "game8-optional-base"; Url = "https://game8.co/games/Monster-Hunter-World/archives/292426" },
    @{ Id = "game8-optional-iceborne"; Url = "https://game8.co/games/Monster-Hunter-World/archives/296709" },
    @{ Id = "game8-event-base"; Url = "https://game8.co/games/Monster-Hunter-World/archives/296738" },
    @{ Id = "game8-event-iceborne"; Url = "https://game8.co/games/Monster-Hunter-World/archives/292420" },
    @{ Id = "game8-arena"; Url = "https://game8.co/games/Monster-Hunter-World/archives/292416" },
    @{ Id = "game8-challenge"; Url = "https://game8.co/games/Monster-Hunter-World/archives/292417" }
)
foreach ($source in $sources) {
    Invoke-WebRequest -UseBasicParsing -Uri $source.Url -OutFile (Join-Path $pageDirectory "$($source.Id).html")
}

$previousSourceDirectory = $env:ACUMOD_QUEST_HTML_DIR
$env:ACUMOD_QUEST_HTML_DIR = $pageDirectory
try {
    & node (Join-Path $PSScriptRoot "fetch-game8-quest-unlocks.mjs")
    exit $LASTEXITCODE
}
finally {
    $env:ACUMOD_QUEST_HTML_DIR = $previousSourceDirectory
}
