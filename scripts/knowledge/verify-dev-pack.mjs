import { access, readFile } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const buildRoot = path.join(projectRoot, "references/knowledge/build");
const mhwdataPath = path.join(buildRoot, "acumod-mhwdata-15.10.acumhwdb");
const documentPacks = [
  {
    path: path.join(buildRoot, "acumod-dev-modding.acukb"),
    kind: "mhw-modding",
    minimumDocuments: 30,
    requiredIds: ["modding-mod3", "modding-mrl3", "modding-evam-slinger", "modding-armor-am-dat", "modding-sharp-plugin-loader-csharp-plugin"],
  },
  {
    path: path.join(buildRoot, "acumod-dev-game-guides.acukb"),
    kind: "mhw-game-guides",
    minimumDocuments: 18,
    requiredIds: ["guide-greatsword-iceborne-midlate", "guide-guiding-lands-basics", "guide-fatalis-combat-preparation"],
  },
  {
    path: path.join(buildRoot, "acumod-dev-acumod-help.acukb"),
    kind: "acumod-help",
    minimumDocuments: 12,
    requiredIds: ["help-mod-import", "help-conflict-priority", "help-model-remap", "help-knowledge-pack", "help-acuai-boundary"],
  },
];
const sourceCatalogPath = path.join(projectRoot, "references/knowledge/sources/catalog.json");

function expect(condition, message) {
  if (!condition) throw new Error(`知识资料验收失败：${message}`);
}

function scalar(database, sql, parameters = []) {
  const row = database.prepare(sql).get(...parameters);
  return Object.values(row ?? {})[0];
}

async function requireFile(filePath) {
  try {
    await access(filePath);
  } catch {
    throw new Error(`缺少开发知识资料：${filePath}。请先运行 npm.cmd run knowledge:build-dev。`);
  }
}

function verifyMhwdata() {
  const database = new DatabaseSync(mhwdataPath, { readOnly: true });
  try {
    expect(scalar(database, "PRAGMA application_id") === 0x414D4844, "MHWData 数据库标识错误。");
    expect(scalar(database, "PRAGMA user_version") === 1, "MHWData 数据库 schema 版本错误。");
    expect(scalar(database, "PRAGMA integrity_check(1)") === "ok", "MHWData 数据库完整性检查失败。");
    expect(scalar(database, "SELECT id FROM mhwdata_manifest") === "mhwdata", "MHWData manifest ID 错误。");
    expect(scalar(database, "SELECT content_baseline_version FROM mhwdata_manifest") === "15.10.00", "内容基线必须固定为 15.10.00。");
    expect(scalar(database, "SELECT runtime_game_version FROM mhwdata_manifest") === "15.23", "运行兼容版本必须标记为 15.23。");
    expect(scalar(database, "SELECT COUNT(*) FROM source_tables") === 50, "MHWData 源表数量异常。");
    expect(scalar(database, "SELECT COUNT(*) FROM entities") >= 8_500, "MHWData 可查询实体数量异常。");
    expect(scalar(database, "SELECT COUNT(*) FROM records") >= 30_000, "MHWData 原始 CSV 行数量异常。");
    for (const section of ["weapon.sharpness", "weapon.crafting", "armor.skills", "armor.crafting", "monster.hitzones", "monster.rewards", "quest.rewards", "skill.levels", "decoration.dropRates"]) {
      expect(scalar(database, "SELECT COUNT(*) FROM records WHERE section = ?", [section]) > 0, `缺少 ${section} 原始行。`);
    }
    const defender = database.prepare("SELECT name_zh_hans, data_json FROM entities WHERE id = 'mhwdata:weapon:2001'").get();
    const defenderData = defender ? JSON.parse(defender.data_json) : null;
    expect(defender?.name_zh_hans === "防卫队炎刃型大剑1" && defenderData?.attack === "624", "防卫队炎刃型大剑 I 名称桥或攻击字段异常。");
    expect(
      scalar(database, "SELECT COUNT(*) FROM record_entities re JOIN records r ON r.id = re.record_id WHERE re.entity_id = 'mhwdata:weapon:2001' AND r.section = 'weapon.sharpness'") > 0,
      "武器斩味行没有关联到对应武器。",
    );
  } finally {
    database.close();
  }
}

function verifyDocumentPack(definition, sourceIds) {
  const database = new DatabaseSync(definition.path, { readOnly: true });
  try {
    expect(scalar(database, "PRAGMA application_id") === 0x4143554B, `${definition.kind} 标识错误。`);
    expect(scalar(database, "PRAGMA integrity_check(1)") === "ok", `${definition.kind} 完整性检查失败。`);
    expect(scalar(database, "SELECT kind FROM pack_manifest") === definition.kind, `${definition.kind} 类型错误。`);
    expect(scalar(database, "SELECT game_version FROM pack_manifest") === "15.23", `${definition.kind} 运行版本错误。`);
    expect(scalar(database, "SELECT COUNT(*) FROM entities") === 0, `${definition.kind} 不得包含游戏事实实体。`);
    expect(scalar(database, "SELECT COUNT(*) FROM relations") === 0, `${definition.kind} 不得包含游戏事实关系。`);
    expect(scalar(database, "SELECT COUNT(*) FROM documents") >= definition.minimumDocuments, `${definition.kind} 文档数量异常。`);
    for (const id of definition.requiredIds) {
      expect(scalar(database, "SELECT COUNT(*) FROM documents WHERE id = ?", [id]) === 1, `${definition.kind} 缺少 ${id}。`);
    }
    const sources = database.prepare("SELECT id FROM sources").all().map((row) => row.id);
    expect(sources.length > 0 && sources.every((id) => sourceIds.has(id)), `${definition.kind} 含未登记来源。`);
  } finally {
    database.close();
  }
}

const catalog = JSON.parse(await readFile(sourceCatalogPath, "utf8"));
expect(Array.isArray(catalog.sources) && catalog.sources.length > 0, "来源目录不能为空。");
const sourceIds = new Set(catalog.sources.map((source) => source.id));
await requireFile(mhwdataPath);
for (const definition of documentPacks) await requireFile(definition.path);
verifyMhwdata();
for (const definition of documentPacks) verifyDocumentPack(definition, sourceIds);
console.log("开发知识资料验收通过：MHWData 固定数值数据库、攻略、MOD 技术和 Acumod 使用说明均符合预期。");
