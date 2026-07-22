import { copyFile, mkdir, readFile, rm } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";
import process from "node:process";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const modelIndexPath = path.join(
  projectRoot,
  "references/mhwi-data/curated/model-index.json",
);
const sourceCatalogPath = path.join(
  projectRoot,
  "references/knowledge/sources/catalog.json",
);
const moddingDocumentsPath = path.join(
  projectRoot,
  "references/knowledge/sources/modding-documents.json",
);
const gameGuideDocumentsPath = path.join(
  projectRoot,
  "references/knowledge/sources/game-guide-documents.json",
);
const locationNameMapPath = path.join(
  projectRoot,
  "references/knowledge/sources/location-name-map.json",
);
const curatedQuestNameMapPath = path.join(
  projectRoot,
  "references/knowledge/sources/quest-name-map.json",
);
const rawGameDataRoot = path.join(
  projectRoot,
  "references/mhwi-data/raw/15.10.00-agent-package/jsonl",
);
const mhwDbSnapshotPath = path.join(
  projectRoot,
  "references/knowledge/raw/mhw-db/current.json",
);
const mhworldDataArmorMapPath = path.join(
  projectRoot,
  "references/knowledge/raw/mhworlddata/armor-name-map.json",
);
const game8QuestUnlockSnapshotPath = path.join(
  projectRoot,
  "references/knowledge/raw/game8-quest-unlocks/current.json",
);
const fullGameTextBridgePath = path.join(
  projectRoot,
  "references/knowledge/raw/mhw-editor/game-text-bridge.json",
);
const defaultOutputPath = path.join(
  projectRoot,
  "references/knowledge/build/acumod-dev-game-facts.acukb",
);
const targetGameVersion = "15.23";
const contentBaselineVersion = "15.10.00";
let englishGameTextNames = new Map();

function outputPathFromArguments(argv) {
  const outputIndex = argv.findIndex((argument) => argument === "--output");
  if (outputIndex >= 0 && argv[outputIndex + 1]) {
    return path.resolve(argv[outputIndex + 1]);
  }
  const inline = argv.find((argument) => argument.startsWith("--output="));
  return inline ? path.resolve(inline.slice("--output=".length)) : defaultOutputPath;
}

function uniqueStrings(values) {
  return [...new Set(values.filter((value) => typeof value === "string" && value.trim()))];
}

function displayTitle(record, fallback) {
  return uniqueStrings(record.displayNames ?? [])[0] ?? fallback;
}

function entityRows(modelIndex) {
  const rows = [];
  for (const target of modelIndex.weaponRemapTargets ?? []) {
    rows.push({
      id: target.targetId,
      kind: "weapon",
      domain: "game-equipment",
      title: displayTitle(target, target.targetId),
      summary: `${target.weaponType}；模型 ${target.modelPaths.join("、")}`,
      aliases: uniqueStrings([
        ...(target.displayNames ?? []),
        ...(target.gameIds ?? []),
        ...(target.modelPaths ?? []),
        target.weaponType,
      ]),
      data: target,
    });
  }
  for (const target of modelIndex.armorRemapTargets ?? []) {
    rows.push({
      id: target.targetId,
      kind: "armor",
      domain: "game-equipment",
      title: displayTitle(target, target.modelId),
      summary: `防具模型 ${target.modelId}`,
      aliases: uniqueStrings([
        ...(target.displayNames ?? []),
        ...(target.gameIds ?? []),
        ...(target.variantIds ?? []),
        target.modelId,
      ]),
      data: target,
    });
  }
  for (const target of modelIndex.hairModels ?? []) {
    rows.push({
      id: `hair:${target.modelId}`,
      kind: "hairstyle",
      domain: "game-appearance",
      title: displayTitle(target, target.modelId),
      summary: `发型模型 ${target.modelPath}`,
      aliases: uniqueStrings([
        ...(target.displayNames ?? []),
        ...(target.gameIds ?? []),
        target.modelId,
        target.modelPath,
      ]),
      data: target,
    });
  }
  for (const target of modelIndex.assetModels ?? []) {
    // 同一随从套装会分别包含头盔和铠甲，部位是稳定实体 ID 的必要组成。
    const entityPart = target.modelPath || target.subKind || target.modelPart || "model";
    rows.push({
      id: `${target.modelKind}:${target.modelId}:${entityPart}`,
      kind: target.modelKind,
      domain: "game-assets",
      title: displayTitle(target, target.modelId),
      summary: `${target.subKind}；模型 ${target.modelPath}`,
      aliases: uniqueStrings([
        ...(target.displayNames ?? []),
        ...(target.gameIds ?? []),
        target.modelId,
        target.modelPath,
      ]),
      data: target,
    });
  }
  return rows;
}

function meaningfulText(value) {
  if (typeof value !== "string") {
    return false;
  }
  const normalized = value.trim();
  return Boolean(
    normalized
      && normalized !== "Unavailable"
      && normalized !== "Invalid Message"
      && normalized !== "无"
      && normalized !== "－－－－－－"
      && !normalized.toLowerCase().startsWith("dummy"),
  );
}

function valueText(value) {
  return typeof value === "string" ? value.trim() : "";
}

function romanNumeral(value) {
  const numerals = [
    [10, "X"],
    [9, "IX"],
    [5, "V"],
    [4, "IV"],
    [1, "I"],
  ];
  let remaining = value;
  let result = "";
  for (const [amount, numeral] of numerals) {
    while (remaining >= amount) {
      result += numeral;
      remaining -= amount;
    }
  }
  return result;
}

function displayNameAliases(value) {
  const text = valueText(value);
  if (!text) return [];

  // 事实条目会追加“属性资料”等显示后缀；玩家通常按游戏名搜索，武器等级也常写成罗马数字。
  const aliases = [text];
  const withoutSuffix = text.replace(/\s*[（(][^（）()]*[）)]\s*$/, "").trim();
  if (withoutSuffix && withoutSuffix !== text) {
    aliases.push(withoutSuffix);
  }
  for (const candidate of [...aliases]) {
    const withoutSlotBrackets = candidate.replace(/[【】]/g, "");
    if (withoutSlotBrackets !== candidate) {
      aliases.push(withoutSlotBrackets);
    }
    const asciiDigits = candidate.replace(/[０-９]/g, (digit) => String.fromCharCode(digit.charCodeAt(0) - 0xFEE0));
    if (asciiDigits !== candidate) {
      aliases.push(asciiDigits);
    }
  }
  for (const candidate of [...aliases]) {
    if (!/[A-Za-z\u4e00-\u9fff]/.test(candidate)) continue;
    const romanized = candidate.replace(/(\d+)(?=\s*(?:[（(]|$))/g, (match) => {
      const number = Number(match);
      return Number.isInteger(number) && number > 0 && number <= 39
        ? romanNumeral(number)
        : match;
    });
    if (romanized !== candidate) {
      aliases.push(romanized);
    }
  }
  return aliases;
}

function rawAliasValues(...values) {
  return uniqueStrings(values.flatMap((value) => {
    if (Array.isArray(value)) {
      return value.flatMap(displayNameAliases);
    }
    return typeof value === "string" ? displayNameAliases(value) : [];
  }).filter(meaningfulText));
}

function rawEntity(id, kind, domain, name, summary, aliases, data) {
  return {
    id,
    kind,
    domain,
    title: name,
    nameZhHans: name,
    // 英文别名只来自同一游戏文本键的本地资源，不使用逐字翻译或模型推断。
    aliases: rawAliasValues(name, ...aliases, englishGameTextNames.get(name)),
    summary,
    // 内容事实以 15.10.00 原始数据为基线；项目已确认后续版本没有新增游戏内容或机制。
    data: { ...data, contentBaselineVersion },
    gameVersion: targetGameVersion,
    confidence: 0.85,
    sourceId: "mhwmod-jodo-ice-15.10",
  };
}

const rawGameTables = [
  {
    file: "02_weapons.jsonl",
    entity: (row) => {
      const typeId = valueText(row["武器类型ID"]);
      const weaponId = valueText(row["武器ID"]);
      const name = valueText(row["武器名称"]);
      if (!meaningfulText(name) || !typeId || !weaponId) return null;
      const weaponType = valueText(row["武器类型"]);
      const rarity = valueText(row["稀有度"]);
      const mainModelPath = valueText(row["主模型地址"]);
      const accessoryModelPath = valueText(row["附件模型地址"]);
      return rawEntity(
        `game-weapon:${typeId}:${weaponId}`,
        "weapon",
        "game-equipment",
        name,
        `${weaponType}；稀有度 ${rarity || "未知"}；主模型 ${mainModelPath || "未知"}`,
        [weaponType, typeId, weaponId, mainModelPath, accessoryModelPath],
        { typeId, weaponType, weaponId, rarity, modelType: valueText(row["模型类型"]), mainModelPath, accessoryModelPath },
      );
    },
  },
  {
    file: "03_armor.jsonl",
    entity: (row) => {
      const partId = valueText(row["部位ID"]);
      const armorId = valueText(row["防具ID"]);
      const name = valueText(row["防具名称"]);
      if (!meaningfulText(name) || !partId || !armorId) return null;
      const part = valueText(row["部位名称"]);
      const modelPath = valueText(row["模型地址"]);
      return rawEntity(
        `game-armor:${partId}:${armorId}`,
        "armor",
        "game-equipment",
        name,
        `${part}；防御 ${valueText(row["防御"]) || "未知"}；稀有度 ${valueText(row["稀有度"]) || "未知"}`,
        [part, armorId, valueText(row["幻化ID"]), modelPath],
        { partId, part, armorId, layeredArmorId: valueText(row["幻化ID"]), defense: valueText(row["防御"]), rarity: valueText(row["稀有度"]), modelPath },
      );
    },
  },
  {
    file: "04_items.jsonl",
    entity: (row) => {
      const itemId = valueText(row["物品ID"]);
      const name = valueText(row["物品名称"]);
      if (!meaningfulText(name) || !itemId) return null;
      const itemType = valueText(row["物品类型"]);
      const description = valueText(row["物品介绍"]);
      return rawEntity(
        `game-item:${itemId}`,
        "item",
        "game-item",
        name,
        `${itemType || "物品"}；稀有度 ${valueText(row["稀有度"]) || "未知"}${meaningfulText(description) ? `；${description}` : ""}`,
        [itemId, itemType],
        { itemId, itemType, rarity: valueText(row["稀有度"]), description },
      );
    },
  },
  {
    file: "05_decorations.jsonl",
    entity: (row) => {
      const decorationId = valueText(row["装饰珠ID"]);
      const name = valueText(row["名称"]);
      if (!meaningfulText(name) || !decorationId) return null;
      const itemId = valueText(row["物品ID"]);
      const slot = valueText(row["孔位"]);
      return rawEntity(
        `game-decoration:${decorationId}`,
        "decoration",
        "game-equipment",
        name,
        `装饰珠；孔位 ${slot || "未知"}`,
        [decorationId, itemId, slot],
        { decorationId, itemId, slot },
      );
    },
  },
  {
    file: "06_monsters.jsonl",
    entity: (row) => {
      const monsterId = valueText(row["怪物ID"]);
      const name = valueText(row["怪物名称"]);
      if (!meaningfulText(name) || !monsterId) return null;
      const code = valueText(row["怪物代码"]);
      return rawEntity(`game-monster:${monsterId}`, "monster", "game-monster", name, `怪物代码 ${code || "未知"}`, [monsterId, code], { monsterId, code });
    },
  },
  {
    file: "07_special_equipment.jsonl",
    entity: (row) => {
      const equipmentId = valueText(row["装备ID"]);
      const name = valueText(row["装备名称"]);
      if (!meaningfulText(name) || !equipmentId) return null;
      const description = valueText(row["装备介绍"]);
      return rawEntity(`game-special-equipment:${equipmentId}`, "specialEquipment", "game-equipment", name, meaningfulText(description) ? description : "特殊装备", [equipmentId, valueText(row["装备代码"]), valueText(row["强化装备名称"])], { equipmentId, code: valueText(row["装备代码"]), upgradedName: valueText(row["强化装备名称"]), description });
    },
  },
  {
    file: "08_skills.jsonl",
    entity: (row) => {
      const skillId = valueText(row["技能ID"]);
      const name = valueText(row["技能名称"]);
      if (!meaningfulText(name) || !skillId) return null;
      const description = valueText(row["技能介绍"]);
      return rawEntity(`game-skill:${skillId}`, "skill", "game-skill", name, meaningfulText(description) ? description : "技能说明缺失", [skillId, valueText(row["技能代码"])], { skillId, code: valueText(row["技能代码"]), description });
    },
  },
  {
    file: "09_palico_weapons.jsonl",
    entity: (row) => {
      const id = valueText(row["武器ID"]);
      const name = valueText(row["武器名称"]);
      if (!meaningfulText(name) || !id) return null;
      const modelPath = valueText(row["模型地址"]);
      return rawEntity(`game-palico-weapon:${id}`, "palicoWeapon", "game-assets", name, `随从武器；模型 ${modelPath || "未知"}`, [id, modelPath], { weaponId: id, modelPath });
    },
  },
  {
    file: "10_palico_armor.jsonl",
    entity: (row) => {
      const partId = valueText(row["部位ID"]);
      const id = valueText(row["防具ID"]);
      const name = valueText(row["防具名称"]);
      if (!meaningfulText(name) || !partId || !id) return null;
      const part = valueText(row["部位名称"]);
      const modelPath = valueText(row["模型地址"]);
      return rawEntity(`game-palico-armor:${partId}:${id}`, "palicoArmor", "game-assets", name, `随从${part || "防具"}；模型 ${modelPath || "未知"}`, [partId, id, part, modelPath], { partId, part, armorId: id, modelPath });
    },
  },
  {
    file: "11_armor_series.jsonl",
    entity: (row) => {
      const id = valueText(row["防具系列ID / 幻化ID"]);
      const name = valueText(row["防具系列名称"]);
      if (!meaningfulText(name) || !id) return null;
      const modelPath = valueText(row["模型地址"]);
      return rawEntity(`game-armor-series:${id}`, "armorSeries", "game-equipment", name, `防具系列；模型 ${modelPath || "未知"}`, [id, modelPath], { seriesOrLayeredId: id, modelPath });
    },
  },
  {
    file: "12_pendants.jsonl",
    entity: (row) => {
      const id = valueText(row["吊坠ID"]);
      const name = valueText(row["吊坠名称"]);
      if (!meaningfulText(name) || !id) return null;
      const modelPath = valueText(row["模型地址"]);
      return rawEntity(`game-pendant:${id}`, "pendant", "game-assets", name, `武器挂件；模型 ${modelPath || "未知"}`, [id, valueText(row["吊坠代码"]), modelPath], { pendantId: id, code: valueText(row["吊坠代码"]), modelPath });
    },
  },
  {
    file: "13_kinsects.jsonl",
    entity: (row) => {
      const id = valueText(row["猎虫ID"]);
      const name = valueText(row["猎虫名称"]);
      if (!meaningfulText(name) || !id) return null;
      const modelPath = valueText(row["模型地址"]);
      return rawEntity(`game-kinsect:${id}`, "kinsect", "game-assets", name, `猎虫；模型 ${modelPath || "未知"}`, [id, valueText(row["猎虫代码"]), modelPath], { kinsectId: id, code: valueText(row["猎虫代码"]), modelPath });
    },
  },
  {
    file: "14_quests.jsonl",
    entity: (row) => {
      const id = valueText(row["任务ID"]);
      const name = valueText(row["任务名称"]);
      if (!meaningfulText(name) || !id) return null;
      const objective = valueText(row["任务目标"]);
      const failure = valueText(row["任务失败条件"]);
      return rawEntity(`game-quest:${id}`, "quest", "game-quest", name, `任务目标：${meaningfulText(objective) ? objective : "未收录"}${meaningfulText(failure) ? `；失败条件：${failure}` : ""}`, [id, objective], { questId: id, objective, failure });
    },
  },
  {
    file: "15_deliveries.jsonl",
    entity: (row) => {
      const id = valueText(row["交货委托ID"]);
      const name = valueText(row["交货委托名称"]);
      if (!meaningfulText(name) || !id) return null;
      const reward = valueText(row["交货委托回报"]);
      return rawEntity(`game-delivery:${id}`, "delivery", "game-quest", name, meaningfulText(reward) ? `交货委托回报：${reward}` : "交货委托", [id, reward], { deliveryId: id, reward });
    },
  },
  {
    file: "16_canteen_skills.jsonl",
    entity: (row) => {
      const code = valueText(row["猫饭代码"]);
      const name = valueText(row["猫饭名称"]);
      if (!meaningfulText(name) || !code) return null;
      const effect = valueText(row["猫饭效果"]);
      return rawEntity(`game-canteen-skill:${code}`, "canteenSkill", "game-canteen", name, meaningfulText(effect) ? effect : "猫饭技能", [code], { code, effect });
    },
  },
  {
    file: "17_npc.jsonl",
    entity: (row) => {
      const code = valueText(row["NPC代码"]);
      const name = valueText(row["NPC名称"]);
      if (!meaningfulText(name) || !code) return null;
      const modelPath = valueText(row["模型地址"]);
      return rawEntity(`game-npc:${code}`, "npc", "game-npc", name, `NPC；模型 ${modelPath || "未知"}`, [code, modelPath], { code, modelPath });
    },
  },
  {
    file: "18_poogie.jsonl",
    entity: (row) => {
      const id = valueText(row["小猪服装ID"]);
      const name = valueText(row["小猪服装名"]);
      if (!meaningfulText(name) || !id) return null;
      return rawEntity(`game-poogie:${id}`, "poogieCostume", "game-assets", name, "噗吱猪服装", [id], { costumeId: id });
    },
  },
  {
    file: "19_stages.jsonl",
    entity: (row) => {
      const id = valueText(row["场景ID"]);
      const name = valueText(row["场景名称"]);
      if (!meaningfulText(name) || !id) return null;
      return rawEntity(`game-stage:${id}`, "stage", "game-stage", name, "地图或场景", [id], { stageId: id });
    },
  },
  {
    file: "20_achievements.jsonl",
    entity: (row) => {
      const id = valueText(row["成就ID"]);
      const name = valueText(row["成就名称"]);
      if (!meaningfulText(name) || !id) return null;
      const requirement = valueText(row["成就要求"]);
      return rawEntity(`game-achievement:${id}`, "achievement", "game-achievement", name, meaningfulText(requirement) ? requirement : "成就要求未收录", [id], { achievementId: id, requirement });
    },
  },
  {
    file: "21_melodies.jsonl",
    entity: (row) => {
      const code = valueText(row["音乐代码"]);
      const skill = valueText(row["音乐技能"]);
      if (!meaningfulText(skill) || !code) return null;
      return rawEntity(`game-melody:${code}`, "huntingHornMelody", "game-equipment", skill, `狩猎笛旋律；等级 ${valueText(row["音乐等级"]) || "未知"}`, [code, valueText(row["音乐等级"])], { code, level: valueText(row["音乐等级"]), effect: skill });
    },
  },
  {
    file: "22_endemic_life.jsonl",
    entity: (row) => {
      const id = valueText(row["生物ID"]);
      const subId = valueText(row["副ID"]);
      const name = valueText(row["名称"]);
      if (!meaningfulText(name) || !id || !subId) return null;
      const itemId = valueText(row["物品ID"]);
      return rawEntity(`game-endemic-life:${id}:${subId}`, "endemicLife", "game-wildlife", name, "环境生物", [id, subId, itemId], { endemicLifeId: id, subId, itemId });
    },
  },
  {
    file: "23_gallery.jsonl",
    entity: (row) => {
      const id = valueText(row["回放ID"]);
      const name = valueText(row["回放名称"]);
      if (!meaningfulText(name) || !id) return null;
      const description = valueText(row["回放描述"]);
      return rawEntity(`game-gallery:${id}`, "gallery", "game-lore", name, meaningfulText(description) ? description : "画廊内容", [id], { galleryId: id, description });
    },
  },
  {
    file: "24_login_bonus.jsonl",
    entity: (row) => {
      const id = valueText(row["奖金任务ID"]);
      const name = valueText(row["奖金任务名称"]);
      if (!meaningfulText(name) || !id) return null;
      const objective = valueText(row["奖金任务目标"]);
      return rawEntity(`game-login-bonus:${id}`, "loginBonus", "game-quest", name, meaningfulText(objective) ? objective : "登录奖金任务", [id], { bonusId: id, objective });
    },
  },
  {
    file: "25_ingredients.jsonl",
    entity: (row) => {
      const code = valueText(row["食材代码"]);
      const name = valueText(row["食材名称"]);
      if (!meaningfulText(name) || !code) return null;
      const description = valueText(row["食材介绍"]);
      return rawEntity(`game-ingredient:${code}`, "ingredient", "game-canteen", name, meaningfulText(description) ? description : "猫饭食材", [code], { code, description });
    },
  },
];

async function readJsonLines(path) {
  const content = await readFile(path, "utf8");
  return content
    .split(/\r?\n/u)
    .filter((line) => line.trim())
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`无法解析 ${path} 第 ${index + 1} 行：${error.message}`);
      }
    });
}

async function loadMhwDbSnapshot() {
  try {
    const snapshot = JSON.parse(await readFile(mhwDbSnapshotPath, "utf8"));
    if (
      snapshot.schemaVersion !== 1
      || snapshot.sourceId !== "mhw-db-live-snapshot"
      || snapshot.gameVersion !== "unverified"
      || !Array.isArray(snapshot.resources?.skills)
      || !Array.isArray(snapshot.resources?.items)
      || !Array.isArray(snapshot.resources?.armor)
      || typeof snapshot.armorIdMapMarkdown !== "string"
    ) {
      throw new Error("快照结构或版本状态无效");
    }
    return snapshot;
  } catch (error) {
    if (error?.code === "ENOENT") {
      return null;
    }
    throw new Error(`无法读取 MHW DB 开发快照：${error.message}`);
  }
}

async function loadMhworldDataArmorNameMap() {
  try {
    const snapshot = JSON.parse(await readFile(mhworldDataArmorMapPath, "utf8"));
    if (
      snapshot.schemaVersion !== 1
      || snapshot.sourceId !== "mhworlddata-armor-name-map"
      || snapshot.contentBaselineVersion !== contentBaselineVersion
      || !snapshot.tables
      || ![
        "weaponBase", "weaponTranslations", "weaponCrafting",
        "armorBase", "armorTranslations", "armorSkills", "armorCrafting",
        "decorationBase", "decorationTranslations",
        "charmBase", "charmTranslations", "charmCrafting",
        "monsterBase", "monsterTranslations", "monsterWeaknesses", "monsterHitzones", "monsterRewards",
        "questBase", "questTranslations", "questMonsters", "questRewards",
        "locationBase", "locationCamps", "locationItems",
        "skillTranslations", "itemTranslations",
      ]
        .every((name) => Array.isArray(snapshot.tables[name]?.rows))
    ) {
      throw new Error("快照结构或内容基线无效");
    }
    for (const row of snapshot.tables.armorTranslations.rows) {
      if (!meaningfulText(row.name_en) || !meaningfulText(row.name_zh)) {
        throw new Error("防具名称映射缺少英文或繁中名称");
      }
    }
    return snapshot;
  } catch (error) {
    if (error?.code === "ENOENT") {
      return null;
    }
    throw new Error(`无法读取 MHWData 防具名称映射：${error.message}`);
  }
}

async function loadGame8QuestUnlockSnapshot() {
  try {
    const snapshot = JSON.parse(await readFile(game8QuestUnlockSnapshotPath, "utf8"));
    if (
      snapshot.schemaVersion !== 1
      || snapshot.sourceKind !== "communityQuestUnlockGuide"
      || !Array.isArray(snapshot.pages)
      || !Array.isArray(snapshot.entries)
      || !snapshot.entries.every((entry) => (
        meaningfulText(entry.questNameEn)
        && meaningfulText(entry.sourceId)
        && meaningfulText(entry.sourceUrl)
        && Array.isArray(entry.requirements)
      ))
    ) {
      throw new Error("任务解锁快照结构无效");
    }
    return snapshot;
  } catch (error) {
    // 任务链是可选补充来源；没有抓取快照时，基础游戏事实包仍应能构建。
    if (error?.code === "ENOENT") return null;
    throw new Error(`无法读取 Game8 任务解锁快照：${error.message}`);
  }
}

function mapByUniqueKey(rows, key, label) {
  const result = new Map();
  for (const row of rows) {
    const value = valueText(row[key]);
    if (!value) {
      continue;
    }
    if (result.has(value)) {
      throw new Error(`${label}存在重复键：${value}`);
    }
    result.set(value, row);
  }
  return result;
}

function groupByCompositeKey(rows, keys) {
  const result = new Map();
  for (const row of rows) {
    const values = keys.map((key) => valueText(row[key]));
    if (values.some((value) => !value)) continue;
    const compositeKey = values.join("\u001f");
    const matches = result.get(compositeKey) ?? [];
    matches.push(row);
    result.set(compositeKey, matches);
  }
  return result;
}

function translatedLocalName(translations, englishName, traditionalToSimplifiedNames) {
  const nameZhHant = valueText(translations.get(englishName)?.name_zh);
  const nameZhHans = nameZhHant ? traditionalToSimplifiedNames.get(nameZhHant) : null;
  return nameZhHans ? { nameZhHant, nameZhHans } : null;
}

async function loadTraditionalToSimplifiedNames() {
  const gameTextPath = path.join(projectRoot, "references/mhwi-data/curated/game-text-zh-hant.json");
  let gameText;
  try {
    gameText = JSON.parse(await readFile(fullGameTextBridgePath, "utf8"));
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw new Error(`无法读取完整简繁游戏文本桥：${error.message}`);
    }
    gameText = JSON.parse(await readFile(gameTextPath, "utf8"));
  }
  if (gameText.schemaVersion !== 1 || !gameText.names || typeof gameText.names !== "object") {
    throw new Error("官方简繁游戏文本映射结构无效。");
  }
  const names = new Map();
  const ambiguous = new Set();
  for (const [nameZhHans, nameZhHant] of Object.entries(gameText.names)) {
    if (!meaningfulText(nameZhHans) || !meaningfulText(nameZhHant)) {
      continue;
    }
    const existing = names.get(nameZhHant);
    if (existing && existing !== nameZhHans) {
      ambiguous.add(nameZhHant);
      continue;
    }
    names.set(nameZhHant, nameZhHans);
  }
  for (const nameZhHant of ambiguous) {
    names.delete(nameZhHant);
  }
  return names;
}

async function loadSimplifiedToEnglishNames() {
  try {
    const gameText = JSON.parse(await readFile(fullGameTextBridgePath, "utf8"));
    if (gameText.schemaVersion !== 1 || !gameText.englishNames || typeof gameText.englishNames !== "object") {
      return new Map();
    }
    return new Map(Object.entries(gameText.englishNames).filter(([nameZhHans, nameEn]) => (
      meaningfulText(nameZhHans) && meaningfulText(nameEn)
    )));
  } catch (error) {
    if (error?.code === "ENOENT") return new Map();
    throw new Error(`无法读取完整简英游戏文本桥：${error.message}`);
  }
}

async function loadCuratedLocationNameMap() {
  let source;
  try {
    source = JSON.parse(await readFile(locationNameMapPath, "utf8"));
  } catch (error) {
    throw new Error(`无法读取地图稳定映射表：${error.message}`);
  }
  if (source.schemaVersion !== 1 || !Array.isArray(source.entries)) {
    throw new Error("地图稳定映射表结构无效。");
  }
  const map = new Map();
  for (const entry of source.entries) {
    const englishName = valueText(entry.englishName);
    const locationId = normalizedNumericId(entry.locationId);
    const stageId = valueText(entry.stageId);
    const nameZhHans = valueText(entry.nameZhHans);
    if (!englishName || !locationId || !stageId || !nameZhHans) {
      throw new Error("地图稳定映射表存在缺少英文名、地点 ID、场景 ID 或简中名称的条目。");
    }
    if (map.has(englishName)) {
      throw new Error(`地图稳定映射表存在重复英文名：${englishName}`);
    }
    map.set(englishName, {
      locationId,
      stageId,
      nameZhHans,
      nameZhHant: valueText(entry.nameZhHant) || null,
    });
  }
  return map;
}

async function loadCuratedQuestNameMap() {
  const source = JSON.parse(await readFile(curatedQuestNameMapPath, "utf8"));
  if (source.schemaVersion !== 1 || source.sourceId !== "mhw-curated-special-quest-name-map" || !Array.isArray(source.entries)) {
    throw new Error("特别任务名称映射表结构无效");
  }
  const result = new Map();
  for (const entry of source.entries) {
    const englishName = normalizedExternalEnglishName(entry.englishName);
    const questId = valueText(entry.questId);
    if (!englishName || !/^\d{5}$/u.test(questId) || !meaningfulText(entry.nameZhHans)) {
      throw new Error("特别任务名称映射表存在无效条目");
    }
    if (result.has(englishName)) {
      throw new Error(`特别任务名称映射表存在重复英文名：${entry.englishName}`);
    }
    result.set(englishName, { questId, nameZhHans: entry.nameZhHans });
  }
  return result;
}

function addMhwDbSkillFacts(entities, relations, snapshot) {
  if (!snapshot) {
    return 0;
  }
  const baseSkills = new Map(
    entities
      .filter((entity) => entity.kind === "skill" && entity.id.startsWith("game-skill:"))
      .map((entity) => [entity.data.skillId, entity]),
  );
  let imported = 0;
  for (const sourceSkill of snapshot.resources.skills) {
    const skillId = String(sourceSkill.id ?? "");
    const baseSkill = baseSkills.get(skillId);
    if (!baseSkill || !meaningfulText(sourceSkill.name) || !Array.isArray(sourceSkill.ranks)) {
      continue;
    }
    const id = `game-skill-fact:${skillId}`;
    const ranks = sourceSkill.ranks
      .filter((rank) => Number.isInteger(rank.level) && meaningfulText(rank.description))
      .map((rank) => ({
        level: rank.level,
        descriptionEn: rank.description,
        modifiers: rank.modifiers ?? {},
      }));
    if (ranks.length === 0) {
      continue;
    }
    entities.push({
      id,
      kind: "skillFact",
      domain: "game-skill",
      title: `${baseSkill.title}（等级资料）`,
      nameZhHans: `${baseSkill.title}（等级资料）`,
      aliases: rawAliasValues(baseSkill.title, sourceSkill.name),
      summary: `${baseSkill.title} 的等级效果资料。来源未提供可机器核验的游戏版本，回答时必须标明该限制。`,
      data: {
        skillEntityId: baseSkill.id,
        englishName: sourceSkill.name,
        descriptionEn: sourceSkill.description ?? "",
        ranks,
        retrievedAt: snapshot.retrievedAt,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.75,
      sourceId: "mhw-db-live-snapshot",
    });
    relations.push({
      id: `relation:hasRankFacts:${baseSkill.id}:${id}`,
      subjectId: baseSkill.id,
      predicate: "hasRankFacts",
      objectId: id,
      gameVersion: "unverified",
      confidence: 0.75,
      sourceId: "mhw-db-live-snapshot",
      data: { retrievedAt: snapshot.retrievedAt },
    });
    imported += 1;
  }
  return imported;
}

function addMhworldDataArmorFacts(entities, relations, snapshot, traditionalToSimplifiedNames) {
  if (!snapshot) {
    return 0;
  }
  const partIds = { head: "0", chest: "1", arms: "2", waist: "3", legs: "4" };
  const baseArmors = new Map();
  const items = new Map(
    entities
      .filter((entity) => entity.kind === "item" && entity.id.startsWith("game-item:"))
      .map((entity) => [entity.data.itemId, entity]),
  );
  const skills = new Map(
    entities
      .filter((entity) => entity.kind === "skill" && entity.id.startsWith("game-skill:"))
      .map((entity) => [entity.data.skillId, entity]),
  );
  for (const entity of entities.filter((entity) => entity.kind === "armor" && entity.id.startsWith("game-armor:"))) {
    const key = `${entity.data.partId}:${entity.title}`;
    const matches = baseArmors.get(key) ?? [];
    matches.push(entity);
    baseArmors.set(key, matches);
  }
  const armorTranslations = mapByUniqueKey(snapshot.tables.armorTranslations.rows, "name_en", "MHWData 防具名称表");
  const skillTranslations = mapByUniqueKey(snapshot.tables.skillTranslations.rows, "name_en", "MHWData 技能名称表");
  const itemTranslations = mapByUniqueKey(snapshot.tables.itemTranslations.rows, "name_en", "MHWData 物品名称表");
  const armorSkills = mapByUniqueKey(snapshot.tables.armorSkills.rows, "base_name_en", "MHWData 防具技能表");
  const armorCrafting = mapByUniqueKey(snapshot.tables.armorCrafting.rows, "base_name_en", "MHWData 防具制作表");
  let imported = 0;
  for (const sourceArmor of snapshot.tables.armorBase.rows) {
    const partId = partIds[sourceArmor.type];
    const translatedName = translatedLocalName(armorTranslations, sourceArmor.name_en, traditionalToSimplifiedNames);
    if (!partId || !translatedName) {
      continue;
    }
    const candidates = baseArmors.get(`${partId}:${translatedName.nameZhHans}`) ?? [];
    // 名称同时经过官方繁中和本地简中游戏文本键验证；仍拒绝任何重复实体。
    if (candidates.length !== 1) {
      continue;
    }
    const baseArmor = candidates[0];
    const id = `game-armor-fact:mhwdata:${sourceArmor.id}`;
    const sourceSkills = armorSkills.get(sourceArmor.name_en);
    const skillEntries = [1, 2].flatMap((index) => {
      const nameEn = valueText(sourceSkills?.[`skill${index}_name`]);
      const level = valueText(sourceSkills?.[`skill${index}_level`]);
      if (!nameEn || !level) return [];
      const translatedSkill = translatedLocalName(skillTranslations, nameEn, traditionalToSimplifiedNames);
      const knownSkill = translatedSkill
        ? [...skills.values()].find((skill) => skill.title === translatedSkill.nameZhHans)
        : null;
      return [{
        skillEntityId: knownSkill?.id ?? null,
        nameZhHans: knownSkill?.title ?? translatedSkill?.nameZhHans ?? null,
        nameZhHant: translatedSkill?.nameZhHant ?? null,
        nameEn,
        level: Number(level),
      }];
    });
    const sourceCrafting = armorCrafting.get(sourceArmor.name_en);
    const materialEntries = [1, 2, 3, 4].flatMap((index) => {
      const nameEn = valueText(sourceCrafting?.[`item${index}_name`]);
      const quantity = valueText(sourceCrafting?.[`item${index}_qty`]);
      if (!nameEn || !quantity) return [];
      const translatedItem = translatedLocalName(itemTranslations, nameEn, traditionalToSimplifiedNames);
      const knownItem = translatedItem
        ? [...items.values()].find((item) => item.title === translatedItem.nameZhHans)
        : null;
      return [{
        itemEntityId: knownItem?.id ?? null,
        nameZhHans: knownItem?.title ?? translatedItem?.nameZhHans ?? null,
        nameZhHant: translatedItem?.nameZhHant ?? null,
        nameEn,
        quantity: Number(quantity),
      }];
    });
    entities.push({
      id,
      kind: "armorFact",
      domain: "game-equipment",
      title: `${baseArmor.title}（属性资料）`,
      nameZhHans: `${baseArmor.title}（属性资料）`,
      aliases: rawAliasValues(baseArmor.title, translatedName.nameZhHant, sourceArmor.name_en),
      summary: `${baseArmor.title} 的防御、耐性、技能和制作资料。名称经过官方简繁游戏文本和本地实体双重校验；数值来源版本尚未机器核验。`,
      data: {
        armorEntityId: baseArmor.id,
        englishName: sourceArmor.name_en,
        nameZhHant: translatedName.nameZhHant,
        localNameZhHans: translatedName.nameZhHans,
        gender: valueText(sourceArmor.gender),
        rarity: Number(valueText(sourceArmor.rarity)),
        defense: {
          base: Number(valueText(sourceArmor.defense_base)),
          max: Number(valueText(sourceArmor.defense_max)),
          augmentMax: Number(valueText(sourceArmor.defense_augment_max)),
        },
        resistances: {
          fire: Number(valueText(sourceArmor.defense_fire)),
          water: Number(valueText(sourceArmor.defense_water)),
          thunder: Number(valueText(sourceArmor.defense_thunder)),
          ice: Number(valueText(sourceArmor.defense_ice)),
          dragon: Number(valueText(sourceArmor.defense_dragon)),
        },
        slots: ["slot_1", "slot_2", "slot_3"].map((field) => Number(valueText(sourceArmor[field]))).filter((slot) => slot > 0),
        skills: skillEntries,
        craftingMaterials: materialEntries,
        retrievedAt: snapshot.retrievedAt,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
    });
    relations.push({
      id: `relation:hasArmorFacts:${baseArmor.id}:${id}`,
      subjectId: baseArmor.id,
      predicate: "hasArmorFacts",
      objectId: id,
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
      data: {
        nameZhHant: translatedName.nameZhHant,
        retrievedAt: snapshot.retrievedAt,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
      },
    });
    for (const skill of skillEntries.filter((entry) => entry.skillEntityId)) {
      relations.push({
        id: `relation:grantsSkill:${id}:${skill.skillEntityId}`,
        subjectId: id,
        predicate: "grantsSkill",
        objectId: skill.skillEntityId,
        gameVersion: "unverified",
        confidence: 0.78,
        sourceId: "mhworlddata-armor-name-map",
        data: { level: skill.level, contentBaselineVersion },
      });
    }
    for (const [materialIndex, material] of materialEntries.filter((entry) => entry.itemEntityId).entries()) {
      relations.push({
        id: `relation:requiresMaterial:${id}:${material.itemEntityId}:${materialIndex}`,
        subjectId: id,
        predicate: "requiresMaterial",
        objectId: material.itemEntityId,
        gameVersion: "unverified",
        confidence: 0.78,
        sourceId: "mhworlddata-armor-name-map",
        data: { quantity: material.quantity, contentBaselineVersion },
      });
    }
    imported += 1;
  }
  return imported;
}

function entitiesByTitle(entities, kind) {
  const result = new Map();
  for (const entity of entities.filter((entry) => entry.kind === kind && entry.id.startsWith(`game-${kind}:`))) {
    const matches = result.get(entity.title) ?? [];
    matches.push(entity);
    result.set(entity.title, matches);
  }
  return result;
}

function uniqueTranslatedEntity(translations, englishName, traditionalToSimplifiedNames, entities) {
  const translated = translatedLocalName(translations, englishName, traditionalToSimplifiedNames);
  if (!translated) return null;
  const candidates = entities.get(translated.nameZhHans) ?? [];
  return candidates.length === 1 ? { translated, entity: candidates[0] } : null;
}

const weaponTypeNames = {
  "great-sword": "大剑",
  "long-sword": "太刀",
  "sword-and-shield": "片手剑",
  "dual-blades": "双剑",
  hammer: "大锤",
  "hunting-horn": "狩猎笛",
  lance: "长枪",
  gunlance: "铳枪",
  "switch-axe": "斩斧",
  "charge-blade": "盾斧",
  "insect-glaive": "操虫棍",
  "light-bowgun": "轻弩炮",
  "heavy-bowgun": "重弩炮",
  bow: "弓",
};

function uniqueTranslatedWeaponEntity(translations, englishName, weaponType, traditionalToSimplifiedNames, weapons) {
  const translated = translatedLocalName(translations, englishName, traditionalToSimplifiedNames);
  const localWeaponType = weaponTypeNames[weaponType];
  if (!translated || !localWeaponType) return null;
  const candidates = weapons.get(`${localWeaponType}\u001f${translated.nameZhHans}`) ?? [];
  return candidates.length === 1 ? { translated, entity: candidates[0] } : null;
}

function translatedMaterialEntries(source, itemTranslations, traditionalToSimplifiedNames, items) {
  const sources = Array.isArray(source) ? source : [source];
  return sources.flatMap((entry) => [1, 2, 3, 4].flatMap((index) => {
    const nameEn = valueText(entry?.[`item${index}_name`]);
    const quantity = valueText(entry?.[`item${index}_qty`]);
    if (!nameEn || !quantity) return [];
    const match = uniqueTranslatedEntity(itemTranslations, nameEn, traditionalToSimplifiedNames, items);
    return [{
      itemEntityId: match?.entity.id ?? null,
      nameZhHans: match?.entity.title ?? match?.translated.nameZhHans ?? null,
      nameZhHant: match?.translated.nameZhHant ?? null,
      nameEn,
      quantity: Number(quantity),
      acquisitionType: valueText(entry?.type),
    }];
  }));
}

function addMhworldDataWeaponFacts(entities, relations, snapshot, traditionalToSimplifiedNames) {
  if (!snapshot) return 0;
  const weapons = new Map();
  for (const entity of entities.filter((entry) => entry.kind === "weapon" && entry.id.startsWith("game-weapon:"))) {
    const key = `${entity.data.weaponType}\u001f${entity.title}`;
    const matches = weapons.get(key) ?? [];
    matches.push(entity);
    weapons.set(key, matches);
  }
  const items = entitiesByTitle(entities, "item");
  const weaponTranslations = mapByUniqueKey(snapshot.tables.weaponTranslations.rows, "name_en", "MHWData 武器名称表");
  const itemTranslations = mapByUniqueKey(snapshot.tables.itemTranslations.rows, "name_en", "MHWData 物品名称表");
  const crafting = groupByCompositeKey(
    snapshot.tables.weaponCrafting.rows,
    ["base_name_en", "weapon_type"],
  );
  let imported = 0;
  for (const sourceWeapon of snapshot.tables.weaponBase.rows) {
    const match = uniqueTranslatedWeaponEntity(
      weaponTranslations,
      sourceWeapon.name_en,
      sourceWeapon.weapon_type,
      traditionalToSimplifiedNames,
      weapons,
    );
    if (!match) continue;
    const id = `game-weapon-fact:mhwdata:${sourceWeapon.id}`;
    const materials = translatedMaterialEntries(
      crafting.get([sourceWeapon.name_en, sourceWeapon.weapon_type].join("\u001f")),
      itemTranslations,
      traditionalToSimplifiedNames,
      items,
    );
    const previous = valueText(sourceWeapon.previous_en)
      ? uniqueTranslatedWeaponEntity(
        weaponTranslations,
        sourceWeapon.previous_en,
        sourceWeapon.weapon_type,
        traditionalToSimplifiedNames,
        weapons,
      )
      : null;
    entities.push({
      id,
      kind: "weaponFact",
      domain: "game-equipment",
      title: `${match.entity.title}（属性资料）`,
      nameZhHans: `${match.entity.title}（属性资料）`,
      aliases: rawAliasValues(match.entity.title, match.translated.nameZhHant, sourceWeapon.name_en),
      summary: `${match.entity.title} 的攻击、会心、属性、孔位和制作资料。名称经过官方简繁游戏文本和本地实体双重校验；数值来源版本尚未机器核验。`,
      data: {
        weaponEntityId: match.entity.id,
        englishName: sourceWeapon.name_en,
        nameZhHant: match.translated.nameZhHant,
        weaponType: valueText(sourceWeapon.weapon_type),
        rarity: Number(valueText(sourceWeapon.rarity)),
        attack: Number(valueText(sourceWeapon.attack)),
        affinity: Number(valueText(sourceWeapon.affinity)),
        defense: Number(valueText(sourceWeapon.defense)),
        element: {
          hidden: valueText(sourceWeapon.element_hidden) === "TRUE",
          primary: valueText(sourceWeapon.element1),
          primaryAttack: Number(valueText(sourceWeapon.element1_attack)),
          secondary: valueText(sourceWeapon.element2),
          secondaryAttack: Number(valueText(sourceWeapon.element2_attack)),
        },
        elderseal: valueText(sourceWeapon.elderseal),
        slots: ["slot_1", "slot_2", "slot_3"].map((field) => Number(valueText(sourceWeapon[field]))).filter((slot) => slot > 0),
        phial: valueText(sourceWeapon.phial),
        phialPower: valueText(sourceWeapon.phial_power),
        shelling: valueText(sourceWeapon.shelling),
        shellingLevel: valueText(sourceWeapon.shelling_level),
        kinsectBonus: valueText(sourceWeapon.kinsect_bonus),
        previousWeaponEntityId: previous?.entity.id ?? null,
        craftingMaterials: materials,
        retrievedAt: snapshot.retrievedAt,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
    });
    relations.push({
      id: `relation:hasWeaponFacts:${match.entity.id}:${id}`,
      subjectId: match.entity.id,
      predicate: "hasWeaponFacts",
      objectId: id,
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
      data: { contentBaselineVersion, sourceCommit: snapshot.sourceCommit },
    });
    if (previous) {
      relations.push({
        id: `relation:upgradesFrom:${id}:${previous.entity.id}`,
        subjectId: id,
        predicate: "upgradesFrom",
        objectId: previous.entity.id,
        gameVersion: "unverified",
        confidence: 0.78,
        sourceId: "mhworlddata-armor-name-map",
        data: { contentBaselineVersion },
      });
    }
    for (const [materialIndex, material] of materials.filter((entry) => entry.itemEntityId).entries()) {
      relations.push({
        id: `relation:requiresMaterial:${id}:${material.itemEntityId}:${materialIndex}`,
        subjectId: id,
        predicate: "requiresMaterial",
        objectId: material.itemEntityId,
        gameVersion: "unverified",
        confidence: 0.78,
        sourceId: "mhworlddata-armor-name-map",
        data: { quantity: material.quantity, acquisitionType: material.acquisitionType, contentBaselineVersion },
      });
    }
    imported += 1;
  }
  return imported;
}

function addMhworldDataDecorationFacts(entities, relations, snapshot, traditionalToSimplifiedNames) {
  if (!snapshot) return 0;
  const decorations = entitiesByTitle(entities, "decoration");
  const skillEntities = entitiesByTitle(entities, "skill");
  const translations = mapByUniqueKey(snapshot.tables.decorationTranslations.rows, "name_en", "MHWData 装饰珠名称表");
  const skillTranslations = mapByUniqueKey(snapshot.tables.skillTranslations.rows, "name_en", "MHWData 技能名称表");
  let imported = 0;
  for (const sourceDecoration of snapshot.tables.decorationBase.rows) {
    const match = uniqueTranslatedEntity(translations, sourceDecoration.name_en, traditionalToSimplifiedNames, decorations);
    if (!match) continue;
    const id = `game-decoration-fact:mhwdata:${sourceDecoration.id}`;
    const skillEntries = [1, 2].flatMap((index) => {
      const nameEn = valueText(sourceDecoration[`skill${index}_name`]);
      const level = valueText(sourceDecoration[`skill${index}_level`]);
      if (!nameEn || !level) return [];
      const skill = uniqueTranslatedEntity(skillTranslations, nameEn, traditionalToSimplifiedNames, skillEntities);
      return [{ skillEntityId: skill?.entity.id ?? null, nameEn, level: Number(level) }];
    });
    entities.push({
      id,
      kind: "decorationFact",
      domain: "game-equipment",
      title: `${match.entity.title}（属性资料）`,
      nameZhHans: `${match.entity.title}（属性资料）`,
      aliases: rawAliasValues(match.entity.title, match.translated.nameZhHant, sourceDecoration.name_en),
      summary: `${match.entity.title} 的孔位、稀有度和技能资料。名称经过官方简繁游戏文本和本地实体双重校验；数值来源版本尚未机器核验。`,
      data: {
        decorationEntityId: match.entity.id,
        englishName: sourceDecoration.name_en,
        nameZhHant: match.translated.nameZhHant,
        slot: Number(valueText(sourceDecoration.slot)),
        rarity: Number(valueText(sourceDecoration.rarity)),
        skills: skillEntries,
        retrievedAt: snapshot.retrievedAt,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
    });
    relations.push({
      id: `relation:hasDecorationFacts:${match.entity.id}:${id}`,
      subjectId: match.entity.id,
      predicate: "hasDecorationFacts",
      objectId: id,
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
      data: { contentBaselineVersion, sourceCommit: snapshot.sourceCommit },
    });
    for (const skill of skillEntries.filter((entry) => entry.skillEntityId)) {
      relations.push({
        id: `relation:grantsSkill:${id}:${skill.skillEntityId}`,
        subjectId: id,
        predicate: "grantsSkill",
        objectId: skill.skillEntityId,
        gameVersion: "unverified",
        confidence: 0.78,
        sourceId: "mhworlddata-armor-name-map",
        data: { level: skill.level, contentBaselineVersion },
      });
    }
    imported += 1;
  }
  return imported;
}

function addMhworldDataCharms(entities, relations, snapshot, traditionalToSimplifiedNames) {
  if (!snapshot) return 0;
  const skills = entitiesByTitle(entities, "skill");
  const items = entitiesByTitle(entities, "item");
  const charmTranslations = mapByUniqueKey(snapshot.tables.charmTranslations.rows, "name_en", "MHWData 护石名称表");
  const skillTranslations = mapByUniqueKey(snapshot.tables.skillTranslations.rows, "name_en", "MHWData 技能名称表");
  const itemTranslations = mapByUniqueKey(snapshot.tables.itemTranslations.rows, "name_en", "MHWData 物品名称表");
  const crafting = mapByUniqueKey(snapshot.tables.charmCrafting.rows, "base_name_en", "MHWData 护石制作表");
  const charmByEnglishName = new Map();
  let imported = 0;
  for (const sourceCharm of snapshot.tables.charmBase.rows) {
    const translated = translatedLocalName(charmTranslations, sourceCharm.name_en, traditionalToSimplifiedNames);
    if (!translated) continue;
    const id = `game-charm:mhwdata:${sourceCharm.id}`;
    const skillEntries = [1, 2].flatMap((index) => {
      const nameEn = valueText(sourceCharm[`skill${index}_name`]);
      const level = valueText(sourceCharm[`skill${index}_level`]);
      if (!nameEn || !level) return [];
      const skill = uniqueTranslatedEntity(skillTranslations, nameEn, traditionalToSimplifiedNames, skills);
      return [{
        skillEntityId: skill?.entity.id ?? null,
        nameZhHans: skill?.entity.title ?? skill?.translated.nameZhHans ?? null,
        nameZhHant: skill?.translated.nameZhHant ?? null,
        nameEn,
        level: Number(level),
      }];
    });
    const materials = translatedMaterialEntries(
      crafting.get(sourceCharm.name_en),
      itemTranslations,
      traditionalToSimplifiedNames,
      items,
    );
    const entity = {
      id,
      kind: "charm",
      domain: "game-equipment",
      title: translated.nameZhHans,
      nameZhHans: translated.nameZhHans,
      aliases: rawAliasValues(translated.nameZhHans, translated.nameZhHant, sourceCharm.name_en),
      summary: `${translated.nameZhHans} 的稀有度、技能和制作资料。名称经过同一游戏文本键的简繁对照；数值来源版本尚未机器核验。`,
      data: {
        charmSourceId: valueText(sourceCharm.id),
        englishName: sourceCharm.name_en,
        nameZhHant: translated.nameZhHant,
        rarity: Number(valueText(sourceCharm.rarity)),
        skills: skillEntries,
        craftingMaterials: materials,
        previousCharmEntityId: null,
        retrievedAt: snapshot.retrievedAt,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.75,
      sourceId: "mhworlddata-armor-name-map",
    };
    const previous = valueText(sourceCharm.previous_en)
      ? charmByEnglishName.get(sourceCharm.previous_en)
      : null;
    if (previous) entity.data.previousCharmEntityId = previous.id;
    entities.push(entity);
    charmByEnglishName.set(sourceCharm.name_en, entity);
    if (previous) {
      relations.push({
        id: `relation:upgradesFrom:${entity.id}:${previous.id}`,
        subjectId: entity.id,
        predicate: "upgradesFrom",
        objectId: previous.id,
        gameVersion: "unverified",
        confidence: 0.75,
        sourceId: "mhworlddata-armor-name-map",
        data: { contentBaselineVersion },
      });
    }
    for (const skill of skillEntries.filter((entry) => entry.skillEntityId)) {
      relations.push({
        id: `relation:grantsSkill:${entity.id}:${skill.skillEntityId}`,
        subjectId: entity.id,
        predicate: "grantsSkill",
        objectId: skill.skillEntityId,
        gameVersion: "unverified",
        confidence: 0.75,
        sourceId: "mhworlddata-armor-name-map",
        data: { level: skill.level, contentBaselineVersion },
      });
    }
    for (const [materialIndex, material] of materials.filter((entry) => entry.itemEntityId).entries()) {
      relations.push({
        id: `relation:requiresMaterial:${entity.id}:${material.itemEntityId}:${materialIndex}`,
        subjectId: entity.id,
        predicate: "requiresMaterial",
        objectId: material.itemEntityId,
        gameVersion: "unverified",
        confidence: 0.75,
        sourceId: "mhworlddata-armor-name-map",
        data: { quantity: material.quantity, acquisitionType: material.acquisitionType, contentBaselineVersion },
      });
    }
    imported += 1;
  }
  return imported;
}

function addMhworldDataMonsterFacts(entities, relations, snapshot, traditionalToSimplifiedNames) {
  if (!snapshot) return 0;
  const monsters = entitiesByTitle(entities, "monster");
  const items = entitiesByTitle(entities, "item");
  const translations = mapByUniqueKey(snapshot.tables.monsterTranslations.rows, "name_en", "MHWData 怪物名称表");
  const itemTranslations = mapByUniqueKey(snapshot.tables.itemTranslations.rows, "name_en", "MHWData 物品名称表");
  const weaknesses = new Map();
  const hitzones = new Map();
  const rewards = new Map();
  for (const [store, rows] of [[weaknesses, snapshot.tables.monsterWeaknesses.rows], [hitzones, snapshot.tables.monsterHitzones.rows], [rewards, snapshot.tables.monsterRewards.rows]]) {
    for (const row of rows) {
      const key = valueText(row.name_en ?? row.base_name_en);
      if (!key) continue;
      const values = store.get(key) ?? [];
      values.push(row);
      store.set(key, values);
    }
  }
  let imported = 0;
  for (const sourceMonster of snapshot.tables.monsterBase.rows) {
    const match = uniqueTranslatedEntity(translations, sourceMonster.name_en, traditionalToSimplifiedNames, monsters);
    if (!match) continue;
    const id = `game-monster-fact:mhwdata:${sourceMonster.id}`;
    const rewardEntries = (rewards.get(sourceMonster.name_en) ?? []).flatMap((reward) => {
      const item = uniqueTranslatedEntity(itemTranslations, valueText(reward.item_en), traditionalToSimplifiedNames, items);
      return [{
        itemEntityId: item?.entity.id ?? null,
        nameEn: valueText(reward.item_en),
        conditionEn: valueText(reward.condition_en),
        rank: valueText(reward.rank),
        quantity: Number(valueText(reward.stack)),
        percentage: Number(valueText(reward.percentage)),
      }];
    });
    entities.push({
      id,
      kind: "monsterFact",
      domain: "game-monster",
      title: `${match.entity.title}（生态资料）`,
      nameZhHans: `${match.entity.title}（生态资料）`,
      aliases: rawAliasValues(match.entity.title, match.translated.nameZhHant, sourceMonster.name_en),
      summary: `${match.entity.title} 的肉质、弱点、陷阱和报酬资料。名称经过官方简繁游戏文本和本地实体双重校验；数值来源版本尚未机器核验。`,
      data: {
        monsterEntityId: match.entity.id,
        englishName: sourceMonster.name_en,
        nameZhHant: match.translated.nameZhHant,
        size: valueText(sourceMonster.size),
        traps: { pitfall: valueText(sourceMonster.pitfall_trap), shock: valueText(sourceMonster.shock_trap), vine: valueText(sourceMonster.vine_trap) },
        weaknesses: weaknesses.get(sourceMonster.name_en) ?? [],
        hitzones: hitzones.get(sourceMonster.name_en) ?? [],
        rewards: rewardEntries,
        retrievedAt: snapshot.retrievedAt,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
    });
    relations.push({
      id: `relation:hasMonsterFacts:${match.entity.id}:${id}`,
      subjectId: match.entity.id,
      predicate: "hasMonsterFacts",
      objectId: id,
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
      data: { contentBaselineVersion, sourceCommit: snapshot.sourceCommit },
    });
    for (const [rewardIndex, reward] of rewardEntries.filter((entry) => entry.itemEntityId).entries()) {
      relations.push({
        id: `relation:dropsItem:${id}:${reward.itemEntityId}:${reward.rank}:${reward.conditionEn}:${rewardIndex}`,
        subjectId: id,
        predicate: "dropsItem",
        objectId: reward.itemEntityId,
        gameVersion: "unverified",
        confidence: 0.78,
        sourceId: "mhworlddata-armor-name-map",
        data: { rank: reward.rank, conditionEn: reward.conditionEn, quantity: reward.quantity, percentage: reward.percentage, contentBaselineVersion },
      });
    }
    imported += 1;
  }
  return imported;
}

function normalizedNumericId(value) {
  const text = valueText(value);
  if (!/^\d+$/u.test(text)) return null;
  return String(Number(text));
}

function normalizedExternalEnglishName(value) {
  return valueText(value)
    .replace(/[‘’]/gu, "'")
    .replace(/[“”]/gu, '"')
    .replace(/\s+/gu, " ")
    .trim()
    .toLocaleLowerCase("en-US");
}

function addNameCandidate(index, name, entity) {
  const key = normalizedExternalEnglishName(name);
  if (!key) return;
  const candidates = index.get(key) ?? [];
  if (!candidates.some((candidate) => candidate.id === entity.id)) {
    candidates.push(entity);
    index.set(key, candidates);
  }
}

function uniqueNameCandidate(index, name) {
  const candidates = index.get(normalizedExternalEnglishName(name)) ?? [];
  return candidates.length === 1 ? candidates[0] : null;
}

function localizedQuestLocation(requirement, curatedLocations) {
  const englishName = valueText(requirement.locationNameEn);
  const curated = curatedLocations.get(englishName);
  if (curated) return curated.nameZhHans;
  return new Map([
    ["Astera", "星辰"],
    ["Seliana", "月辰"],
    ["Research Base", "研究基地"],
  ]).get(englishName) ?? null;
}

function unlockRequirementDisplay(requirement, monster, prerequisite, locationNameZhHans) {
  const rawMonsterName = valueText(requirement.monsterNameEn);
  const sourceCondition = rawMonsterName ? `来源页条件：${rawMonsterName}` : "来源页记录的条件需进一步核对";
  switch (requirement.kind) {
    case "completeQuest": return `完成任务：${prerequisite?.title ?? valueText(requirement.questNameEn)}`;
    case "requiresExpansion": return "需要拥有冰原扩展内容";
    case "huntMonster": return monster ? `狩猎：${monster.title}` : sourceCondition;
    case "captureMonster": return monster ? `捕获：${monster.title}` : sourceCondition;
    case "discoverMonster": return monster ? `发现：${monster.title}` : sourceCondition;
    case "talkToNpc": return `与 ${valueText(requirement.npcNameEn)} 对话`;
    case "unlockRank":
    case "reachRank": return `达到 ${valueText(requirement.rank)} ${valueText(requirement.level) || requirement.level || ""}`;
    case "discoverCamp": return locationNameZhHans ? `发现营地：${locationNameZhHans}` : `来源页地点条件：${valueText(requirement.locationNameEn)}`;
    case "discoverLocation": return locationNameZhHans ? `发现地点：${locationNameZhHans}` : `来源页地点条件：${valueText(requirement.locationNameEn)}`;
    case "researchLevel": {
      const level = valueText(requirement.level) || requirement.level || null;
      if (monster) return level ? `${monster.title}的研究等级达到 ${level}` : `提高${monster.title}的研究等级`;
      return sourceCondition;
    }
    case "completeBountyChain": return requirement.chainNameEn === "Cultural Exchange: Hoarfrost Reach"
      ? "完成文化交流：永霜冻土交货链"
      : "完成来源页指定的交货或调查链";
    case "completeOptionalRange": return "完成来源页指定的可选任务范围";
    case "completeStory": return requirement.storyNameEn
      ? valueText(requirement.storyNameEn) === "Iceborne" ? "完成冰原主线" : `完成剧情：${valueText(requirement.storyNameEn)}`
      : "完成本体主线";
    case "completeStoryOperation": return valueText(requirement.operationNameEn) === "Zorah Magdaros"
      ? "完成熔山龙作战"
      : `完成剧情作战：${valueText(requirement.operationNameEn)}`;
    case "eventAvailability": return "任务开放期间可承接";
    case "availableFromStart": return "游戏开始时即可承接";
    case "returnToLocation": return `返回：${locationNameZhHans ?? valueText(requirement.locationNameEn)}`;
    case "unlockGuidingLandsRegions": {
      const names = new Map([[
        "Volcanic", "熔岩地带",
      ], [
        "Tundra", "冰雪地带",
      ]]);
      return `解锁聚魔之地地区：${(requirement.regions ?? []).map((region) => names.get(region) ?? region).join("、")}`;
    }
    case "sourceText": return `来源页条件：${valueText(requirement.textEn)}`;
    default: return "来源页记录的解锁条件";
  }
}

function sourceQuestTargets(snapshot, sourceQuest) {
  const sourceQuestId = normalizedNumericId(sourceQuest.id);
  if (!sourceQuestId) return [];
  return snapshot.tables.questMonsters.rows
    .filter((row) => normalizedNumericId(row.base_id) === sourceQuestId && valueText(row.is_objective) === "TRUE")
    .map((row) => valueText(row.monster_en))
    .filter(Boolean);
}

function questForUnlockEntry(entry, questsByEnglishName, questDescriptors) {
  const direct = uniqueNameCandidate(questsByEnglishName, entry.questNameEn);
  if (direct) {
    const descriptor = questDescriptors.get(direct.id);
    if (!entry.questCategory || descriptor?.category === entry.questCategory) return direct;
  }
  // 同一怪物可能对应大量活动或挑战任务；只有斗技场编号与目标怪物共同出现时，才允许标题差异走目标回退。
  if (!entry.targetValidatedMatch || !entry.questCategory || !(entry.targetNamesEn?.length > 0)) return null;

  const sourceTargets = new Set(entry.targetNamesEn.map(normalizedExternalEnglishName).filter(Boolean));
  const candidates = [...questDescriptors.values()].filter((descriptor) => {
    if (descriptor.category !== entry.questCategory) return false;
    const targetNames = new Set(descriptor.targetNamesEn.map(normalizedExternalEnglishName));
    return [...sourceTargets].every((target) => targetNames.has(target));
  });
  return candidates.length === 1 ? candidates[0].quest : null;
}

function unlockConditionId(quest, entry, requirementIndex) {
  const questId = quest.id.slice("game-quest:".length);
  if (!entry.questCategory) return `game-unlock-condition:${questId}:${requirementIndex}`;
  // 新增的分类任务页可能用不同标题重复列出同一任务；来源与标题共同保证条件 ID 稳定且不冲突。
  const sourceTitleKey = normalizedExternalEnglishName(entry.questNameEn).replace(/[^a-z0-9]+/gu, "-").replace(/^-|-$/gu, "");
  return `game-unlock-condition:${questId}:${entry.sourceId}:${sourceTitleKey}:${requirementIndex}`;
}

function addGame8QuestUnlockFacts(
  entities,
  relations,
  questUnlockSnapshot,
  mhworldDataSnapshot,
  traditionalToSimplifiedNames,
  verifiedQuestEntityIds,
  curatedQuestNameMap,
  curatedLocations,
) {
  const empty = {
    questUnlockEntryCount: 0,
    prerequisiteRelationCount: 0,
    unlockConditionCount: 0,
    unresolvedQuestNameCount: 0,
    unresolvedMonsterNameCount: 0,
  };
  if (!questUnlockSnapshot || !mhworldDataSnapshot) return empty;

  const questsById = new Map(
    entities
      .filter((entity) => entity.kind === "quest" && entity.id.startsWith("game-quest:"))
      .map((entity) => [normalizedNumericId(entity.data.questId), entity])
      .filter(([id]) => id),
  );
  const questsByEnglishName = new Map();
  const questDescriptors = new Map();
  for (const sourceQuest of mhworldDataSnapshot.tables.questBase.rows) {
    const quest = questsById.get(normalizedNumericId(sourceQuest.id));
    if (quest && verifiedQuestEntityIds.has(quest.id)) {
      addNameCandidate(questsByEnglishName, sourceQuest.name_en, quest);
      questDescriptors.set(quest.id, {
        quest,
        category: valueText(sourceQuest.category),
        targetNamesEn: sourceQuestTargets(mhworldDataSnapshot, sourceQuest),
      });
    }
  }
  // 少量特别任务在外部表和本地表使用不同 ID；人工核对表是该例外的唯一入口。
  for (const [englishName, entry] of curatedQuestNameMap) {
    const quest = entities.find((entity) => entity.id === `game-quest:${entry.questId}`);
    if (!quest || quest.title !== entry.nameZhHans) {
      throw new Error(`特别任务名称映射未命中本地任务：${entry.questId}`);
    }
    quest.aliases = rawAliasValues(...quest.aliases, englishName);
    addNameCandidate(questsByEnglishName, englishName, quest);
  }

  const monstersByEnglishName = new Map();
  const monsters = entitiesByTitle(entities, "monster");
  const monsterTranslations = mapByUniqueKey(
    mhworldDataSnapshot.tables.monsterTranslations.rows,
    "name_en",
    "MHWData 怪物名称表",
  );
  for (const sourceMonster of mhworldDataSnapshot.tables.monsterBase.rows) {
    const match = uniqueTranslatedEntity(
      monsterTranslations,
      valueText(sourceMonster.name_en),
      traditionalToSimplifiedNames,
      monsters,
    );
    if (match) addNameCandidate(monstersByEnglishName, sourceMonster.name_en, match.entity);
  }

  const entriesByQuestName = new Map();
  for (const entry of questUnlockSnapshot.entries) {
    const key = normalizedExternalEnglishName(entry.questNameEn);
    const grouped = entriesByQuestName.get(key) ?? [];
    grouped.push(entry);
    entriesByQuestName.set(key, grouped);
  }

  let prerequisiteRelationCount = 0;
  let unlockConditionCount = 0;
  let unresolvedQuestNameCount = 0;
  let unresolvedMonsterNameCount = 0;
  let questUnlockEntryCount = 0;
  for (const entries of entriesByQuestName.values()) {
    // 同名任务在快照中有多条时，不选择其中任一条，避免不同版本或重复解析被混成确定关系。
    if (entries.length !== 1) {
      unresolvedQuestNameCount += entries.length;
      continue;
    }
    const entry = entries[0];
    const quest = questForUnlockEntry(entry, questsByEnglishName, questDescriptors);
    if (!quest) {
      unresolvedQuestNameCount += 1;
      continue;
    }
    questUnlockEntryCount += 1;
    for (const [requirementIndex, requirement] of entry.requirements.entries()) {
      const monster = requirement.monsterNameEn
        ? uniqueNameCandidate(monstersByEnglishName, requirement.monsterNameEn)
        : null;
      const prerequisite = requirement.kind === "completeQuest"
        ? uniqueNameCandidate(questsByEnglishName, requirement.questNameEn)
        : null;
      const locationNameZhHans = localizedQuestLocation(requirement, curatedLocations);
      if (requirement.monsterNameEn && !monster) unresolvedMonsterNameCount += 1;
      const conditionId = unlockConditionId(quest, entry, requirementIndex);
      const display = unlockRequirementDisplay(requirement, monster, prerequisite, locationNameZhHans);
      entities.push({
        id: conditionId,
        kind: "unlockCondition",
        domain: "game-quest",
        title: `${quest.title} 解锁条件 ${requirementIndex + 1}`,
        nameZhHans: `${quest.title} 解锁条件 ${requirementIndex + 1}`,
        aliases: rawAliasValues(
          quest.title,
          entry.questNameEn,
          display,
          requirement.questNameEn,
          requirement.monsterNameEn,
          requirement.npcNameEn,
          requirement.locationNameEn,
          requirement.operationNameEn,
        ),
        summary: `${quest.title} 的已核验解锁条件：${display}。来源为 Game8 任务资料页；未能唯一对应的英文名称只作为条件原文保留，不会被推断为游戏内关系。`,
        data: {
          questEntityId: quest.id,
          displayZhHans: display,
          requirement,
          relatedMonsterEntityId: monster?.id ?? null,
          locationNameZhHans,
          sourceUrl: entry.sourceUrl,
          sourcePageId: entry.sourceId,
          retrievedAt: questUnlockSnapshot.retrievedAt,
          contentBaselineVersion,
          sourceVersion: "community-unverified",
        },
        gameVersion: targetGameVersion,
        confidence: 0.68,
        sourceId: entry.sourceId,
      });
      relations.push({
        id: `relation:requiresCondition:${quest.id}:${conditionId}`,
        subjectId: quest.id,
        predicate: "requiresCondition",
        objectId: conditionId,
        gameVersion: targetGameVersion,
        confidence: 0.68,
        sourceId: entry.sourceId,
        data: { sourceUrl: entry.sourceUrl, sourcePageId: entry.sourceId, contentBaselineVersion },
      });
      unlockConditionCount += 1;

      if (requirement.kind !== "completeQuest") continue;
      if (!prerequisite) {
        unresolvedQuestNameCount += 1;
        continue;
      }
      const prerequisiteRelationId = `relation:requiresQuest:${quest.id}:${prerequisite.id}`;
      if (!relations.some((relation) => relation.id === prerequisiteRelationId)) {
        relations.push({
          id: prerequisiteRelationId,
          subjectId: quest.id,
          predicate: "requiresQuest",
          objectId: prerequisite.id,
          gameVersion: targetGameVersion,
          confidence: 0.68,
          sourceId: entry.sourceId,
          data: { sourceUrl: entry.sourceUrl, sourcePageId: entry.sourceId, contentBaselineVersion },
        });
        prerequisiteRelationCount += 1;
      }
    }
  }
  return {
    questUnlockEntryCount,
    prerequisiteRelationCount,
    unlockConditionCount,
    unresolvedQuestNameCount,
    unresolvedMonsterNameCount,
  };
}

function addMhworldDataQuestAndLocationFacts(entities, relations, snapshot, traditionalToSimplifiedNames, curatedLocations) {
  if (!snapshot) {
    return {
      questFactCount: 0,
      locationCount: 0,
      questRewardCount: 0,
      gatheringCount: 0,
      verifiedQuestEntityIds: new Set(),
    };
  }
  const questsById = new Map(
    entities
      .filter((entity) => entity.kind === "quest" && entity.id.startsWith("game-quest:"))
      .map((entity) => [normalizedNumericId(entity.data.questId), entity])
      .filter(([id]) => id),
  );
  const items = entitiesByTitle(entities, "item");
  const monsters = entitiesByTitle(entities, "monster");
  const questTranslationsById = new Map(
    snapshot.tables.questTranslations.rows
      .map((row) => [normalizedNumericId(row.id), row])
      .filter(([id]) => id),
  );
  const monsterTranslations = mapByUniqueKey(snapshot.tables.monsterTranslations.rows, "name_en", "MHWData 怪物名称表");
  const itemTranslations = mapByUniqueKey(snapshot.tables.itemTranslations.rows, "name_en", "MHWData 物品名称表");
  const stagesById = new Map(
    entities
      .filter((entity) => entity.kind === "stage" && entity.id.startsWith("game-stage:"))
      .map((entity) => [valueText(entity.data.stageId), entity])
      .filter(([id]) => id),
  );
  const locationsByEnglishName = new Map();
  let locationCount = 0;

  for (const sourceLocation of snapshot.tables.locationBase.rows) {
    const englishName = valueText(sourceLocation.name_en);
    const locationId = normalizedNumericId(sourceLocation.id);
    const curated = curatedLocations.get(englishName);
    const stage = curated ? stagesById.get(curated.stageId) : null;
    // 地图名称不走字形转换：必须由人工核对的英文名、外部地点 ID 与本地 STxxx 三者同时命中。
    if (!curated || !stage || !locationId || curated.locationId !== locationId || stage.title !== curated.nameZhHans) continue;
    const sourceNameZhHant = valueText(sourceLocation.name_zh);
    if (curated.nameZhHant && sourceNameZhHant && curated.nameZhHant !== sourceNameZhHant) continue;
    const id = `game-location:mhwdata:${locationId}`;
    entities.push({
      id,
      kind: "location",
      domain: "game-location",
      title: curated.nameZhHans,
      nameZhHans: curated.nameZhHans,
      aliases: rawAliasValues(curated.nameZhHans, curated.nameZhHant, englishName, locationId, curated.stageId),
      summary: "游戏地图资料。英文名、外部地点 ID 与本地 STxxx 场景 ID 已人工交叉核对；采集点和任务地点来自开发期资料。",
      data: {
        locationId,
        stageEntityId: stage.id,
        stageId: curated.stageId,
        englishName,
        nameZhHant: (curated.nameZhHant ?? sourceNameZhHant) || null,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
    });
    locationsByEnglishName.set(englishName, {
      id,
      display: { nameZhHans: curated.nameZhHans, nameZhHant: (curated.nameZhHant ?? sourceNameZhHant) || null },
    });
    locationCount += 1;
  }

  const questMonsterRows = groupByCompositeKey(snapshot.tables.questMonsters.rows, ["base_id"]);
  const questRewardRows = groupByCompositeKey(snapshot.tables.questRewards.rows, ["base_id"]);
  let questFactCount = 0;
  let questRewardCount = 0;
  const verifiedQuestEntityIds = new Set();
  for (const sourceQuest of snapshot.tables.questBase.rows) {
    const questId = normalizedNumericId(sourceQuest.id);
    const questEntity = questId ? questsById.get(questId) : null;
    const questTranslation = questId ? questTranslationsById.get(questId) : null;
    const nameZhHant = valueText(questTranslation?.name_zh);
    const nameZhHans = nameZhHant ? traditionalToSimplifiedNames.get(nameZhHant) : null;
    // 本地和外部任务表都以数值任务 ID 为稳定键；本地表的官方简中标题始终是展示名称。
    // 名称仅作为额外证据保存，不能因为翻译键缺失而丢弃正确的任务资料。
    if (!questEntity) continue;
    const location = locationsByEnglishName.get(valueText(sourceQuest.location_en)) ?? null;
    const targets = (questMonsterRows.get(questId) ?? []).flatMap((row) => {
      const match = uniqueTranslatedEntity(
        monsterTranslations,
        valueText(row.monster_en),
        traditionalToSimplifiedNames,
        monsters,
      );
      return [{
        monsterEntityId: match?.entity.id ?? null,
        nameZhHans: match?.entity.title ?? match?.translated.nameZhHans ?? null,
        nameZhHant: match?.translated.nameZhHant ?? null,
        nameEn: valueText(row.monster_en),
        quantity: Number(valueText(row.quantity)),
      isObjective: valueText(row.is_objective) === "TRUE",
      }];
    });
    const objectiveMonsterNames = targets
      .filter((target) => target.isObjective && target.nameZhHans)
      .map((target) => target.nameZhHans);
    // MHWData 的后期部分任务 ID 与本地表存在冲突。只有外部目标怪物都能在本地任务目标中验证时，
    // 才允许借用该外部行的英文名、报酬和解锁链；没有独立证据时宁可保留缺口。
    if (
      objectiveMonsterNames.length === 0
      || !objectiveMonsterNames.every((name) => questEntity.data.objective.includes(name))
    ) {
      continue;
    }
    verifiedQuestEntityIds.add(questEntity.id);
    // 目标怪物交叉核验通过后，数值 ID 才成为可信的跨来源键，可安全补齐英文与官方繁中别名。
    questEntity.aliases = rawAliasValues(...questEntity.aliases, sourceQuest.name_en, nameZhHant);
    const id = `game-quest-fact:mhwdata:${questId}`;
    entities.push({
      id,
      kind: "questFact",
      domain: "game-quest",
      title: `${questEntity.title}（任务资料）`,
      nameZhHans: `${questEntity.title}（任务资料）`,
      aliases: rawAliasValues(questEntity.title, nameZhHant, sourceQuest.name_en, questId),
      summary: `${questEntity.title} 的类别、等级、星级、地点、目标与报酬资料。任务 ID 已与本地表精确核对；外部字段版本尚未机器核验。`,
      data: {
        questEntityId: questEntity.id,
        englishName: sourceQuest.name_en,
        nameZhHant,
        category: valueText(sourceQuest.category),
        rank: valueText(sourceQuest.rank),
        stars: Number(valueText(sourceQuest.stars)),
        questType: valueText(sourceQuest.quest_type),
        zenny: Number(valueText(sourceQuest.zenny)),
        locationEntityId: location?.id ?? null,
        locationNameZhHans: location?.display.nameZhHans ?? null,
        objectiveZhHant: valueText(questTranslation?.objective_zh),
        descriptionZhHant: valueText(questTranslation?.description_zh),
        targets,
        retrievedAt: snapshot.retrievedAt,
        sourceCommit: snapshot.sourceCommit,
        contentBaselineVersion,
        sourceVersion: "unverified",
      },
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
    });
    relations.push({
      id: `relation:hasQuestFacts:${questEntity.id}:${id}`,
      subjectId: questEntity.id,
      predicate: "hasQuestFacts",
      objectId: id,
      gameVersion: "unverified",
      confidence: 0.78,
      sourceId: "mhworlddata-armor-name-map",
      data: { contentBaselineVersion, sourceCommit: snapshot.sourceCommit },
    });
    if (location) {
      relations.push({
        id: `relation:occursAt:${questEntity.id}:${location.id}`,
        subjectId: questEntity.id,
        predicate: "occursAt",
        objectId: location.id,
        gameVersion: "unverified",
        confidence: 0.78,
        sourceId: "mhworlddata-armor-name-map",
        data: { contentBaselineVersion, sourceCommit: snapshot.sourceCommit },
      });
    }
    for (const [rewardIndex, reward] of (questRewardRows.get(questId) ?? []).entries()) {
      const item = uniqueTranslatedEntity(itemTranslations, valueText(reward.item_en), traditionalToSimplifiedNames, items);
      if (!item) continue;
      relations.push({
        id: `relation:rewardsItem:${questEntity.id}:${item.entity.id}:${rewardIndex}`,
        subjectId: questEntity.id,
        predicate: "rewardsItem",
        objectId: item.entity.id,
        gameVersion: "unverified",
        confidence: 0.75,
        sourceId: "mhworlddata-armor-name-map",
        data: {
          group: valueText(reward.group),
          quantity: Number(valueText(reward.stack)),
          percentage: Number(valueText(reward.percentage)),
          contentBaselineVersion,
        },
      });
      questRewardCount += 1;
    }
    questFactCount += 1;
  }

  let gatheringCount = 0;
  for (const [gatheringIndex, row] of snapshot.tables.locationItems.rows.entries()) {
    const location = locationsByEnglishName.get(valueText(row.base_name_en));
    const item = uniqueTranslatedEntity(itemTranslations, valueText(row.item), traditionalToSimplifiedNames, items);
    if (!location || !item) continue;
    relations.push({
      id: `relation:gathersItem:${location.id}:${item.entity.id}:${gatheringIndex}`,
      subjectId: location.id,
      predicate: "gathersItem",
      objectId: item.entity.id,
      gameVersion: "unverified",
      confidence: 0.75,
      sourceId: "mhworlddata-armor-name-map",
      data: {
        area: valueText(row.area),
        rank: valueText(row.rank),
        quantity: Number(valueText(row.stack)),
        percentage: Number(valueText(row.percentage)),
        nodes: Number(valueText(row.nodes)),
        contentBaselineVersion,
      },
    });
    gatheringCount += 1;
  }
  return { questFactCount, locationCount, questRewardCount, gatheringCount, verifiedQuestEntityIds };
}

function rawGameRelations(entities, modelIndex) {
  const entityIds = new Set(entities.map((entity) => entity.id));
  const monsterEntities = entities
    .filter((entity) => entity.kind === "monster" && entity.id.startsWith("game-monster:"))
    // 名称长的先匹配，避免较短名称成为同一任务目标的唯一解释。
    .sort((left, right) => right.title.length - left.title.length);
  const weaponModels = new Map();
  for (const target of modelIndex.weaponRemapTargets ?? []) {
    weaponModels.set(
      `${target.weaponTypeId}:${target.mainModelPath || ""}:${target.accessoryModelPath || ""}`,
      target.targetId,
    );
  }
  const armorModels = new Map(
    (modelIndex.armorRemapTargets ?? []).map((target) => [target.modelId, target.targetId]),
  );
  const relations = [];
  const add = (subjectId, predicate, objectId, data = {}) => {
    if (!entityIds.has(subjectId) || !entityIds.has(objectId)) return;
    relations.push({
      id: `relation:${predicate}:${subjectId}:${objectId}`,
      subjectId,
      predicate,
      objectId,
      gameVersion: targetGameVersion,
      confidence: 1.0,
      sourceId: "mhwmod-jodo-ice-15.10",
      data: { ...data, contentBaselineVersion },
    });
  };
  for (const entity of entities) {
    if (entity.kind === "weapon" && entity.id.startsWith("game-weapon:")) {
      const { typeId, mainModelPath, accessoryModelPath } = entity.data;
      const targetId = weaponModels.get(`${typeId}:${mainModelPath || ""}:${accessoryModelPath || ""}`);
      if (targetId) add(entity.id, "usesAppearanceModel", targetId, { mainModelPath, accessoryModelPath });
    }
    if (entity.kind === "armor" && entity.id.startsWith("game-armor:")) {
      const targetId = armorModels.get(entity.data.modelPath);
      if (targetId) add(entity.id, "usesAppearanceModel", targetId, { modelPath: entity.data.modelPath });
    }
    if (entity.kind === "decoration" && entity.data.itemId) {
      add(entity.id, "hasItemRecord", `game-item:${entity.data.itemId}`, { itemId: entity.data.itemId });
    }
    if (entity.kind === "endemicLife" && /^\d+$/u.test(entity.data.itemId ?? "")) {
      add(entity.id, "collectsAsItem", `game-item:${entity.data.itemId}`, { itemId: entity.data.itemId });
    }
    if (entity.kind === "quest" && meaningfulText(entity.data.objective)) {
      const matchedMonsters = monsterEntities.filter((monster) => entity.data.objective.includes(monster.title));
      for (const monster of matchedMonsters) {
        const isNameFragmentOfLongerMatch = matchedMonsters.some(
          (other) => other.id !== monster.id && other.title.length > monster.title.length && other.title.includes(monster.title),
        );
        if (!isNameFragmentOfLongerMatch) {
          // 任务目标原文直接出现怪物官方简体名称时，才建立目标关系；
          // 短名称若只是较长怪物名称的一部分（如“贼龙”与“大贼龙”），
          // 只保留长名称；地图、解锁链和报酬等未明确字段不做推断。
          add(entity.id, "huntsMonster", monster.id, { objective: entity.data.objective });
        }
      }
    }
  }
  return relations;
}

async function rawGameFacts(modelIndex) {
  englishGameTextNames = await loadSimplifiedToEnglishNames();
  const entities = [];
  for (const table of rawGameTables) {
    const tablePath = path.join(rawGameDataRoot, table.file);
    let rows;
    try {
      rows = await readJsonLines(tablePath);
    } catch (error) {
      throw new Error(`无法读取本地 15.10.00 游戏事实源 ${table.file}：${error.message}`);
    }
    for (const row of rows) {
      const entity = table.entity(row);
      if (entity) entities.push(entity);
    }
  }
  const modelEntities = entityRows(modelIndex).map((entity) => ({
    ...entity,
    data: { ...entity.data, contentBaselineVersion },
    gameVersion: targetGameVersion,
    sourceId: "mhwmod-jodo-ice-15.10",
  }));
  const allEntities = [...modelEntities, ...entities];
  const duplicate = allEntities.find((entity, index) => allEntities.findIndex((other) => other.id === entity.id) !== index);
  if (duplicate) throw new Error(`游戏事实实体 ID 重复：${duplicate.id}`);
  const relations = rawGameRelations(allEntities, modelIndex);
  const [snapshot, mhworldDataArmorMap, traditionalToSimplifiedNames, curatedLocations, questUnlockSnapshot, curatedQuestNameMap] = await Promise.all([
    loadMhwDbSnapshot(),
    loadMhworldDataArmorNameMap(),
    loadTraditionalToSimplifiedNames(),
    loadCuratedLocationNameMap(),
    loadGame8QuestUnlockSnapshot(),
    loadCuratedQuestNameMap(),
  ]);
  const supplementalSkillFactCount = addMhwDbSkillFacts(
    allEntities,
    relations,
    snapshot,
  );
  const supplementalArmorFactCount = addMhworldDataArmorFacts(
    allEntities,
    relations,
    mhworldDataArmorMap,
    traditionalToSimplifiedNames,
  );
  const supplementalWeaponFactCount = addMhworldDataWeaponFacts(
    allEntities,
    relations,
    mhworldDataArmorMap,
    traditionalToSimplifiedNames,
  );
  const supplementalDecorationFactCount = addMhworldDataDecorationFacts(
    allEntities,
    relations,
    mhworldDataArmorMap,
    traditionalToSimplifiedNames,
  );
  const supplementalCharmCount = addMhworldDataCharms(
    allEntities,
    relations,
    mhworldDataArmorMap,
    traditionalToSimplifiedNames,
  );
  const supplementalMonsterFactCount = addMhworldDataMonsterFacts(
    allEntities,
    relations,
    mhworldDataArmorMap,
    traditionalToSimplifiedNames,
  );
  const questAndLocationFacts = addMhworldDataQuestAndLocationFacts(
    allEntities,
    relations,
    mhworldDataArmorMap,
    traditionalToSimplifiedNames,
    curatedLocations,
  );
  const questUnlockFacts = addGame8QuestUnlockFacts(
    allEntities,
    relations,
    questUnlockSnapshot,
    mhworldDataArmorMap,
    traditionalToSimplifiedNames,
    questAndLocationFacts.verifiedQuestEntityIds,
    curatedQuestNameMap,
    curatedLocations,
  );
  return {
    entities: allEntities,
    relations,
    supplementalSkillFactCount,
    supplementalArmorFactCount,
    supplementalWeaponFactCount,
    supplementalDecorationFactCount,
    supplementalCharmCount,
    supplementalMonsterFactCount,
    supplementalQuestFactCount: questAndLocationFacts.questFactCount,
    supplementalLocationCount: questAndLocationFacts.locationCount,
    supplementalQuestRewardCount: questAndLocationFacts.questRewardCount,
    supplementalGatheringCount: questAndLocationFacts.gatheringCount,
    ...questUnlockFacts,
  };
}

function validateKnowledgeDocuments(documentStore, sourceIds, label) {
  if (documentStore.schemaVersion !== 1 || !Array.isArray(documentStore.documents)) {
    throw new Error(`${label}结构无效。`);
  }
  const documentIds = new Set();
  for (const document of documentStore.documents) {
    for (const field of ["id", "domain", "title", "body", "gameVersion", "sourceId"]) {
      if (typeof document[field] !== "string" || !document[field].trim()) {
        throw new Error(`${label}缺少字段 ${field}。`);
      }
    }
    if (documentIds.has(document.id)) {
      throw new Error(`${label} ID 重复：${document.id}`);
    }
    if (!sourceIds.has(document.sourceId)) {
      throw new Error(`${label}引用了未登记来源：${document.sourceId}`);
    }
    if (typeof document.confidence !== "number" || document.confidence < 0 || document.confidence > 1) {
      throw new Error(`${label}置信度无效：${document.id}`);
    }
    documentIds.add(document.id);
  }
  return documentStore.documents;
}

async function main() {
  const outputPath = outputPathFromArguments(process.argv.slice(2));
  const moddingOutputPath = path.join(path.dirname(outputPath), "acumod-dev-modding.acukb");
  const guideOutputPath = path.join(path.dirname(outputPath), "acumod-dev-game-guides.acukb");
  const modelIndex = JSON.parse(await readFile(modelIndexPath, "utf8"));
  const sourceCatalog = JSON.parse(await readFile(sourceCatalogPath, "utf8"));
  const sourceIds = new Set(sourceCatalog.sources.map((source) => source.id));
  const technicalDocuments = validateKnowledgeDocuments(
    JSON.parse(await readFile(moddingDocumentsPath, "utf8")),
    sourceIds,
    "MOD 技术知识文件",
  );
  const guideDocuments = validateKnowledgeDocuments(
    JSON.parse(await readFile(gameGuideDocumentsPath, "utf8")),
    sourceIds,
    "游戏攻略知识文件",
  );
  const gameFacts = await rawGameFacts(modelIndex);
  const entities = gameFacts.entities;
  await mkdir(path.dirname(outputPath), { recursive: true });
  await rm(outputPath, { force: true });
  await rm(moddingOutputPath, { force: true });
  await rm(guideOutputPath, { force: true });

  const database = new DatabaseSync(outputPath);
  database.exec(`
    PRAGMA application_id = 1094931787;
    PRAGMA user_version = 1;
    PRAGMA foreign_keys = ON;
    CREATE TABLE pack_manifest (
      pack_id TEXT PRIMARY KEY,
      display_name TEXT NOT NULL,
      kind TEXT NOT NULL,
      version TEXT NOT NULL,
      game_version TEXT NOT NULL,
      locale TEXT NOT NULL,
      min_app_version TEXT NOT NULL,
      description TEXT NOT NULL
    );
    CREATE TABLE sources (
      id TEXT PRIMARY KEY,
      title TEXT NOT NULL,
      url TEXT,
      kind TEXT NOT NULL,
      game_version TEXT NOT NULL,
      license_note TEXT NOT NULL
    );
    CREATE TABLE entities (
      id TEXT PRIMARY KEY,
      kind TEXT NOT NULL,
      domain TEXT NOT NULL,
      canonical_name TEXT NOT NULL,
      name_zh_hans TEXT,
      name_zh_hant TEXT,
      summary TEXT NOT NULL,
      game_version TEXT NOT NULL,
      confidence REAL NOT NULL,
      source_id TEXT,
      data_json TEXT NOT NULL,
      FOREIGN KEY (source_id) REFERENCES sources(id)
    );
    CREATE TABLE aliases (
      entity_id TEXT NOT NULL,
      locale TEXT NOT NULL,
      alias TEXT NOT NULL,
      PRIMARY KEY (entity_id, locale, alias),
      FOREIGN KEY (entity_id) REFERENCES entities(id)
    );
    CREATE TABLE relations (
      id TEXT PRIMARY KEY,
      subject_id TEXT NOT NULL,
      predicate TEXT NOT NULL,
      object_id TEXT NOT NULL,
      game_version TEXT NOT NULL,
      confidence REAL NOT NULL,
      source_id TEXT,
      data_json TEXT NOT NULL,
      FOREIGN KEY (subject_id) REFERENCES entities(id),
      FOREIGN KEY (object_id) REFERENCES entities(id),
      FOREIGN KEY (source_id) REFERENCES sources(id)
    );
    CREATE TABLE documents (
      id TEXT PRIMARY KEY,
      namespace TEXT NOT NULL,
      title TEXT NOT NULL,
      body TEXT NOT NULL,
      game_version TEXT NOT NULL,
      confidence REAL NOT NULL,
      source_id TEXT,
      FOREIGN KEY (source_id) REFERENCES sources(id)
    );
    CREATE VIRTUAL TABLE knowledge_fts USING fts5(
      result_id UNINDEXED,
      result_kind UNINDEXED,
      domain UNINDEXED,
      title,
      body,
      tokenize='trigram'
    );
    CREATE INDEX aliases_alias_index ON aliases(alias);
    CREATE INDEX entities_kind_index ON entities(kind);
    CREATE INDEX relations_subject_index ON relations(subject_id, predicate);
    CREATE INDEX relations_object_index ON relations(object_id, predicate);
  `);

  database.exec("BEGIN IMMEDIATE");
  try {
    database.prepare("INSERT INTO pack_manifest VALUES (?, ?, ?, ?, ?, ?, ?, ?)").run(
      "acumod-dev-game-facts",
      "AcuAI 游戏事实开发包",
      "mhw-game-facts",
      "0.1.0-dev",
      targetGameVersion,
      "zh-Hans",
      "0.1.0",
      "目标运行版本为 15.23；内容事实以 15.10.00 为基线，项目已确认 15.11 至 15.23 未新增明显游戏内容或机制。该开发包仍不可作为正式知识包分发。",
    );
    const insertSource = database.prepare("INSERT INTO sources VALUES (?, ?, ?, ?, ?, ?)");
    for (const source of sourceCatalog.sources) {
      const licenseNote = [
        `用途：${source.usage}`,
        `分发：${source.redistribution}`,
        ...(source.notes ?? []),
      ].join("；");
      insertSource.run(
        source.id,
        source.title ?? source.id,
        source.url ?? null,
        source.kind,
        source.gameVersion,
        licenseNote,
      );
    }

    const insertEntity = database.prepare(
      "INSERT INTO entities VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    const insertAlias = database.prepare("INSERT OR IGNORE INTO aliases VALUES (?, ?, ?)");
    const insertRelation = database.prepare("INSERT INTO relations VALUES (?, ?, ?, ?, ?, ?, ?, ?)");
    const insertFts = database.prepare("INSERT INTO knowledge_fts VALUES (?, ?, ?, ?, ?)");
    for (const entity of entities) {
      insertEntity.run(
        entity.id,
        entity.kind,
        entity.domain,
        entity.title,
        entity.nameZhHans ?? entity.title,
        null,
        entity.summary,
        entity.gameVersion ?? modelIndex.gameVersion,
        entity.confidence ?? 0.9,
        entity.sourceId ?? "mhwmod-jodo-ice-15.10",
        JSON.stringify(entity.data),
      );
      for (const alias of entity.aliases) {
        insertAlias.run(entity.id, "und", alias);
      }
      insertFts.run(
        entity.id,
        "entity",
        entity.domain,
        entity.title,
        `${entity.summary} ${entity.aliases.join(" ")}`,
      );
    }
    for (const relation of gameFacts.relations) {
      insertRelation.run(
        relation.id,
        relation.subjectId,
        relation.predicate,
        relation.objectId,
        relation.gameVersion,
        relation.confidence,
        relation.sourceId,
        JSON.stringify(relation.data),
      );
    }

    const insertDocument = database.prepare("INSERT INTO documents VALUES (?, ?, ?, ?, ?, ?, ?)");
    for (const document of [...technicalDocuments, ...guideDocuments]) {
      insertDocument.run(
        document.id,
        document.domain,
        document.title,
        document.body,
        document.gameVersion,
        document.confidence,
        document.sourceId,
      );
      insertFts.run(document.id, "document", document.domain, document.title, document.body);
    }
    database.exec("COMMIT");
  } catch (error) {
    database.exec("ROLLBACK");
    throw error;
  }
  database.exec("VACUUM");
  const integrity = database.prepare("PRAGMA integrity_check(1)").get();
  database.close();
  if (integrity.integrity_check !== "ok") {
    throw new Error(`开发知识包完整性检查失败: ${integrity.integrity_check}`);
  }
  await copyFile(outputPath, moddingOutputPath);
  await copyFile(outputPath, guideOutputPath);
  pruneDevelopmentPack(outputPath, "mhw-game-facts");
  pruneDevelopmentPack(moddingOutputPath, "mhw-modding");
  pruneDevelopmentPack(guideOutputPath, "mhw-game-guides");
  process.stdout.write(
    `开发知识包已生成:\n- ${path.relative(projectRoot, outputPath)}\n- ${path.relative(projectRoot, moddingOutputPath)}\n- ${path.relative(projectRoot, guideOutputPath)}\n游戏实体 ${entities.length}，可验证关系 ${gameFacts.relations.length}，技能等级补充 ${gameFacts.supplementalSkillFactCount}，武器属性补充 ${gameFacts.supplementalWeaponFactCount}，防具属性补充 ${gameFacts.supplementalArmorFactCount}，装饰珠属性补充 ${gameFacts.supplementalDecorationFactCount}，护石补充 ${gameFacts.supplementalCharmCount}，怪物生态补充 ${gameFacts.supplementalMonsterFactCount}，任务资料 ${gameFacts.supplementalQuestFactCount}，地图 ${gameFacts.supplementalLocationCount}，任务报酬 ${gameFacts.supplementalQuestRewardCount}，采集关系 ${gameFacts.supplementalGatheringCount}，任务链条目 ${gameFacts.questUnlockEntryCount}，前置任务关系 ${gameFacts.prerequisiteRelationCount}，解锁条件 ${gameFacts.unlockConditionCount}，未唯一对应任务名 ${gameFacts.unresolvedQuestNameCount}，未唯一对应怪物名 ${gameFacts.unresolvedMonsterNameCount}，技术文档 ${technicalDocuments.length}，攻略文档 ${guideDocuments.length}\n`,
  );
}

function pruneDevelopmentPack(databasePath, kind) {
  const database = new DatabaseSync(databasePath);
  database.exec("PRAGMA foreign_keys = ON; BEGIN IMMEDIATE");
  try {
    if (kind === "mhw-game-facts") {
      database
        .prepare("UPDATE pack_manifest SET pack_id = ?, display_name = ?, kind = ?, description = ?")
        .run(
          "acumod-dev-game-facts",
          "AcuAI 游戏事实开发包",
          kind,
          "目标运行版本为 15.23；内容事实以 15.10.00 为基线，项目已确认 15.11 至 15.23 未新增明显游戏内容或机制。该开发包仍不可作为正式知识包分发。",
        );
      database.exec(`
        DELETE FROM knowledge_fts WHERE result_kind = 'document';
        DELETE FROM documents;
      `);
    } else if (kind === "mhw-modding") {
      database
        .prepare("UPDATE pack_manifest SET pack_id = ?, display_name = ?, kind = ?, game_version = ?, description = ?")
        .run(
          "acumod-dev-modding",
          "AcuAI MOD 技术开发包",
          kind,
          "15.23",
          "项目已验证技术规则的开发包，用于验证 MOD 文件知识检索链路。",
        );
      database.exec(`
        DELETE FROM aliases;
        DELETE FROM relations;
        DELETE FROM entities;
        DELETE FROM knowledge_fts WHERE domain NOT LIKE 'mod-%';
        DELETE FROM documents WHERE namespace NOT LIKE 'mod-%';
      `);
    } else if (kind === "mhw-game-guides") {
      database
        .prepare("UPDATE pack_manifest SET pack_id = ?, display_name = ?, kind = ?, game_version = ?, description = ?")
        .run(
          "acumod-dev-game-guides",
          "AcuAI 游戏攻略开发包",
          kind,
          targetGameVersion,
          "带来源、适用条件和明确边界的游戏攻略开发包；攻略建议必须与游戏事实包交叉核验。",
        );
      database.exec(`
        DELETE FROM aliases;
        DELETE FROM relations;
        DELETE FROM entities;
        DELETE FROM knowledge_fts WHERE domain NOT LIKE 'guide-%';
        DELETE FROM documents WHERE namespace NOT LIKE 'guide-%';
      `);
    } else {
      throw new Error(`不支持的开发知识包类型：${kind}`);
    }
    // FTS5 删除行后会保留旧分段；裁剪出的技术/攻略包若不重建索引，
    // 文件仍会接近完整事实包。这里从保留下来的 documents/entities 重建，
    // 让可选知识包只承担自己实际包含的内容体积。
    database.exec("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')");
    database.exec(`
      DELETE FROM sources
      WHERE id NOT IN (SELECT source_id FROM entities WHERE source_id IS NOT NULL)
        AND id NOT IN (SELECT source_id FROM relations WHERE source_id IS NOT NULL)
        AND id NOT IN (SELECT source_id FROM documents WHERE source_id IS NOT NULL);
      COMMIT;
      VACUUM;
    `);
    const integrity = database.prepare("PRAGMA integrity_check(1)").get();
    if (integrity.integrity_check !== "ok") {
      throw new Error(`${kind} 开发包完整性检查失败: ${integrity.integrity_check}`);
    }
  } catch (error) {
    try {
      database.exec("ROLLBACK");
    } catch {
      // COMMIT 后的完整性错误无需再次回滚。
    }
    throw error;
  } finally {
    database.close();
  }
}

await main();
