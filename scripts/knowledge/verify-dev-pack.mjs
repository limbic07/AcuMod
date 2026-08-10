import { access, readFile } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const buildRoot = path.join(projectRoot, "references/knowledge/build");
const gameFactsPath = path.join(buildRoot, "acumod-dev-game-facts.acukb");
const moddingPath = path.join(buildRoot, "acumod-dev-modding.acukb");
const guidesPath = path.join(buildRoot, "acumod-dev-game-guides.acukb");
const acumodHelpPath = path.join(buildRoot, "acumod-dev-acumod-help.acukb");
const sourceCatalogPath = path.join(projectRoot, "references/knowledge/sources/catalog.json");

async function requireFile(filePath) {
  try {
    await access(filePath);
  } catch {
    throw new Error(`缺少开发知识包：${filePath}。请先运行 npm.cmd run knowledge:build-dev。`);
  }
}

function scalar(database, sql, parameters = []) {
  const row = database.prepare(sql).get(...parameters);
  return Object.values(row ?? {})[0];
}

function expect(condition, message) {
  if (!condition) {
    throw new Error(`知识包验收失败：${message}`);
  }
}

async function verifySourceCatalog() {
  const catalog = JSON.parse(await readFile(sourceCatalogPath, "utf8"));
  expect(Array.isArray(catalog.sources) && catalog.sources.length > 0, "来源目录不能为空。");
  const ids = new Set();
  for (const source of catalog.sources) {
    expect(typeof source.id === "string" && source.id.trim(), "来源缺少稳定 ID。");
    expect(!ids.has(source.id), `来源 ID 重复：${source.id}。`);
    ids.add(source.id);
    for (const field of ["title", "kind", "gameVersion", "usage", "redistribution", "licenseStatus", "verificationStatus"]) {
      expect(typeof source[field] === "string" && source[field].trim(), `来源 ${source.id} 缺少 ${field}。`);
    }
    if (source.url !== null) {
      expect(
        typeof source.url === "string" && /^(https?|local):/.test(source.url),
        `来源 ${source.id} 的 URL 必须是 HTTP(S) 或明确的 local 记录。`,
      );
    }
    expect(
      Array.isArray(source.notes) && source.notes.every((note) => typeof note === "string" && note.trim()),
      `来源 ${source.id} 的 notes 必须是非空字符串数组。`,
    );
  }
  return ids;
}

function verifyPackSourceReferences(filePath, catalogIds, label) {
  const database = new DatabaseSync(filePath, { readOnly: true });
  try {
    const sourceIds = database.prepare("SELECT id FROM sources").all().map((row) => row.id);
    expect(sourceIds.length > 0, `${label} 没有来源记录。`);
    for (const sourceId of sourceIds) {
      expect(catalogIds.has(sourceId), `${label} 引用了未登记来源：${sourceId}。`);
    }
  } finally {
    database.close();
  }
}

function verifyMhworldDataFallbackFacts(database) {
  expect(
    String(scalar(database, "SELECT description FROM pack_manifest")).includes("MHWData 快照"),
    "MHWData 回退事实包必须在 manifest 中明确其本地开发边界。",
  );
  expect(
    scalar(database, "SELECT COUNT(*) FROM entities WHERE json_extract(data_json, '$.buildProfile') = 'mhworlddata-fallback'") >= 9_000,
    "MHWData 回退事实数量异常。",
  );
  expect(scalar(database, "SELECT COUNT(*) FROM entities") >= 11_000, "MHWData 回退实体数量异常。");
  expect(scalar(database, "SELECT COUNT(*) FROM relations") >= 37_000, "MHWData 回退关系数量异常。");
  for (const [kind, minimum] of [["weapon", 3_500], ["armor", 1_500], ["item", 1_300], ["monster", 90], ["quest", 500], ["skill", 170], ["location", 17], ["decoration", 390], ["charm", 300]]) {
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = ?", [kind]) >= minimum,
      `MHWData 回退缺少足量 ${kind} 实体。`,
    );
  }
  for (const [predicate, minimum] of [["grantsSkill", 3_000], ["requiresMaterial", 17_000], ["huntsMonster", 1_400], ["occursAt", 500], ["rewardsItem", 9_000], ["gathersItem", 900], ["hasWeaknessFacts", 80], ["hasHitzone", 780], ["upgradesFrom", 2_900]]) {
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = ?", [predicate]) >= minimum,
      `MHWData 回退缺少 ${predicate} 关系。`,
    );
  }
  const defender = database.prepare("SELECT canonical_name, name_zh_hant, data_json FROM entities WHERE id = 'game-weapon:mhwdata:2001'").get();
  const defenderData = defender ? JSON.parse(defender.data_json) : null;
  expect(
    defender?.canonical_name === "防卫队炎刃型大剑1"
      && defender?.name_zh_hant === "防衛隊炎刃型大劍Ⅰ"
      && defenderData?.attack === 624
      && defenderData?.weaponType === "great-sword",
    "防卫队炎刃型大剑 I 的同键名称桥或攻击字段异常。",
  );
  const leather = database.prepare("SELECT data_json FROM entities WHERE id = 'game-armor:mhwdata:1'").get();
  const leatherData = leather ? JSON.parse(leather.data_json) : null;
  expect(
    leatherData?.defenseBase === 2 && leatherData?.resistances?.fire === 2,
    "皮制头饰的防御或耐性字段异常。",
  );
  const firstQuest = database.prepare("SELECT data_json FROM entities WHERE id = 'game-quest:mhwdata:101'").get();
  const firstQuestData = firstQuest ? JSON.parse(firstQuest.data_json) : null;
  expect(
    firstQuestData?.locationEn === "Ancient Forest" && firstQuestData?.objectiveEn === "Slay 7 Jagras",
    "首个任务的地点或目标字段异常。",
  );
  const ancientForest = database.prepare("SELECT data_json FROM entities WHERE id = 'game-location:mhwdata:1'").get();
  const ancientForestData = ancientForest ? JSON.parse(ancientForest.data_json) : null;
  expect(
    ancientForestData?.stageId === "ST101",
    "古代树森林必须保留人工核对的 ST101 场景映射。",
  );
  expect(
    scalar(database, "SELECT COUNT(*) FROM relations WHERE subject_id = 'game-quest:mhwdata:101' AND predicate = 'occursAt' AND object_id = 'game-location:mhwdata:1'") === 1,
    "首个任务与古代树森林的地点关系缺失。",
  );
  expect(
    scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'unlockCondition'") >= 600,
    "MHWData 回退任务解锁条件覆盖异常。",
  );
  expect(
    scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest'") >= 200,
    "MHWData 回退任务前置关系覆盖异常。",
  );
  expect(
    scalar(database, "SELECT COUNT(*) FROM relations WHERE subject_id = 'game-quest:mhwdata:201' AND predicate = 'requiresQuest' AND object_id = 'game-quest:mhwdata:103'") === 1,
    "致力于设置营地与狩猎大贼龙的唯一英文标题前置关系缺失。",
  );
}

function verifyGameFacts() {
  const database = new DatabaseSync(gameFactsPath, { readOnly: true });
  try {
    expect(
      scalar(database, "SELECT game_version FROM pack_manifest") === "15.23",
      "开发游戏事实包必须声明 15.23 运行版本。",
    );
    if (scalar(database, "SELECT COUNT(*) FROM entities WHERE json_extract(data_json, '$.buildProfile') = 'mhworlddata-fallback'") > 0) {
      verifyMhworldDataFallbackFacts(database);
      return;
    }
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM entities WHERE json_extract(data_json, '$.contentBaselineVersion') = '15.10.00'",
      ) >= 15_000,
      "游戏实体缺少 15.10.00 内容基线来源标记。",
    );
    expect(scalar(database, "SELECT COUNT(*) FROM entities") >= 15_000, "游戏实体数量异常。");
    expect(scalar(database, "SELECT COUNT(*) FROM relations") >= 7_000, "可验证关系数量异常。");
    for (const kind of ["weapon", "armor", "item", "monster", "quest", "skill", "location"]) {
      expect(
        scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = ?", [kind]) > 0,
        `缺少 ${kind} 实体。`,
      );
    }
    for (const predicate of ["usesAppearanceModel", "hasItemRecord", "collectsAsItem", "hasArmorFacts", "grantsSkill", "requiresMaterial", "huntsMonster", "hasQuestFacts", "occursAt", "rewardsItem", "gathersItem"]) {
      expect(
        scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = ?", [predicate]) > 0,
        `缺少 ${predicate} 关系。`,
      );
    }
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'armorFact'") >= 1_500,
      "防具属性资料未通过官方简繁名称桥接导入。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'weaponFact'") >= 2_400,
      "武器属性资料未通过官方简繁名称和武器类型桥接导入。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'decorationFact'") >= 390,
      "装饰珠属性资料未通过官方简繁名称桥接导入。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'monsterFact'") >= 90,
      "怪物生态资料未通过官方简繁名称桥接导入。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'charm'") >= 300,
      "护石资料未通过官方简繁名称桥接导入。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'grantsSkill'") >= 1_900,
      "防具技能关系未通过完整简繁文本桥接导入。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresMaterial'") >= 5_000,
      "防具制作素材关系未通过完整简繁文本桥接导入。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'huntsMonster'") >= 390,
      "任务目标与怪物的精确名称关系数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'questFact'") >= 350,
      "经任务目标交叉核验的任务资料数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'location'") >= 16,
      "经本地 STxxx 场景映射核对的地图资料缺失。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'occursAt'") >= 350,
      "任务地点关系数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'rewardsItem'") >= 2_600,
      "任务报酬关系数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'gathersItem'") >= 900,
      "地图采集关系数量异常。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM relations WHERE predicate = 'huntsMonster' AND subject_id = 'game-quest:00103' AND object_id = 'game-monster:3'",
      ) === 0,
      "任务目标名称匹配错误地把“大贼龙”拆成了“贼龙”。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest' AND subject_id = 'game-quest:01121' AND object_id = 'game-quest:01101'",
      ) === 1,
      "深雪的潜水员与初次洗礼的已核验前置关系缺失。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest' AND subject_id = 'game-quest:00205' AND object_id = 'game-quest:00201'",
      ) === 1,
      "英文标题差异映射后，紧急任务·狩猎毒妖鸟与致力于设置营地的前置关系缺失。",
    );
    const pukeiUrgentCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-unlock-condition:00205:0'",
    ).get();
    expect(
      JSON.parse(pukeiUrgentCondition?.data_json ?? "{}").displayZhHans === "完成任务：致力于设置营地",
      "英文标题差异映射必须仍以官方简中任务名显示前置条件。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest' AND subject_id = object_id",
      ) === 0,
      "同名或近似名的指派/可选任务不得生成自指前置关系。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'unlockCondition'") >= 460,
      "活动、斗技场与挑战任务的解锁条件数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest'") >= 180,
      "活动、斗技场与挑战任务补充后，前置任务关系数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE kind = 'deliveryUnlockCondition'") >= 32,
      "本体交货委托的可核验解锁条件数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresCondition' AND subject_id LIKE 'game-delivery:%'") >= 32,
      "本体交货委托的解锁条件关系缺失。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest' AND subject_id LIKE 'game-delivery:%'") >= 21,
      "交货委托的唯一任务前置关系数量异常。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest' AND subject_id = object_id") === 0,
      "交货委托不得生成自指前置关系。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest' AND subject_id = 'game-delivery:1' AND object_id = 'game-quest:00601'") === 1,
      "草木们，健康地成长吧。与惊愕的！毒妖鸟！调查！的交货委托前置关系缺失。",
    );
    const deliveryCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-delivery-condition:1:0'",
    ).get();
    expect(
      JSON.parse(deliveryCondition?.data_json ?? "{}").displayZhHans?.includes("惊愕的！毒妖鸟！调查！"),
      "交货委托解锁条件没有保留简体中文显示文本。",
    );
    const arenaQuestCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-unlock-condition:03101:game8-arena:arena-quest-1:0'",
    ).get();
    expect(
      JSON.parse(arenaQuestCondition?.data_json ?? "{}").displayZhHans === "完成任务：贼龙与古代树森林",
      "斗技大会01的来源前置任务未正确关联到官方简中任务名。",
    );
    const challengeRankCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-unlock-condition:63073:game8-challenge:challenge-quest-1-mr-expert:0'",
    ).get();
    expect(
      JSON.parse(challengeRankCondition?.data_json ?? "{}").displayZhHans === "达到 MR 24",
      "大师上级挑战任务01的 MR 解锁条件缺失。",
    );
    const eventAvailabilityCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-unlock-condition:61606:game8-event-base:the-greatest-jagras:0'",
    ).get();
    expect(
      JSON.parse(eventAvailabilityCondition?.data_json ?? "{}").displayZhHans === "任务开放期间可承接",
      "活动任务的开放条件缺失。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresCondition' AND subject_id = 'game-quest:01272'",
      ) >= 2,
      "春神降临珊瑚大地的等级与发现条件缺失。",
    );
    const pinkPowerGrabCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-unlock-condition:01272:0'",
    ).get();
    expect(
      JSON.parse(pinkPowerGrabCondition?.data_json ?? "{}").displayZhHans === "达到 MR 6",
      "春神降临珊瑚大地的 MR 解锁条件未按结构化来源导入。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresCondition' AND subject_id = 'game-quest:01101'",
      ) >= 2,
      "初次洗礼的冰原主线和冰鱼龙解锁条件缺失。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM aliases WHERE entity_id = 'game-quest:01101' AND alias = 'Baptism by Ice'",
      ) === 1,
      "经任务目标核验的英文任务别名未写入基础任务实体。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities WHERE id = 'game-quest-fact:mhwdata:67803'") === 0,
      "存在冲突任务 ID 时不得将黑龙外部资料绑定到本地其他任务。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM relations WHERE predicate = 'requiresQuest' AND subject_id = 'game-quest:51612' AND object_id = 'game-quest:51613'",
      ) === 1,
      "人工核验的黑龙与破晓的凯旋前置关系缺失。",
    );
    const blackDragonCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-unlock-condition:51612:0'",
    ).get();
    expect(
      JSON.parse(blackDragonCondition?.data_json ?? "{}").displayZhHans === "完成任务：破晓的凯旋",
      "已映射的前置任务必须以官方简中名称展示。",
    );
    const baptismStoryCondition = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-unlock-condition:01101:1'",
    ).get();
    expect(
      JSON.parse(baptismStoryCondition?.data_json ?? "{}").displayZhHans === "完成本体主线",
      "本体主线解锁条件必须使用简体中文游戏术语展示。",
    );
    const leather = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-armor-fact:mhwdata:1'",
    ).get();
    const leatherData = leather ? JSON.parse(leather.data_json) : null;
    expect(
      leatherData?.armorEntityId === "game-armor:0:2"
        && leatherData?.skills?.[0]?.skillEntityId === "game-skill:77"
        && leatherData?.craftingMaterials?.[0]?.itemEntityId === "game-item:205",
      "皮制头饰的防具、技能和素材实体桥接异常。",
    );
    const defender = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-weapon-fact:mhwdata:2001'",
    ).get();
    const defenderData = defender ? JSON.parse(defender.data_json) : null;
    expect(
      defenderData?.weaponEntityId === "game-weapon:0:136"
        && defenderData?.attack === 624
        && defenderData?.craftingMaterials?.[0]?.itemEntityId === "game-item:205",
      "防卫队炎刃型大剑 I 的武器和素材实体桥接异常。",
    );
    const antidoteJewel = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-decoration-fact:mhwdata:1'",
    ).get();
    const antidoteJewelData = antidoteJewel ? JSON.parse(antidoteJewel.data_json) : null;
    expect(
      antidoteJewelData?.decorationEntityId === "game-decoration:0"
        && antidoteJewelData?.skills?.[0]?.skillEntityId === "game-skill:1",
      "耐毒珠的装饰珠和技能实体桥接异常。",
    );
    const poisonCharm = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-charm:mhwdata:1'",
    ).get();
    const poisonCharmData = poisonCharm ? JSON.parse(poisonCharm.data_json) : null;
    expect(
      poisonCharmData?.skills?.[0]?.skillEntityId === "game-skill:1"
        && poisonCharmData?.craftingMaterials?.[0]?.itemEntityId === "game-item:320",
      "耐毒护石的技能和素材实体桥接异常。",
    );
    const firstQuest = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-quest-fact:mhwdata:101'",
    ).get();
    const firstQuestData = firstQuest ? JSON.parse(firstQuest.data_json) : null;
    expect(
      firstQuestData?.questEntityId === "game-quest:00101"
        && firstQuestData?.locationEntityId === "game-location:mhwdata:1"
        && firstQuestData?.targets?.[0]?.monsterEntityId === "game-monster:3",
      "首个任务的稳定任务 ID、地图或目标怪物桥接异常。",
    );
    const blackDragonMap = database.prepare(
      "SELECT data_json FROM entities WHERE id = 'game-location:mhwdata:17'",
    ).get();
    const blackDragonMapData = blackDragonMap ? JSON.parse(blackDragonMap.data_json) : null;
    expect(
      blackDragonMapData?.stageEntityId === "game-stage:ST417"
        && blackDragonMapData?.stageId === "ST417",
      "虚黑城必须由稳定场景 ID 绑定，不能依赖缺失的简繁文本桥。",
    );
  } finally {
    database.close();
  }
}

function verifyModding() {
  const database = new DatabaseSync(moddingPath, { readOnly: true });
  try {
    expect(
      scalar(database, "SELECT kind FROM pack_manifest") === "mhw-modding",
      "MOD 技术包类型错误。",
    );
    expect(scalar(database, "SELECT COUNT(*) FROM documents") >= 30, "MOD 技术文档数量异常。");
    expect(
      scalar(database, "SELECT version FROM pack_manifest") === "0.3.0-dev",
      "MOD 技术包版本未升级。",
    );
    for (const id of ["modding-mod3", "modding-mrl3", "modding-evam-slinger", "modding-sobj-list", "modding-evwp", "modding-armor-am-dat", "modding-dat-armor-remap-boundary", "modding-runtime-framework-boundary", "modding-epvsp-effect-sound", "modding-ui-camera-scheduler", "modding-shell-parameter", "modding-sharp-plugin-loader-layout", "modding-sharp-plugin-loader-csharp-plugin"]) {
      expect(
        scalar(database, "SELECT COUNT(*) FROM documents WHERE id = ?", [id]) === 1,
        `缺少 ${id} 文档。`,
      );
    }
    expect(
      scalar(
        database,
        "SELECT s.url FROM documents d JOIN sources s ON s.id = d.source_id WHERE d.id = 'modding-armor-am-dat'",
      ) === "https://github.com/fre-sch/mhw_armor_edit",
      "armor.am_dat 文档必须保留可追溯的解析器来源。",
    );
    expect(
      scalar(
        database,
        "SELECT s.url FROM documents d JOIN sources s ON s.id = d.source_id WHERE d.id = 'modding-sharp-plugin-loader-csharp-plugin'",
      ) === "https://github.com/Fexty12573/SharpPluginLoader",
      "SPL 文档必须保留可追溯的作者来源。",
    );
    expect(
      scalar(
        database,
        "SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH 'armor' AND result_id = 'modding-armor-am-dat'",
      ) === 1,
      "armor.am_dat 技术文档未进入全文索引。",
    );
  } finally {
    database.close();
  }
}

function verifyGuides() {
  const database = new DatabaseSync(guidesPath, { readOnly: true });
  try {
    expect(
      scalar(database, "SELECT kind FROM pack_manifest") === "mhw-game-guides",
      "游戏攻略包类型错误。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities") === 0,
      "游戏攻略包不得混入游戏事实实体。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM documents WHERE namespace LIKE 'guide-%'") >= 18,
      "游戏攻略包缺少全武器推进与通用进度/战斗攻略文档。",
    );
    for (const id of [
      "guide-greatsword-iceborne-midlate",
      "guide-longsword-iceborne-midlate",
      "guide-sword-and-shield-iceborne-midlate",
      "guide-dual-blades-iceborne-midlate",
      "guide-hammer-iceborne-midlate",
      "guide-lance-iceborne-midlate",
      "guide-gunlance-iceborne-midlate",
      "guide-switch-axe-iceborne-midlate",
      "guide-charge-blade-iceborne-midlate",
      "guide-hunting-horn-iceborne-midlate",
      "guide-insect-glaive-iceborne-midlate",
      "guide-bow-iceborne-midlate",
      "guide-light-bowgun-iceborne-midlate",
      "guide-heavy-bowgun-iceborne-midlate",
      "guide-story-progression-basics",
      "guide-beginner-hunting-foundations",
      "guide-guiding-lands-basics",
      "guide-fatalis-combat-preparation",
    ]) {
      expect(
        scalar(database, "SELECT COUNT(*) FROM documents WHERE id = ?", [id]) === 1,
        `缺少首批冰原武器推进攻略文档：${id}。`,
      );
    }
  } finally {
    database.close();
  }
}

function verifyAcumodHelp() {
  const database = new DatabaseSync(acumodHelpPath, { readOnly: true });
  try {
    expect(
      scalar(database, "SELECT kind FROM pack_manifest") === "acumod-help",
      "Acumod 使用说明包类型错误。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM entities") === 0,
      "Acumod 使用说明包不得混入游戏实体。",
    );
    expect(
      scalar(database, "SELECT COUNT(*) FROM documents WHERE namespace LIKE 'help-%'") >= 12,
      "Acumod 使用说明包缺少核心操作说明。",
    );
    for (const id of [
      "help-mod-import",
      "help-conflict-priority",
      "help-model-remap",
      "help-knowledge-pack",
      "help-acuai-boundary",
    ]) {
      expect(
        scalar(database, "SELECT COUNT(*) FROM documents WHERE id = ?", [id]) === 1,
        "缺少 Acumod 使用说明：" + id + "。",
      );
    }
  } finally {
    database.close();
  }
}

const sourceCatalogIds = await verifySourceCatalog();
await requireFile(gameFactsPath);
await requireFile(moddingPath);
await requireFile(guidesPath);
await requireFile(acumodHelpPath);
verifyPackSourceReferences(gameFactsPath, sourceCatalogIds, "游戏事实包");
verifyPackSourceReferences(moddingPath, sourceCatalogIds, "MOD 技术包");
verifyPackSourceReferences(guidesPath, sourceCatalogIds, "攻略包");
verifyPackSourceReferences(acumodHelpPath, sourceCatalogIds, "Acumod 使用说明包");
verifyGameFacts();
verifyModding();
verifyGuides();
verifyAcumodHelp();
console.log("开发知识包验收通过：游戏事实、攻略、MOD 技术和 Acumod 使用说明均符合预期。");
