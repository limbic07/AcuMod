param(
    [string]$RawPackagePath,
    [string]$HairSourcePath,
    [string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($RawPackagePath)) {
    $RawPackagePath = Join-Path $repositoryRoot "references/mhwi-data/raw/15.10.00-agent-package"
}

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $repositoryRoot "references/mhwi-data/curated/model-index.json"
}

if ([string]::IsNullOrWhiteSpace($HairSourcePath)) {
    $HairSourcePath = Join-Path $repositoryRoot "references/mhwi-data/curated/sources/hairstyles.json"
}

$weaponCsvPath = Join-Path $RawPackagePath "csv/02_weapons.csv"
$armorCsvPath = Join-Path $RawPackagePath "csv/03_armor.csv"

if (-not (Test-Path -LiteralPath $weaponCsvPath -PathType Leaf)) {
    throw "Weapon data was not found: $weaponCsvPath"
}

if (-not (Test-Path -LiteralPath $armorCsvPath -PathType Leaf)) {
    throw "Armor data was not found: $armorCsvPath"
}

if (-not (Test-Path -LiteralPath $HairSourcePath -PathType Leaf)) {
    throw "Curated hairstyle data was not found: $HairSourcePath"
}

function Normalize-ModelPath {
    param([string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value.Trim() -eq "无") {
        return $null
    }

    return (($Value -replace '\s*\(.*$', '').Trim() -replace '\\', '/').ToLowerInvariant()
}

function Test-DisplayName {
    param([string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $false
    }

    $normalized = $Value.Trim()
    return $normalized -ne "Unavailable" -and
        $normalized -ne "Invalid Message" -and
        -not $normalized.StartsWith("dummy", [System.StringComparison]::OrdinalIgnoreCase)
}

function Get-SortedIds {
    param([object[]]$Values)

    return @($Values | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
        ForEach-Object { [string]$_ } | Sort-Object { [int]$_ } -Unique)
}

function Get-SortedNames {
    param([object[]]$Values)

    return @($Values | Where-Object { Test-DisplayName ([string]$_) } |
        ForEach-Object { ([string]$_).Trim() } | Sort-Object -Unique)
}

$weaponRows = Import-Csv -LiteralPath $weaponCsvPath -Encoding utf8
$armorRows = Import-Csv -LiteralPath $armorCsvPath -Encoding utf8
$hairSource = Get-Content -LiteralPath $HairSourcePath -Raw -Encoding utf8 | ConvertFrom-Json

$weaponModelRows = foreach ($row in $weaponRows) {
    $mainModelPath = Normalize-ModelPath $row.主模型地址
    if ($null -ne $mainModelPath) {
        [pscustomobject]@{
            modelPath = $mainModelPath
            modelPart = "main"
            weaponType = $row.武器类型
            weaponTypeId = $row.武器类型ID
            weaponId = $row.武器ID
            displayName = $row.武器名称
        }
    }

    $accessoryModelPath = Normalize-ModelPath $row.附件模型地址
    if ($null -ne $accessoryModelPath) {
        [pscustomobject]@{
            modelPath = $accessoryModelPath
            modelPart = "accessory"
            weaponType = $row.武器类型
            weaponTypeId = $row.武器类型ID
            weaponId = $row.武器ID
            displayName = $row.武器名称
        }
    }
}

$weaponModels = @($weaponModelRows |
    Group-Object modelPath, modelPart, weaponType, weaponTypeId |
    ForEach-Object {
        $first = $_.Group[0]
        [pscustomobject]@{
            modelPath = $first.modelPath
            modelPart = $first.modelPart
            weaponType = $first.weaponType
            weaponTypeId = [string]$first.weaponTypeId
            weaponIds = @(Get-SortedIds @($_.Group.weaponId))
            displayNames = @(Get-SortedNames @($_.Group.displayName))
        }
    } |
    Sort-Object { [int]$_.weaponTypeId }, modelPath, modelPart)

$armorModelRows = foreach ($row in $armorRows) {
    $modelPath = Normalize-ModelPath $row.模型地址
    if ($null -eq $modelPath -or $row.部位名称 -eq "护石") {
        continue
    }

    [pscustomobject]@{
        modelPath = $modelPath
        armorPart = $row.部位名称
        armorPartId = $row.部位ID
        armorId = $row.防具ID
        layeredArmorId = $row.幻化ID
        displayName = $row.防具名称
    }
}

$armorModels = @($armorModelRows |
    Group-Object modelPath, armorPart, armorPartId |
    ForEach-Object {
        $first = $_.Group[0]
        [pscustomobject]@{
            modelPath = $first.modelPath
            armorPart = $first.armorPart
            armorPartId = [string]$first.armorPartId
            armorIds = @(Get-SortedIds @($_.Group.armorId))
            layeredArmorIds = @(Get-SortedIds @($_.Group.layeredArmorId))
            displayNames = @(Get-SortedNames @($_.Group.displayName))
        }
    } |
    Sort-Object { [int]$_.armorPartId }, modelPath)

$hairModels = @($hairSource.hairstyles |
    ForEach-Object {
        $modelPath = Normalize-ModelPath $_.modelPath
        if ($null -eq $modelPath -or $modelPath -notmatch '/hair[0-9]+$') {
            throw "Invalid hairstyle model path: $($_.modelPath)"
        }

        $hairstyleId = [string]$_.hairstyleId
        $hasNumericSlot = $hairstyleId -match '^\d+-\d+$'
        $displayNames = @()

        if ($hasNumericSlot) {
            $displayNames += "发型 $hairstyleId"
        }

        if (-not [string]::IsNullOrWhiteSpace($_.displayName)) {
            $displayNames += ([string]$_.displayName).Trim()
        }

        [pscustomobject]@{
            modelPath = $modelPath
            modelId = Split-Path -Leaf $modelPath
            gameIds = @($(if ($hasNumericSlot) { $hairstyleId }))
            displayNames = @($displayNames)
            category = [string]$_.category
        }
    } |
    Sort-Object modelPath)

if ($hairModels.Count -ne $hairSource.hairstyles.Count) {
    throw "Some curated hairstyle entries were not generated."
}

if (($hairModels.modelPath | Sort-Object -Unique).Count -ne $hairModels.Count) {
    throw "Curated hairstyle model paths must be unique."
}

$index = [ordered]@{
    schemaVersion = 3
    gameVersion = "15.10.00"
    sourceFiles = @("02_weapons.csv", "03_armor.csv", "curated/sources/hairstyles.json")
    sourceReferences = @($hairSource.source)
    weaponModels = $weaponModels
    armorModels = $armorModels
    hairModels = $hairModels
}

$outputDirectory = Split-Path -Parent $OutputPath
[System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$json = $index | ConvertTo-Json -Depth 8 -Compress
[System.IO.File]::WriteAllText($OutputPath, $json, [System.Text.UTF8Encoding]::new($false))

Write-Output "Generated $($weaponModels.Count) weapon, $($armorModels.Count) armor, and $($hairModels.Count) hairstyle model entries."
Write-Output "Output: $OutputPath"
