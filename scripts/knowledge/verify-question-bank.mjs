import { access } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const buildRoot = path.join(projectRoot, "references/knowledge/build");
const databasePath = path.join(buildRoot, "acumod-mhwdata-15.10.acumhwdb");
const packs = {
  modding: path.join(buildRoot, "acumod-dev-modding.acukb"),
  help: path.join(buildRoot, "acumod-dev-acumod-help.acukb"),
};

function expect(condition, message) {
  if (!condition) throw new Error(message);
}

async function requireFile(filePath) {
  try {
    await access(filePath);
  } catch {
    throw new Error(`缺少题库验收资料：${filePath}；请先运行 npm.cmd run knowledge:build-dev。`);
  }
}

for (const filePath of [databasePath, ...Object.values(packs)]) await requireFile(filePath);
const database = new DatabaseSync(databasePath, { readOnly: true });
const documents = Object.fromEntries(
  Object.entries(packs).map(([key, filePath]) => [key, new DatabaseSync(filePath, { readOnly: true })]),
);

function entity(kind, nameEn) {
  return database.prepare("SELECT id, name_zh_hans, data_json FROM entities WHERE kind = ? AND name_en = ?").get(kind, nameEn);
}

function rows(entityId, section) {
  return database
    .prepare("SELECT data_json FROM record_entities re JOIN records r ON r.id = re.record_id WHERE re.entity_id = ? AND r.section = ? ORDER BY r.id")
    .all(entityId, section)
    .map((row) => JSON.parse(row.data_json));
}

function hasDocument(pack, id) {
  return documents[pack].prepare("SELECT COUNT(*) AS count FROM documents WHERE id = ?").get(id).count === 1;
}

function monsterHas(nameEn, section) {
  const value = entity("monster", nameEn);
  return Boolean(value && rows(value.id, section).length > 0);
}

function skillHasLevels(nameEn) {
  const value = entity("skill", nameEn);
  return Boolean(value && rows(value.id, "skill.levels").length > 0);
}

const checks = [
  ["G01", "应答", () => {
    const rajang = entity("monster", "Rajang");
    const hitzones = rows(rajang.id, "monster.hitzones");
    return rajang?.name_zh_hans === "金狮子"
      && hitzones.some((row) => row.hitzone_en === "Head" && row.cut === "60" && row.impact === "62" && row.shot === "45")
      && hitzones.some((row) => row.hitzone_en === "Forelegs (Charged)" && row.cut === "10" && row.impact === "10" && row.shot === "5");
  }],
  ["G02", "应答", () => monsterHas("Furious Rajang", "monster.hitzones") && monsterHas("Furious Rajang", "monster.weaknesses")],
  ["G03", "应答", () => monsterHas("Velkhana", "monster.hitzones") && monsterHas("Velkhana", "monster.weaknesses")],
  ["G04", "应答", () => rows(entity("monster", "Alatreon").id, "monster.weaknesses").length >= 3],
  ["G05", "应答", () => {
    const hitzones = rows(entity("monster", "Raging Brachydios").id, "monster.hitzones");
    return hitzones.some((row) => row.hitzone_en === "Forearms" && row.cut === "55" && row.shot === "45")
      && hitzones.some((row) => row.hitzone_en === "Forearms (Red Slime)" && row.cut === "75" && row.shot === "10");
  }],
  ["G06", "应答", () => rows(entity("monster", "Rajang").id, "monster.rewards").some((row) => row.item_en === "Rajang Hardhorn" && row.condition_en.includes("Break Horn"))],
  ["G07", "应答", () => ["Alatreon Pallium", "Alatreon Mantle"].every((name) => {
    const item = entity("item", name);
    return item && rows(item.id, "monster.rewards").length > 0;
  })],
  ["G08", "安全缺口", () => Boolean(entity("quest", "Land of Convergence"))],
  // 游戏攻略不进入本地知识包；题库只验证如实说明缺口或提出必要追问。
  ["G09", "安全缺口", () => true],
  ["G10", "需上下文", () => true],
  ["G11", "应答", () => skillHasLevels("Handicraft")],
  ["G12", "部分应答", () => skillHasLevels("Latent Power")],
  ["G13", "应答", () => {
    const sets = database.prepare("SELECT id FROM entities WHERE kind = 'armorSet' AND name_en IN ('Dragon α+', 'Dragon β+')").all();
    return sets.length === 2 && [1571, 1572, 1573, 1574, 1575].every((id) => rows(`mhwdata:armor:${id}`, "armor.crafting").length === 1);
  }],
  ["G14", "部分应答", () => {
    const weapon = entity("weapon", "Fatalis Blade");
    return weapon?.name_zh_hans === "黑龙刃"
      && rows(weapon.id, "weapon.sharpness").some((row) => row.white === "30" && row.purple === "60")
      && skillHasLevels("Handicraft");
  }],
  ["G15", "安全缺口", () => Boolean(entity("weapon", "Fatalis Blade"))],
  ["G16", "安全缺口", () => true],
  ["G17", "需上下文", () => monsterHas("Rajang", "monster.weaknesses")],
  ["G18", "应答", () => monsterHas("Velkhana", "monster.weaknesses")],
  ["G19", "需上下文", () => true],
  ["G20", "需上下文", () => true],
  ["M01", "应答", () => hasDocument("help", "help-mod-root-detection")],
  ["M02", "应答", () => hasDocument("help", "help-conflict-priority")],
  ["M03", "应答", () => hasDocument("modding", "modding-mrl3")],
  ["M04", "应答", () => hasDocument("modding", "modding-armor-am-dat")],
  ["M05", "应答", () => hasDocument("modding", "modding-dat-armor-remap-boundary")],
  ["M06", "应答", () => hasDocument("modding", "modding-evam-slinger") && hasDocument("modding", "modding-slinger-chain")],
  ["M07", "应答", () => hasDocument("modding", "modding-weapon-epv-scope")],
  ["M08", "需上下文", () => hasDocument("help", "help-mod-enable-disable") && hasDocument("modding", "modding-component-evidence")],
  ["M09", "应答", () => hasDocument("modding", "modding-runtime-framework-boundary")],
  ["M10", "应答", () => hasDocument("modding", "modding-sharp-plugin-loader-csharp-plugin")],
];

try {
  const failed = checks.filter(([, , check]) => !check());
  expect(failed.length === 0, `题库资料或策略前提缺失：${failed.map(([id]) => id).join(", ")}`);
  const totals = checks.reduce((result, [, status]) => ({ ...result, [status]: (result[status] ?? 0) + 1 }), {});
  console.log(`30 题知识题库资料回归通过：应答 ${totals["应答"]}，部分应答 ${totals["部分应答"]}，安全缺口 ${totals["安全缺口"]}，需上下文 ${totals["需上下文"]}。`);
  console.log("该检查验证每题需要的本地事实/文档与预期边界；模型自然语言质量、追问是否恰当和最终引用标记仍由人工 AcuAI 对话验收。\n");
} finally {
  database.close();
  for (const pack of Object.values(documents)) pack.close();
}
