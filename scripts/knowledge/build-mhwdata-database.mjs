import { createHash } from "node:crypto";
import { access, mkdir, readFile, rm } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const snapshotPath = path.join(
  projectRoot,
  "references/knowledge/raw/mhworlddata/armor-name-map.json",
);
const gameTextBridgePath = path.join(
  projectRoot,
  "references/knowledge/raw/mhw-editor/game-text-bridge.json",
);
const defaultOutputPath = path.join(
  projectRoot,
  "references/knowledge/build/acumod-mhwdata-15.10.acumhwdb",
);
const runtimeGameVersion = "15.23";
const databaseApplicationId = 0x414D4844; // "AMHD"，用于防止把普通 SQLite 文件当作游戏数据库安装。

function outputPathFromArguments(argv) {
  const outputIndex = argv.findIndex((argument) => argument === "--output");
  if (outputIndex >= 0 && argv[outputIndex + 1]) return path.resolve(argv[outputIndex + 1]);
  const inline = argv.find((argument) => argument.startsWith("--output="));
  return inline ? path.resolve(inline.slice("--output=".length)) : defaultOutputPath;
}

function text(value) {
  return typeof value === "string" ? value.trim() : "";
}

function normalized(value) {
  return text(value).toLocaleLowerCase("en-US");
}

function stableReferenceId(prefix, value) {
  return `${prefix}:${createHash("sha256").update(value).digest("hex").slice(0, 20)}`;
}

function readRows(snapshot, key) {
  const table = snapshot.tables?.[key];
  if (!table || !Array.isArray(table.rows)) throw new Error(`MHWData 快照缺少表：${key}`);
  return table.rows;
}

function translationIndex(rows, keys = ["name_en"]) {
  const result = new Map();
  for (const row of rows) {
    const key = keys.map((field) => normalized(row[field])).join("\u001F");
    if (key && !result.has(key)) result.set(key, row);
  }
  return result;
}

async function readJson(filePath, required = true) {
  try {
    return JSON.parse(await readFile(filePath, "utf8"));
  } catch (error) {
    if (!required && error?.code === "ENOENT") return null;
    throw error;
  }
}

function simplifiedToTraditionalMap(bridge) {
  const candidates = new Map();
  for (const [simplified, traditional] of Object.entries(bridge?.names ?? {})) {
    const source = text(simplified);
    const target = text(traditional);
    if (!source || !target) continue;
    const values = candidates.get(target) ?? new Set();
    values.add(source);
    candidates.set(target, values);
  }
  // 只采纳一对一文本桥；一对多时宁可保留上游繁中，避免猜测简中译名。
  return new Map([...candidates].filter(([, values]) => values.size === 1).map(([target, values]) => [target, [...values][0]]));
}

function entityNames(baseRow, translation, hantToHans) {
  const nameEn = text(baseRow.name_en);
  const nameZhHant = text(translation?.name_zh ?? baseRow.name_zh);
  const nameZhHans = hantToHans.get(nameZhHant) ?? nameZhHant;
  return { nameEn, nameZhHans, nameZhHant };
}

function tableDigest(snapshot) {
  return createHash("sha256")
    .update(JSON.stringify(Object.fromEntries(Object.entries(snapshot.tables).map(([key, value]) => [key, value.sha256]))))
    .digest("hex");
}

const snapshot = await readJson(snapshotPath);
if (snapshot.schemaVersion !== 1 || snapshot.sourceId !== "mhworlddata-armor-name-map") {
  throw new Error("MHWData 快照格式不受支持；请先重新执行 knowledge:fetch-mhworlddata。");
}
const bridge = await readJson(gameTextBridgePath, false);
const hantToHans = simplifiedToTraditionalMap(bridge);
const outputPath = outputPathFromArguments(process.argv.slice(2));
await mkdir(path.dirname(outputPath), { recursive: true });
await rm(outputPath, { force: true });

const database = new DatabaseSync(outputPath);
const entities = new Map();
const idsByKindAndEnglishName = new Map();
const idsByKindAndSourceId = new Map();

function addEntity({ id, kind, sourceKey, row, translation, aliases = [] }) {
  if (entities.has(id)) throw new Error(`MHWData 实体 ID 重复：${id}`);
  const names = entityNames(row, translation, hantToHans);
  const entity = {
    id,
    kind,
    sourceKey,
    ...names,
    data: row,
    aliases: new Set([id, names.nameEn, names.nameZhHans, names.nameZhHant, ...aliases].map(text).filter(Boolean)),
  };
  entities.set(id, entity);
  if (names.nameEn) idsByKindAndEnglishName.set(`${kind}\u001F${normalized(names.nameEn)}`, id);
  const sourceId = text(row.id);
  if (sourceId) idsByKindAndSourceId.set(`${kind}\u001F${sourceId}`, id);
  return id;
}

function entityIdByEnglishName(kind, name) {
  return idsByKindAndEnglishName.get(`${kind}\u001F${normalized(name)}`) ?? null;
}

function entityIdBySourceId(kind, sourceId) {
  return idsByKindAndSourceId.get(`${kind}\u001F${text(sourceId)}`) ?? null;
}

function createNamedEntities({ baseKey, translationKey, kind, idForRow, translationKeys }) {
  const baseRows = readRows(snapshot, baseKey);
  const translations = translationKey ? translationIndex(readRows(snapshot, translationKey), translationKeys) : new Map();
  for (const row of baseRows) {
    const translationKeyValue = (translationKeys ?? ["name_en"]).map((field) => normalized(row[field])).join("\u001F");
    addEntity({
      id: idForRow(row),
      kind,
      sourceKey: `${baseKey}:${text(row.id) || normalized(row.name_en)}`,
      row,
      translation: translations.get(translationKeyValue),
    });
  }
}

createNamedEntities({
  baseKey: "weaponBase", translationKey: "weaponTranslations", kind: "weapon",
  idForRow: (row) => `mhwdata:weapon:${row.id}`,
  translationKeys: ["name_en", "weapon_type"],
});
createNamedEntities({ baseKey: "armorBase", translationKey: "armorTranslations", kind: "armor", idForRow: (row) => `mhwdata:armor:${row.id}` });
createNamedEntities({ baseKey: "decorationBase", translationKey: "decorationTranslations", kind: "decoration", idForRow: (row) => `mhwdata:decoration:${row.id}` });
createNamedEntities({ baseKey: "charmBase", translationKey: "charmTranslations", kind: "charm", idForRow: (row) => `mhwdata:charm:${row.id}` });
createNamedEntities({ baseKey: "itemBase", translationKey: "itemTranslations", kind: "item", idForRow: (row) => `mhwdata:item:${row.id}` });
createNamedEntities({ baseKey: "monsterBase", translationKey: "monsterTranslations", kind: "monster", idForRow: (row) => `mhwdata:monster:${row.id}` });
createNamedEntities({ baseKey: "questBase", translationKey: "questTranslations", kind: "quest", idForRow: (row) => `mhwdata:quest:${row.id}` });
createNamedEntities({ baseKey: "locationBase", translationKey: null, kind: "location", idForRow: (row) => `mhwdata:location:${row.id}` });
createNamedEntities({ baseKey: "toolBase", translationKey: "toolTranslations", kind: "tool", idForRow: (row) => `mhwdata:tool:${row.id}` });
createNamedEntities({ baseKey: "kinsectBase", translationKey: "kinsectTranslations", kind: "kinsect", idForRow: (row) => `mhwdata:kinsect:${row.id}` });
createNamedEntities({ baseKey: "armorSets", translationKey: "armorSetTranslations", kind: "armorSet", idForRow: (row) => stableReferenceId("mhwdata:armor-set", row.name_en) });
createNamedEntities({ baseKey: "armorSetBonuses", translationKey: "armorSetBonusTranslations", kind: "armorSetBonus", idForRow: (row) => stableReferenceId("mhwdata:armor-set-bonus", row.name_en) });
createNamedEntities({ baseKey: "skillBase", translationKey: "skillTranslations", kind: "skill", idForRow: (row) => stableReferenceId("mhwdata:skill", row.name_en) });

// 这些是全局数值表的稳定查询锚点；表内每行仍保持原始 CSV 字段。
addEntity({
  id: "mhwdata:table:decoration-drop-rates",
  kind: "dropRateTable",
  sourceKey: "decorationDropRates",
  row: { name_en: "Decoration feystone drop rates", content_baseline_version: snapshot.contentBaselineVersion },
  aliases: ["装饰珠掉落率", "鉴定珠掉落率", "feystone drop rates"],
});

database.exec(`
  PRAGMA application_id = ${databaseApplicationId};
  PRAGMA user_version = 1;
  PRAGMA foreign_keys = ON;
  CREATE TABLE mhwdata_manifest (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    format_version INTEGER NOT NULL,
    content_baseline_version TEXT NOT NULL,
    runtime_game_version TEXT NOT NULL,
    source_repository TEXT NOT NULL,
    source_commit TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    built_at TEXT NOT NULL
  ) STRICT;
  CREATE TABLE source_tables (
    table_key TEXT PRIMARY KEY,
    source_path TEXT NOT NULL,
    source_url TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    row_count INTEGER NOT NULL CHECK (row_count >= 0)
  ) STRICT;
  CREATE TABLE entities (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    source_key TEXT NOT NULL,
    name_en TEXT NOT NULL,
    name_zh_hans TEXT,
    name_zh_hant TEXT,
    data_json TEXT NOT NULL
  ) STRICT;
  CREATE TABLE aliases (
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    locale TEXT NOT NULL,
    alias TEXT NOT NULL,
    normalized_alias TEXT NOT NULL,
    PRIMARY KEY (entity_id, locale, alias)
  ) STRICT;
  CREATE TABLE records (
    id INTEGER PRIMARY KEY,
    section TEXT NOT NULL,
    source_table TEXT NOT NULL REFERENCES source_tables(table_key),
    data_json TEXT NOT NULL
  ) STRICT;
  CREATE TABLE record_entities (
    record_id INTEGER NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    PRIMARY KEY (record_id, entity_id)
  ) STRICT;
  CREATE INDEX aliases_normalized_index ON aliases(normalized_alias, entity_id);
  CREATE INDEX entities_kind_index ON entities(kind, name_en);
  CREATE INDEX record_entities_entity_index ON record_entities(entity_id, record_id);
  CREATE INDEX records_section_index ON records(section, id);
`);

const insertEntity = database.prepare("INSERT INTO entities VALUES (?, ?, ?, ?, ?, ?, ?)");
const insertAlias = database.prepare("INSERT INTO aliases VALUES (?, ?, ?, ?)");
const insertSourceTable = database.prepare("INSERT INTO source_tables VALUES (?, ?, ?, ?, ?)");
const insertRecord = database.prepare("INSERT INTO records(section, source_table, data_json) VALUES (?, ?, ?)");
const insertRecordEntity = database.prepare("INSERT OR IGNORE INTO record_entities VALUES (?, ?)");

function aliasesFor(entity) {
  const result = [];
  const push = (locale, value) => {
    const alias = text(value);
    if (alias) result.push([locale, alias]);
  };
  push("id", entity.id);
  push("en", entity.nameEn);
  push("zh-Hans", entity.nameZhHans);
  push("zh-Hant", entity.nameZhHant);
  for (const alias of entity.aliases) push("alias", alias);
  return result;
}

function relatedMaterials(row) {
  const values = [];
  for (let index = 1; index <= 4; index += 1) {
    const value = text(row[`item${index}_name`]);
    if (value) values.push(entityIdByEnglishName("item", value));
  }
  return values.filter(Boolean);
}

function attachRecord(section, sourceTable, row, entityIds = []) {
  const inserted = insertRecord.run(section, sourceTable, JSON.stringify(row));
  const recordId = Number(inserted.lastInsertRowid);
  for (const entityId of new Set(entityIds.filter(Boolean))) insertRecordEntity.run(recordId, entityId);
}

function attachAllRows(tableKey, section, targetIds) {
  for (const row of readRows(snapshot, tableKey)) attachRecord(section, tableKey, row, targetIds(row));
}

database.exec("BEGIN IMMEDIATE");
try {
  database.prepare("INSERT INTO mhwdata_manifest VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)").run(
    "mhwdata",
    "Monster Hunter: World 游戏数值数据库",
    1,
    snapshot.contentBaselineVersion,
    runtimeGameVersion,
    "https://github.com/gatheringhallstudios/MHWorldData",
    snapshot.sourceCommit,
    tableDigest(snapshot),
    new Date().toISOString(),
  );
  for (const [key, table] of Object.entries(snapshot.tables)) {
    insertSourceTable.run(key, table.sourcePath, table.sourceUrl, table.sha256, table.rows.length);
  }
  for (const entity of entities.values()) {
    insertEntity.run(
      entity.id, entity.kind, entity.sourceKey, entity.nameEn,
      entity.nameZhHans || null, entity.nameZhHant || null, JSON.stringify(entity.data),
    );
    for (const [locale, alias] of aliasesFor(entity)) insertAlias.run(entity.id, locale, alias, normalized(alias));
  }

  // 所有源表均入库；下面的关联只决定某实体查询时应取得哪些原始 CSV 行。
  attachAllRows("weaponBase", "weapon.base", (row) => [entityIdBySourceId("weapon", row.id)]);
  attachAllRows("weaponTranslations", "weapon.translation", (row) => [entityIdByEnglishName("weapon", row.name_en)]);
  attachAllRows("weaponCrafting", "weapon.crafting", (row) => [entityIdByEnglishName("weapon", row.base_name_en), ...relatedMaterials(row)]);
  attachAllRows("weaponSharpness", "weapon.sharpness", (row) => [entityIdByEnglishName("weapon", row.base_name_en)]);
  attachAllRows("weaponAmmo", "weapon.ammo", (row) => readRows(snapshot, "weaponBase").filter((weapon) => text(weapon.ammo_config) === text(row.key)).map((weapon) => entityIdBySourceId("weapon", weapon.id)));
  attachAllRows("weaponBow", "weapon.bow", (row) => [entityIdByEnglishName("weapon", row.base_name_en)]);
  attachAllRows("weaponMelodies", "weapon.melody", () => []);
  attachAllRows("weaponMelodyNotes", "weapon.melodyNotes", (row) => [entityIdByEnglishName("weapon", row.base_name_en)]);
  attachAllRows("kinsectBase", "kinsect.base", (row) => [entityIdBySourceId("kinsect", row.id)]);
  attachAllRows("kinsectTranslations", "kinsect.translation", (row) => [entityIdByEnglishName("kinsect", row.name_en)]);
  attachAllRows("kinsectCrafting", "kinsect.crafting", (row) => [entityIdByEnglishName("kinsect", row.base_name_en), ...relatedMaterials(row)]);
  attachAllRows("armorBase", "armor.base", (row) => [entityIdBySourceId("armor", row.id)]);
  attachAllRows("armorTranslations", "armor.translation", (row) => [entityIdByEnglishName("armor", row.name_en)]);
  attachAllRows("armorSkills", "armor.skills", (row) => [entityIdByEnglishName("armor", row.base_name_en), entityIdByEnglishName("skill", row.skill1_name), entityIdByEnglishName("skill", row.skill2_name)]);
  attachAllRows("armorCrafting", "armor.crafting", (row) => [entityIdByEnglishName("armor", row.base_name_en), ...relatedMaterials(row)]);
  attachAllRows("armorSets", "armorSet.base", (row) => [entityIdByEnglishName("armorSet", row.name_en), ...["head", "chest", "arms", "waist", "legs"].map((part) => entityIdByEnglishName("armor", row[part]))]);
  attachAllRows("armorSetTranslations", "armorSet.translation", (row) => [entityIdByEnglishName("armorSet", row.name_en)]);
  attachAllRows("armorSetBonuses", "armorSetBonus.base", (row) => [entityIdByEnglishName("armorSetBonus", row.name_en), entityIdByEnglishName("skill", row.skill1_name), entityIdByEnglishName("skill", row.skill2_name)]);
  attachAllRows("armorSetBonusTranslations", "armorSetBonus.translation", (row) => [entityIdByEnglishName("armorSetBonus", row.name_en)]);
  attachAllRows("decorationBase", "decoration.base", (row) => [entityIdBySourceId("decoration", row.id), entityIdByEnglishName("skill", row.skill1_name), entityIdByEnglishName("skill", row.skill2_name)]);
  attachAllRows("decorationTranslations", "decoration.translation", (row) => [entityIdByEnglishName("decoration", row.name_en)]);
  attachAllRows("decorationDropRates", "decoration.dropRates", () => ["mhwdata:table:decoration-drop-rates"]);
  attachAllRows("charmBase", "charm.base", (row) => [entityIdBySourceId("charm", row.id), entityIdByEnglishName("skill", row.skill1_name), entityIdByEnglishName("skill", row.skill2_name)]);
  attachAllRows("charmTranslations", "charm.translation", (row) => [entityIdByEnglishName("charm", row.name_en)]);
  attachAllRows("charmCrafting", "charm.crafting", (row) => [entityIdByEnglishName("charm", row.base_name_en), ...relatedMaterials(row)]);
  attachAllRows("itemBase", "item.base", (row) => [entityIdBySourceId("item", row.id)]);
  attachAllRows("itemTranslations", "item.translation", (row) => [entityIdByEnglishName("item", row.name_en)]);
  attachAllRows("itemCombinations", "item.combination", (row) => [entityIdByEnglishName("item", row.result), entityIdByEnglishName("item", row.first), entityIdByEnglishName("item", row.second)]);
  attachAllRows("monsterBase", "monster.base", (row) => [entityIdBySourceId("monster", row.id)]);
  attachAllRows("monsterTranslations", "monster.translation", (row) => [entityIdByEnglishName("monster", row.name_en)]);
  attachAllRows("monsterAilments", "monster.ailments", (row) => [entityIdByEnglishName("monster", row.base_name_en)]);
  attachAllRows("monsterBreaks", "monster.breaks", (row) => [entityIdByEnglishName("monster", row.base_name_en)]);
  attachAllRows("monsterHabitats", "monster.habitats", (row) => [entityIdByEnglishName("monster", row.base_name_en), entityIdByEnglishName("location", row.map_en)]);
  attachAllRows("monsterWeaknesses", "monster.weaknesses", (row) => [entityIdByEnglishName("monster", row.name_en)]);
  attachAllRows("monsterHitzones", "monster.hitzones", (row) => [entityIdByEnglishName("monster", row.base_name_en)]);
  attachAllRows("monsterRewards", "monster.rewards", (row) => [entityIdByEnglishName("monster", row.base_name_en), entityIdByEnglishName("item", row.item_en)]);
  attachAllRows("rewardConditions", "monster.rewardCondition", () => []);
  attachAllRows("questBase", "quest.base", (row) => [entityIdBySourceId("quest", row.id), entityIdByEnglishName("location", row.location_en)]);
  attachAllRows("questTranslations", "quest.translation", (row) => [entityIdBySourceId("quest", row.id)]);
  attachAllRows("questMonsters", "quest.monsters", (row) => [entityIdBySourceId("quest", row.base_id), entityIdByEnglishName("monster", row.monster_en)]);
  attachAllRows("questRewards", "quest.rewards", (row) => [entityIdBySourceId("quest", row.base_id), entityIdByEnglishName("item", row.item_en)]);
  attachAllRows("locationBase", "location.base", (row) => [entityIdBySourceId("location", row.id)]);
  attachAllRows("locationCamps", "location.camps", (row) => [entityIdByEnglishName("location", row.base_name_en)]);
  attachAllRows("locationItems", "location.items", (row) => [entityIdByEnglishName("location", row.base_name_en)]);
  attachAllRows("gatheringStacks", "location.gatheringStacks", () => []);
  attachAllRows("toolBase", "tool.base", (row) => [entityIdBySourceId("tool", row.id)]);
  attachAllRows("toolTranslations", "tool.translation", (row) => [entityIdByEnglishName("tool", row.name_en)]);
  attachAllRows("skillBase", "skill.base", (row) => [entityIdByEnglishName("skill", row.name_en)]);
  attachAllRows("skillTranslations", "skill.translation", (row) => [entityIdByEnglishName("skill", row.name_en)]);
  attachAllRows("skillLevels", "skill.levels", (row) => [entityIdByEnglishName("skill", row.base_name_en)]);
  database.exec("COMMIT; VACUUM");
  const integrity = database.prepare("PRAGMA integrity_check(1)").get();
  if (integrity.integrity_check !== "ok") throw new Error(`MHWData 数据库完整性检查失败：${integrity.integrity_check}`);
} catch (error) {
  try { database.exec("ROLLBACK"); } catch { /* 已提交的事务无需回滚。 */ }
  throw error;
} finally {
  database.close();
}

await access(outputPath);
console.log(`已生成 MHWData 受控查询数据库：${path.relative(projectRoot, outputPath)}`);
console.log(`实体 ${entities.size}；源表 ${Object.keys(snapshot.tables).length}；内容基线 ${snapshot.contentBaselineVersion}；运行版本 ${runtimeGameVersion}`);
