import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceRoot = path.resolve(
  process.argv[2] ?? path.join(repositoryRoot, "src-tauri/target/analysis/MHW-Editor-source/Assets"),
);
const modelIndexPath = path.join(
  repositoryRoot,
  "references/mhwi-data/curated/model-index.json",
);
const outputPath = path.join(
  repositoryRoot,
  "references/mhwi-data/curated/game-text-zh-hant.json",
);

if (!fs.existsSync(sourceRoot)) {
  throw new Error(`找不到 MHW-Editor Assets 目录：${sourceRoot}`);
}

function pairedTextMap(relativePath) {
  const simplifiedPath = path.join(sourceRoot, relativePath);
  const traditionalPath = simplifiedPath.replace(`${path.sep}chS_`, `${path.sep}chT_`);
  const simplified = JSON.parse(fs.readFileSync(simplifiedPath, "utf8"));
  const traditional = JSON.parse(fs.readFileSync(traditionalPath, "utf8"));
  const result = new Map();

  for (const [key, simplifiedText] of Object.entries(simplified)) {
    const traditionalText = traditional[key];
    if (
      typeof simplifiedText === "string" &&
      simplifiedText.trim() &&
      typeof traditionalText === "string" &&
      traditionalText.trim()
    ) {
      result.set(simplifiedText.trim(), traditionalText.trim());
    }
  }
  return result;
}

function mergeMaps(paths) {
  const merged = new Map();
  const ambiguous = new Set();
  for (const relativePath of paths) {
    for (const [simplified, traditional] of pairedTextMap(relativePath)) {
      if (ambiguous.has(simplified)) {
        continue;
      }
      const existing = merged.get(simplified);
      if (existing && existing !== traditional) {
        // 不同武器表偶尔复用相同简体描述，但繁体描述包含具体武器名；歧义文本不能进入名称索引。
        merged.delete(simplified);
        ambiguous.add(simplified);
        continue;
      }
      merged.set(simplified, traditional);
    }
  }
  return merged;
}

const sourceMaps = {
  weapon: mergeMaps(
    fs
      .readdirSync(path.join(sourceRoot, "WeaponData"))
      .filter((fileName) => fileName.startsWith("chS_") && fileName.endsWith(".json"))
      .map((fileName) => path.join("WeaponData", fileName)),
  ),
  armor: mergeMaps([path.join("ArmorData", "chS_armorData.json")]),
  palicoArmor: mergeMaps([path.join("OtomoData", "chS_otomo_armorData.json")]),
  palicoWeapon: mergeMaps([path.join("OtomoData", "chS_otomo_weaponData.json")]),
  kinsect: mergeMaps([path.join("InsectData", "chS_insectData.json")]),
  pendant: mergeMaps([path.join("PendantData", "chS_pendantData.json")]),
  monster: mergeMaps([path.join("MonsterData", "chS_monsterData.json")]),
};

function findSimplifiedTextFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      return findSimplifiedTextFiles(fullPath);
    }
    return /^chS_.*\.json$/u.test(entry.name)
      ? [path.relative(sourceRoot, fullPath)]
      : [];
  });
}

const globalSourceMap = mergeMaps(findSimplifiedTextFiles(sourceRoot));

const manualNames = new Map([
  ["通用投射器/飞翔爪", "通用投射器／飛翔爪"],
  ["投射器", "投射器"],
  ["独角仙/燕尾蝶关联投射器", "獨角仙／鳳蝶關聯投射器"],
  ["【旅团】服装关联投射器", "【旅團】服裝關聯投射器"],
  ["独角仙后/燕尾蝶男关联投射器", "獨角仙女／鳳蝶男關聯投射器"],
  ["【盛装】服装关联投射器", "【盛裝】服裝關聯投射器"],
  ["【浴场】服装关联投射器", "【浴場】服裝關聯投射器"],
  ["【希里】服装关联投射器", "【希里】服裝關聯投射器"],
  ["【风火轮】服装关联投射器", "【風火輪】服裝關聯投射器"],
]);

function generatedTraditionalName(name) {
  const hairstyleMatch = /^发型 (.+)$/u.exec(name);
  if (hairstyleMatch) {
    return `髮型 ${hairstyleMatch[1]}`;
  }
  const voiceMatch = /^(女性|男性)语音 (\d+) 号$/u.exec(name);
  if (voiceMatch) {
    return `${voiceMatch[1]}語音 ${voiceMatch[2]} 號`;
  }
  return manualNames.get(name);
}

const modelIndex = JSON.parse(fs.readFileSync(modelIndexPath, "utf8"));
const names = new Map();
const missing = new Set();

function collect(entries, sourceMap) {
  for (const entry of entries ?? []) {
    for (const name of entry.displayNames ?? []) {
      const translated = generatedTraditionalName(name) ?? sourceMap?.get(name);
      if (translated) {
        names.set(name, translated);
      } else {
        missing.add(name);
      }
    }
  }
}

collect(modelIndex.weaponModels, sourceMaps.weapon);
collect(modelIndex.weaponRemapTargets, sourceMaps.weapon);
collect(modelIndex.armorModels, sourceMaps.armor);
collect(modelIndex.armorRemapTargets, sourceMaps.armor);
collect(modelIndex.hairModels, globalSourceMap);
for (const kind of Object.keys(sourceMaps)) {
  collect(
    modelIndex.assetModels?.filter((entry) => entry.modelKind === kind),
    sourceMaps[kind],
  );
}
for (const kind of new Set(modelIndex.assetModels?.map((entry) => entry.modelKind))) {
  if (!sourceMaps[kind]) {
    collect(modelIndex.assetModels?.filter((entry) => entry.modelKind === kind), globalSourceMap);
  }
}
collect(modelIndex.palicoArmorRemapTargets, sourceMaps.palicoArmor);
collect(modelIndex.slingerRemapTargets);
collect(modelIndex.voiceModels);

const output = {
  schemaVersion: 1,
  gameVersion: modelIndex.gameVersion,
  locale: "zh-Hant",
  source: {
    title: "Synthlight/MHW-Editor paired chS/chT game text",
    url: "https://github.com/Synthlight/MHW-Editor",
    commit: "a9fd86fd7dbd29fc3f85b1a2ea8ecb0f47458a94",
    method: "按同一游戏文本键配对，不进行简繁字形转换",
  },
  names: Object.fromEntries([...names].sort(([left], [right]) => left.localeCompare(right, "zh-Hans-CN"))),
  fallbackNames: [...missing].sort((left, right) => left.localeCompare(right, "zh-Hans-CN")),
};

fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, "utf8");
console.log(`已生成 ${names.size} 条官方繁体名称，${missing.size} 条名称保留简体回退。`);
