param(
    [string]$RawPackagePath,
    [string]$HairSourcePath,
    [string]$ExtendedAssetSourcePath,
    [string]$AdditionalAssetSourcePath,
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

if ([string]::IsNullOrWhiteSpace($ExtendedAssetSourcePath)) {
    $ExtendedAssetSourcePath = Join-Path $repositoryRoot "references/mhwi-data/curated/sources/extended-assets.json"
}

if ([string]::IsNullOrWhiteSpace($AdditionalAssetSourcePath)) {
    $AdditionalAssetSourcePath = Join-Path $repositoryRoot "references/mhwi-data/curated/sources/additional-assets.json"
}

$weaponCsvPath = Join-Path $RawPackagePath "csv/02_weapons.csv"
$armorCsvPath = Join-Path $RawPackagePath "csv/03_armor.csv"
$palicoWeaponCsvPath = Join-Path $RawPackagePath "csv/09_palico_weapons.csv"
$palicoArmorCsvPath = Join-Path $RawPackagePath "csv/10_palico_armor.csv"
$pendantCsvPath = Join-Path $RawPackagePath "csv/12_pendants.csv"
$kinsectCsvPath = Join-Path $RawPackagePath "csv/13_kinsects.csv"
$npcCsvPath = Join-Path $RawPackagePath "csv/17_npc.csv"
$monsterCsvPath = Join-Path $RawPackagePath "csv/06_monsters.csv"
$poogieCsvPath = Join-Path $RawPackagePath "csv/18_poogie.csv"

foreach ($requiredDataPath in @(
        $weaponCsvPath,
        $armorCsvPath,
        $palicoWeaponCsvPath,
        $palicoArmorCsvPath,
        $pendantCsvPath,
        $kinsectCsvPath,
        $npcCsvPath,
        $monsterCsvPath,
        $poogieCsvPath
    )) {
    if (-not (Test-Path -LiteralPath $requiredDataPath -PathType Leaf)) {
        throw "Required MHWI data was not found: $requiredDataPath"
    }
}

if (-not (Test-Path -LiteralPath $HairSourcePath -PathType Leaf)) {
    throw "Curated hairstyle data was not found: $HairSourcePath"
}

if (-not (Test-Path -LiteralPath $ExtendedAssetSourcePath -PathType Leaf)) {
    throw "Curated extended asset data was not found: $ExtendedAssetSourcePath"
}

if (-not (Test-Path -LiteralPath $AdditionalAssetSourcePath -PathType Leaf)) {
    throw "Curated additional asset data was not found: $AdditionalAssetSourcePath"
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

function Get-SortedTextIds {
    param([object[]]$Values)

    return @($Values | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
        ForEach-Object { ([string]$_).Trim() } | Sort-Object -Unique)
}

function Get-ModelId {
    param([string]$ModelPath)

    return ($ModelPath -split '/')[-1]
}

$weaponRows = Import-Csv -LiteralPath $weaponCsvPath -Encoding utf8
$armorRows = Import-Csv -LiteralPath $armorCsvPath -Encoding utf8
$palicoWeaponRows = Import-Csv -LiteralPath $palicoWeaponCsvPath -Encoding utf8
$palicoArmorRows = Import-Csv -LiteralPath $palicoArmorCsvPath -Encoding utf8
$pendantRows = Import-Csv -LiteralPath $pendantCsvPath -Encoding utf8
$kinsectRows = Import-Csv -LiteralPath $kinsectCsvPath -Encoding utf8
$npcRows = Import-Csv -LiteralPath $npcCsvPath -Encoding utf8
$monsterRows = Import-Csv -LiteralPath $monsterCsvPath -Encoding utf8
$poogieRows = Import-Csv -LiteralPath $poogieCsvPath -Encoding utf8
$hairSource = Get-Content -LiteralPath $HairSourcePath -Raw -Encoding utf8 | ConvertFrom-Json
$extendedAssetSource = Get-Content -LiteralPath $ExtendedAssetSourcePath -Raw -Encoding utf8 | ConvertFrom-Json
$additionalAssetSource = Get-Content -LiteralPath $AdditionalAssetSourcePath -Raw -Encoding utf8 | ConvertFrom-Json

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

# Recognition is grouped by one resource path, while remapping needs the main
# and optional accessory paths that form one usable in-game appearance.
$weaponRemapRows = foreach ($row in $weaponRows) {
    $mainModelPath = Normalize-ModelPath $row.主模型地址
    if ($null -eq $mainModelPath) {
        continue
    }

    [pscustomobject]@{
        weaponType = [string]$row.武器类型
        weaponTypeId = [string]$row.武器类型ID
        mainModelPath = $mainModelPath
        accessoryModelPath = Normalize-ModelPath $row.附件模型地址
        weaponId = [string]$row.武器ID
        displayName = [string]$row.武器名称
    }
}

$weaponRemapTargets = @($weaponRemapRows |
    Group-Object weaponType, weaponTypeId, mainModelPath, accessoryModelPath |
    ForEach-Object {
        $first = $_.Group[0]
        $modelPaths = @($first.mainModelPath)
        if ($null -ne $first.accessoryModelPath) {
            $modelPaths += $first.accessoryModelPath
        }

        [pscustomobject]@{
            targetId = "weapon:$($first.weaponTypeId):$($first.mainModelPath)|$($first.accessoryModelPath)"
            weaponType = $first.weaponType
            weaponTypeId = $first.weaponTypeId
            mainModelPath = $first.mainModelPath
            accessoryModelPath = $first.accessoryModelPath
            modelPaths = @($modelPaths)
            gameIds = @(Get-SortedIds @($_.Group.weaponId))
            displayNames = @(Get-SortedNames @($_.Group.displayName))
        }
    } |
    Sort-Object { [int]$_.weaponTypeId }, mainModelPath, accessoryModelPath)

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

$armorRemapTargets = @($armorModelRows |
    Group-Object modelPath |
    ForEach-Object {
        $first = $_.Group[0]
        [pscustomobject]@{
            targetId = "armor:$($first.modelPath)"
            modelId = $first.modelPath
            gameIds = @(Get-SortedIds @($_.Group.armorId))
            variantIds = @(Get-SortedIds @($_.Group.layeredArmorId))
            displayNames = @(Get-SortedNames @($_.Group.displayName))
            affectedParts = @($_.Group | Sort-Object { [int]$_.armorPartId } | ForEach-Object { $_.armorPart } | Select-Object -Unique)
        }
    } |
    Sort-Object modelId)

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

$assetModelRows = @(
    $palicoWeaponRows | ForEach-Object {
        $modelPath = Normalize-ModelPath $_.模型地址
        if ($null -ne $modelPath) {
            [pscustomobject]@{
                modelKind = "palicoWeapon"
                subKind = "随从武器"
                modelPart = "model"
                modelPath = $modelPath
                modelId = Get-ModelId $modelPath
                gameId = [string]$_.武器ID
                displayName = $_.武器名称
            }
        }
    }

    $palicoArmorRows | ForEach-Object {
        $modelPath = Normalize-ModelPath $_.模型地址
        if ($null -ne $modelPath) {
            [pscustomobject]@{
                modelKind = "palicoArmor"
                subKind = [string]$_.部位名称
                modelPart = "model"
                modelPath = $modelPath
                modelId = ($modelPath -split '/')[2]
                gameId = [string]$_.防具ID
                displayName = $_.防具名称
            }
        }
    }

    $kinsectRows | ForEach-Object {
        $modelPath = Normalize-ModelPath $_.模型地址
        if ($null -ne $modelPath) {
            [pscustomobject]@{
                modelKind = "kinsect"
                subKind = "猎虫"
                modelPart = "model"
                modelPath = $modelPath
                modelId = Get-ModelId $modelPath
                gameId = [string]$_.猎虫ID
                displayName = $_.猎虫名称
            }
        }
    }

    $pendantRows | ForEach-Object {
        $modelPath = Normalize-ModelPath $_.模型地址
        if ($null -ne $modelPath) {
            [pscustomobject]@{
                modelKind = "pendant"
                subKind = "挂件"
                modelPart = "model"
                modelPath = $modelPath
                modelId = Get-ModelId $modelPath
                gameId = [string]$_.吊坠ID
                displayName = $_.吊坠名称
            }
        }
    }

    $npcRows | ForEach-Object {
        $modelPath = Normalize-ModelPath $_.模型地址
        if ($null -ne $modelPath) {
            [pscustomobject]@{
                modelKind = "npc"
                subKind = "NPC"
                modelPart = "model"
                modelPath = $modelPath
                modelId = Get-ModelId $modelPath
                gameId = [string]$_.NPC代码
                displayName = $_.NPC名称
            }
        }
    }

    $monsterRows | ForEach-Object {
        $modelId = ([string]$_.怪物代码).Trim().ToLowerInvariant()
        if ($modelId -match '^(?:em|ems)[0-9]{3}(?:_[0-9]{2}|_[0-9]{2}_[a-z])?$') {
            [pscustomobject]@{
                modelKind = "monster"
                subKind = "怪物"
                modelPart = "model"
                modelPath = "em/$modelId"
                modelId = $modelId
                gameId = [string]$_.怪物ID
                displayName = $_.怪物名称
            }
        }
    }

    $poogieNameById = @{}
    $poogieRows | ForEach-Object {
        $poogieNameById[[string]$_.小猪服装ID] = [string]$_.小猪服装名
    }

    $additionalAssetSource.poogieModels | ForEach-Object {
        $modelPath = Normalize-ModelPath $_.modelPath
        $poogieId = [string]$_.poogieId
        if ($null -eq $modelPath -or $modelPath -notmatch '^pg/pg[0-9]{3}$') {
            throw "Invalid Poogie model path: $($_.modelPath)"
        }

        $displayName = $poogieNameById[$poogieId]
        if (-not (Test-DisplayName $displayName)) {
            throw "Poogie model $modelPath has no matching local Chinese name for ID $poogieId."
        }

        [pscustomobject]@{
            modelKind = "poogie"
            subKind = "噗吱猪服装"
            modelPart = "model"
            modelPath = $modelPath
            modelId = Get-ModelId $modelPath
            gameId = $poogieId
            displayName = $displayName
        }
    }

    $extendedAssetSource.slingerModels | ForEach-Object {
        $modelPath = Normalize-ModelPath $_.modelPath
        if ($null -eq $modelPath -or $modelPath -notmatch '^wp/slg/slg[0-9]{3}(?:_[0-9]{4})?$') {
            throw "Invalid slinger model path: $($_.modelPath)"
        }

        foreach ($gameId in $_.gameIds) {
            foreach ($displayName in $_.displayNames) {
                [pscustomobject]@{
                    modelKind = "slinger"
                    subKind = "投射器"
                    modelPart = "model"
                    modelPath = $modelPath
                    modelId = [string]$_.modelId
                    gameId = [string]$gameId
                    displayName = [string]$displayName
                }
            }
        }
    }

)

$assetModels = @($assetModelRows |
    Group-Object modelKind, subKind, modelPart, modelPath, modelId |
    ForEach-Object {
        $first = $_.Group[0]
        $gameIds = if ($first.modelKind -in @("npc", "slinger")) {
            @(Get-SortedTextIds @($_.Group.gameId))
        }
        else {
            @(Get-SortedIds @($_.Group.gameId))
        }

        [pscustomobject]@{
            modelKind = $first.modelKind
            subKind = $first.subKind
            modelPart = $first.modelPart
            modelPath = $first.modelPath
            modelId = $first.modelId
            gameIds = @($gameIds)
            variantIds = @()
            displayNames = @(Get-SortedNames @($_.Group.displayName))
        }
    } |
    Sort-Object modelKind, modelPath, subKind)

$palicoArmorRemapTargets = @($palicoArmorRows |
    ForEach-Object {
        $modelPath = Normalize-ModelPath $_.模型地址
        if ($null -ne $modelPath -and $modelPath -match '^otomo/equip/(ot[0-9]{3})/(helm|body)$') {
            [pscustomobject]@{
                modelId = $Matches[1]
                armorId = [string]$_.防具ID
                displayName = [string]$_.防具名称
                armorPart = [string]$_.部位名称
            }
        }
    } |
    Group-Object modelId |
    ForEach-Object {
        $first = $_.Group[0]
        [pscustomobject]@{
            targetId = "palicoArmor:$($first.modelId)"
            modelId = $first.modelId
            gameIds = @(Get-SortedIds @($_.Group.armorId))
            displayNames = @(Get-SortedNames @($_.Group.displayName))
            affectedParts = @($_.Group.armorPart | Sort-Object -Unique)
        }
    } |
    Sort-Object modelId)

$slingerRemapTargets = @($assetModels |
    Where-Object { $_.modelKind -eq "slinger" } |
    ForEach-Object {
        [pscustomobject]@{
            targetId = "slinger:$($_.modelId)"
            modelId = $_.modelId
            gameIds = @($_.gameIds)
            displayNames = @($_.displayNames)
        }
    } |
    Sort-Object modelId)

$voiceModels = @($extendedAssetSource.voiceModels |
    ForEach-Object {
        $genderLabel = switch ([string]$_.gender) {
            "female" { "女性" }
            "male" { "男性" }
            default { throw "Unsupported character voice gender: $($_.gender)" }
        }
        $fileName = ([string]$_.fileName).ToLowerInvariant()
        if ($fileName -notmatch '^pl_act_vo_[fm]_[0-9]{2}_m\.nbnk$') {
            throw "Invalid character voice file name: $($_.fileName)"
        }

        [pscustomobject]@{
            fileName = $fileName
            modelId = [System.IO.Path]::GetFileNameWithoutExtension($fileName)
            gender = [string]$_.gender
            voiceNumber = [string]$_.voiceNumber
            displayNames = @("${genderLabel}语音 $($_.voiceNumber) 号")
        }
    } |
    Sort-Object gender, { [int]$_.voiceNumber })

$index = [ordered]@{
    schemaVersion = 7
    gameVersion = "15.10.00"
    sourceFiles = @(
        "02_weapons.csv",
        "03_armor.csv",
        "09_palico_weapons.csv",
        "10_palico_armor.csv",
        "12_pendants.csv",
        "13_kinsects.csv",
        "17_npc.csv",
        "06_monsters.csv",
        "18_poogie.csv",
        "curated/sources/hairstyles.json",
        "curated/sources/extended-assets.json",
        "curated/sources/additional-assets.json"
    )
    sourceReferences = @($hairSource.source) + @($extendedAssetSource.sources) + @($additionalAssetSource.sources)
    weaponModels = $weaponModels
    weaponRemapTargets = $weaponRemapTargets
    armorModels = $armorModels
    armorRemapTargets = $armorRemapTargets
    hairModels = $hairModels
    assetModels = $assetModels
    palicoArmorRemapTargets = $palicoArmorRemapTargets
    slingerRemapTargets = $slingerRemapTargets
    voiceModels = $voiceModels
}

$outputDirectory = Split-Path -Parent $OutputPath
[System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$json = $index | ConvertTo-Json -Depth 8 -Compress
[System.IO.File]::WriteAllText($OutputPath, $json, [System.Text.UTF8Encoding]::new($false))

Write-Output "Generated $($weaponModels.Count) weapon recognition and $($weaponRemapTargets.Count) weapon remap entries."
Write-Output "Generated $($armorModels.Count) armor recognition, $($armorRemapTargets.Count) armor remap, $($palicoArmorRemapTargets.Count) Palico armor remap, $($hairModels.Count) hairstyle, $($slingerRemapTargets.Count) slinger, $($assetModels.Count) extended asset, and $($voiceModels.Count) voice entries."
Write-Output "Output: $OutputPath"
