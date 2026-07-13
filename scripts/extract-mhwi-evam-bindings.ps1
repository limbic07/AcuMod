param(
    [Parameter(Mandatory = $true)]
    [string]$EvamRoot,
    [string]$OutputPath,
    [string]$GameVersion = "15.10.00",
    [string]$SteamBuildId = "15539686"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $repositoryRoot "references/mhwi-data/curated/sources/armor-slinger-bindings.json"
}

if (-not (Test-Path -LiteralPath $EvamRoot -PathType Container)) {
    throw "Extracted EVAM root was not found: $EvamRoot"
}

$resolvedRoot = (Resolve-Path -LiteralPath $EvamRoot).Path
$rootPrefix = $resolvedRoot.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
) + [System.IO.Path]::DirectorySeparatorChar
$files = @(Get-ChildItem -LiteralPath $resolvedRoot -Recurse -Filter "*.evam" -File)
if ($files.Count -eq 0) {
    throw "No EVAM files were found under: $resolvedRoot"
}

$bindings = foreach ($file in $files) {
    if (-not $file.FullName.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "EVAM file is outside the requested extraction root: $($file.FullName)"
    }
    $relativePath = $file.FullName.Substring($rootPrefix.Length).Replace("\", "/")
    $match = [regex]::Match(
        $relativePath,
        '^pl/(?<gender>[fm])_equip/(?<armor>pl(?<armorNumber>[0-9]{3})_(?<variant>[0-9]{4}))/arm/mod/(?<fileGender>[fm])_arm(?<fileModel>[0-9]{3}_[0-9]{4})\.evam$',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    if (-not $match.Success) {
        throw "EVAM path does not match the supported armor layout: $relativePath"
    }

    $genderCode = $match.Groups["gender"].Value.ToLowerInvariant()
    if ($genderCode -ne $match.Groups["fileGender"].Value.ToLowerInvariant()) {
        throw "EVAM directory and file gender do not match: $relativePath"
    }
    if ($match.Groups["armor"].Value.Substring(2) -ne $match.Groups["fileModel"].Value) {
        throw "EVAM armor directory and file model IDs do not match: $relativePath"
    }

    $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
    if ($bytes.Length -ne 26) {
        throw "EVAM must be exactly 26 bytes: $relativePath"
    }
    if ([System.BitConverter]::ToString($bytes, 0, 4) -ne "01-10-09-18") {
        throw "EVAM prefix is invalid: $relativePath"
    }
    if ([System.Text.Encoding]::ASCII.GetString($bytes, 4, 4) -ne "EVAM") {
        throw "EVAM marker is invalid: $relativePath"
    }
    if ([System.BitConverter]::ToUInt32($bytes, 8) -ne 3) {
        throw "Only EVAM version 3 is supported: $relativePath"
    }

    $rawSlingerId = [System.BitConverter]::ToUInt32($bytes, 16)
    $hasSlinger = $rawSlingerId -ne [uint32]::MaxValue
    $armorNumber = [int]$match.Groups["armorNumber"].Value
    $gender = if ($genderCode -eq "f") { "female" } else { "male" }

    [pscustomobject][ordered]@{
        armorModelId = $match.Groups["armor"].Value.ToLowerInvariant()
        gender = $gender
        slingerId = $(if ($hasSlinger) { [int]$rawSlingerId } else { $null })
        slingerModelId = $(if ($hasSlinger) { "slg{0:D3}_0000" -f $rawSlingerId } else { $null })
        matchesArmorBaseId = $hasSlinger -and $armorNumber -eq $rawSlingerId
        sourcePath = $relativePath
    }
}

$bindings = @($bindings | Sort-Object armorModelId, @{ Expression = { if ($_.gender -eq "female") { 0 } else { 1 } } })
$duplicateKeys = @($bindings | Group-Object armorModelId, gender | Where-Object Count -gt 1)
if ($duplicateKeys.Count -gt 0) {
    throw "Duplicate armor and gender EVAM bindings were found: $($duplicateKeys.Name -join ', ')"
}

$armorGroups = @($bindings | Group-Object armorModelId)
$genderMismatchCount = @($armorGroups | Where-Object {
        @($_.Group | ForEach-Object {
                if ($null -eq $_.slingerId) { "none" } else { [string]$_.slingerId }
            } | Sort-Object -Unique).Count -gt 1
    }).Count

$result = [ordered]@{
    schemaVersion = 1
    gameVersion = $GameVersion
    source = [ordered]@{
        title = "Monster Hunter: World Iceborne original armor EVAM bindings"
        provenance = "localSteamGameChunks"
        steamAppId = "582010"
        steamBuildId = $SteamBuildId
        chunkOrder = "Available chunkG*.bin files sorted by numeric suffix; later numeric chunks override earlier files"
        extractionTool = "WorldChunkTool"
        extractionToolUrl = "https://github.com/mhvuze/WorldChunkTool"
        extractionToolCommit = "2ad4fc848c84e7df742ee38eca0f3b5f7a677ab0"
        sourcePathPattern = "pl/{f,m}_equip/plNNN_NNNN/arm/mod/{f,m}_armNNN_NNNN.evam"
        licenseNote = "Only derived IDs and virtual source paths are stored. Original EVAM files are not redistributed."
    }
    statistics = [ordered]@{
        bindingCount = $bindings.Count
        armorModelCount = $armorGroups.Count
        femaleBindingCount = @($bindings | Where-Object gender -eq "female").Count
        maleBindingCount = @($bindings | Where-Object gender -eq "male").Count
        noSlingerBindingCount = @($bindings | Where-Object { $null -eq $_.slingerId }).Count
        nonMatchingBaseIdCount = @($bindings | Where-Object { $null -ne $_.slingerId -and -not $_.matchesArmorBaseId }).Count
        genderMismatchArmorModelCount = $genderMismatchCount
    }
    bindings = $bindings
}

$outputDirectory = Split-Path -Parent $OutputPath
[System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$json = $result | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($OutputPath, $json + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))

Write-Output "Generated $($bindings.Count) original EVAM bindings for $($armorGroups.Count) armor models."
Write-Output "Gender-specific differences: $genderMismatchCount; no-slinger bindings: $($result.statistics.noSlingerBindingCount)."
Write-Output "Output: $OutputPath"
