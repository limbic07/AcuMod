import { lstat, mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";
import process from "node:process";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const rawPackageRoot = path.join(
  projectRoot,
  "references/mhwi-data/raw/15.10.00-agent-package",
);
const rawManifestPath = path.join(rawPackageRoot, "manifest.json");
const mhworldDataSnapshotPath = path.join(
  projectRoot,
  "references/knowledge/raw/mhworlddata/armor-name-map.json",
);
const curatedRoot = path.join(projectRoot, "references/mhwi-data/curated");
const defaultOutputRoot = path.join(projectRoot, "references/knowledge/audits");
const deliveryUnlockSourcePath = path.join(projectRoot, "references/knowledge/sources/delivery-unlock-documents.json");
const developmentMhwdataPath = path.join(projectRoot, "references/knowledge/build/acumod-mhwdata-15.10.acumhwdb");

const coverageDefinitions = [
  {
    topic: "weapons",
    label: "武器",
    status: "partial",
    tables: ["weapons"],
    available: "名称、类型、稀有度、模型路径",
    missing: "攻击属性、斩味、孔位、升级树与素材配方",
  },
  {
    topic: "armor",
    label: "防具与外观装备",
    status: "partial",
    tables: ["armor", "armor_series"],
    available: "名称、部位、稀有度、防御、幻化 ID 与模型路径",
    missing: "技能、孔位、抗性与制作素材",
  },
  {
    topic: "skills",
    label: "技能",
    status: "partial",
    tables: ["skills"],
    available: "技能名称与基础说明",
    missing: "各等级数值、装备来源与配装关系",
  },
  {
    topic: "decorations",
    label: "装饰珠",
    status: "partial",
    tables: ["decorations"],
    available: "名称、物品 ID 与孔位",
    missing: "技能组成、掉落来源与概率",
  },
  {
    topic: "charms",
    label: "护石",
    status: "missing",
    tables: [],
    available: "无；现有 pendants 表是武器挂件，不是护石",
    missing: "护石名称、等级、技能、升级路线与素材",
  },
  {
    topic: "items",
    label: "物品",
    status: "partial",
    tables: ["items"],
    available: "名称、类型、稀有度与说明",
    missing: "获取来源、用途关系与合成配方",
  },
  {
    topic: "crafting",
    label: "制作与强化素材",
    status: "missing",
    tables: [],
    available: "无结构化关系",
    missing: "装备制作、升级与素材获取关系",
  },
  {
    topic: "monsters",
    label: "怪物",
    status: "partial",
    tables: ["monsters"],
    available: "名称、游戏 ID 与代码",
    missing: "弱点、肉质、异常耐性、招式与生态信息",
  },
  {
    topic: "monsterDrops",
    label: "怪物素材与掉落",
    status: "missing",
    tables: [],
    available: "无结构化关系",
    missing: "部位破坏、剥取、任务奖励与概率",
  },
  {
    topic: "quests",
    label: "任务",
    status: "partial",
    tables: ["quests", "deliveries", "login_bonus"],
    available: "任务名称、目标、失败条件及部分交货任务",
    missing: "前置条件、解锁链、地点、怪物、奖励与素材",
  },
  {
    topic: "stages",
    label: "地图与场景",
    status: "partial",
    tables: ["stages"],
    available: "场景 ID 与名称",
    missing: "区域、采集点、营地和环境关系",
  },
  {
    topic: "specialEquipment",
    label: "特殊装备",
    status: "partial",
    tables: ["special_equipment"],
    available: "名称、代码与说明",
    missing: "解锁条件、持续时间和冷却数据",
  },
  {
    topic: "palicoEquipment",
    label: "随从装备",
    status: "partial",
    tables: ["palico_weapons", "palico_armor"],
    available: "名称与模型路径",
    missing: "属性、技能与制作素材",
  },
  {
    topic: "kinsects",
    label: "猎虫",
    status: "partial",
    tables: ["kinsects"],
    available: "名称、ID 与模型路径",
    missing: "属性、粉尘和强化路线",
  },
  {
    topic: "canteen",
    label: "猫饭与食材",
    status: "partial",
    tables: ["canteen_skills", "ingredients"],
    available: "猫饭技能、食材名称与说明",
    missing: "食材解锁条件、组合规则与触发概率",
  },
  {
    topic: "otherTerminology",
    label: "其他游戏术语",
    status: "partial",
    tables: ["npc", "poogie", "achievements", "melodies", "endemic_life", "gallery"],
    available: "若干名称、说明、代码或模型路径",
    missing: "跨表关系与完整攻略语义",
  },
];

function parseArguments(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith("--")) continue;
    const [rawKey, inlineValue] = argument.slice(2).split("=", 2);
    if (inlineValue !== undefined) {
      values[rawKey] = inlineValue;
      continue;
    }
    const next = argv[index + 1];
    if (next && !next.startsWith("--")) {
      values[rawKey] = next;
      index += 1;
    } else {
      values[rawKey] = true;
    }
  }
  return values;
}

async function pathExists(targetPath) {
  try {
    await stat(targetPath);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, "utf8"));
}

function incrementCount(map, key, amount = 1) {
  map.set(key, (map.get(key) ?? 0) + amount);
}

function normalizeRelativePath(relativePath) {
  return relativePath.replaceAll("\\", "/").replace(/^\.\//, "");
}

function stripDeploymentRoot(relativePath) {
  const normalized = normalizeRelativePath(relativePath);
  return normalized.replace(/^nativepc\//i, "");
}

function extensionOf(relativePath) {
  const basename = path.posix.basename(normalizeRelativePath(relativePath));
  const extension = path.posix.extname(basename).toLowerCase();
  return extension || "[无扩展名]";
}

function sortedCounts(map, keyName = "value") {
  return [...map.entries()]
    .map(([key, count]) => ({ [keyName]: key, count }))
    .sort((left, right) => right.count - left.count || String(left[keyName]).localeCompare(String(right[keyName]), "zh-CN"));
}

function addAggregateEntry(map, key, modIndex, size) {
  const current = map.get(key) ?? { fileCount: 0, totalBytes: 0, modIndexes: new Set() };
  current.fileCount += 1;
  current.totalBytes += size;
  current.modIndexes.add(modIndex);
  map.set(key, current);
}

function serializeAggregate(map, keyName) {
  return [...map.entries()]
    .map(([key, value]) => ({
      [keyName]: key,
      fileCount: value.fileCount,
      modCount: value.modIndexes.size,
      totalBytes: value.totalBytes,
    }))
    .sort((left, right) => right.fileCount - left.fileCount || String(left[keyName]).localeCompare(String(right[keyName]), "zh-CN"));
}

async function collectFiles(rootPath, relativeRoot = "") {
  const files = [];
  let unreadableEntryCount = 0;
  const directories = [{ absolutePath: rootPath, relativePath: relativeRoot }];

  while (directories.length > 0) {
    const current = directories.pop();
    let entries;
    try {
      entries = await readdir(current.absolutePath, { withFileTypes: true });
    } catch {
      unreadableEntryCount += 1;
      continue;
    }

    for (const entry of entries) {
      const absolutePath = path.join(current.absolutePath, entry.name);
      const relativePath = normalizeRelativePath(path.join(current.relativePath, entry.name));
      if (entry.isSymbolicLink()) continue;
      if (entry.isDirectory()) {
        directories.push({ absolutePath, relativePath });
        continue;
      }
      if (!entry.isFile()) continue;
      try {
        const metadata = await lstat(absolutePath);
        files.push({ absolutePath, relativePath, size: metadata.size });
      } catch {
        unreadableEntryCount += 1;
      }
    }
  }

  return { files, unreadableEntryCount };
}

async function findModLibrary(explicitRoot) {
  if (explicitRoot) {
    const resolved = path.resolve(explicitRoot);
    if (!(await pathExists(resolved))) {
      throw new Error(`指定的 MOD 库目录不存在: ${resolved}`);
    }
    return resolved;
  }

  const candidates = [
    path.join(projectRoot, "src-tauri/target/debug/AcumodData/mods/installed"),
    path.join(projectRoot, "src-tauri/target/release/AcumodData/mods/installed"),
  ];
  for (const candidate of candidates) {
    if (await pathExists(candidate)) return candidate;
  }
  return null;
}

async function auditRawPackage() {
  if (!(await pathExists(rawManifestPath))) {
    return auditMhworldDataFallbackInput();
  }
  const manifest = await readJson(rawManifestPath);
  const formatTotals = {};
  for (const format of ["csv", "jsonl"]) {
    let fileCount = 0;
    let totalBytes = 0;
    for (const sheet of manifest.sheets) {
      const relativePath = sheet[format];
      if (!relativePath) continue;
      const metadata = await stat(path.join(rawPackageRoot, relativePath));
      fileCount += 1;
      totalBytes += metadata.size;
    }
    formatTotals[format] = { fileCount, totalBytes };
  }

  const sqlitePath = path.join(rawPackageRoot, "mhwi_data.sqlite");
  const sqliteMetadata = await stat(sqlitePath);
  formatTotals.sqlite = { fileCount: 1, totalBytes: sqliteMetadata.size };

  return {
    packageName: manifest.name,
    sourceFileName: manifest.source_file_name,
    sourceSha256: manifest.source_sha256,
    gameVersion: "15.10.00",
    sheetCount: manifest.sheet_count,
    totalDataRows: manifest.total_data_rows,
    formats: formatTotals,
    tables: manifest.sheets.map((sheet) => ({
      index: sheet.index,
      sheetTitle: sheet.sheet_title,
      tableName: sheet.table_name,
      rowCount: sheet.row_count,
      columnCount: sheet.column_count,
      columns: sheet.columns,
    })),
    sourcePage: "http://www.mhwmod.com/archives/660",
    redistribution: "requiresPermission",
  };
}

async function auditMhworldDataFallbackInput() {
  if (!(await pathExists(mhworldDataSnapshotPath))) {
    throw new Error(
      "本地 15.10.00 原始表缺失，且未找到 MHWData 开发快照。请先运行 npm.cmd run knowledge:fetch-mhworlddata。",
    );
  }
  const [snapshot, metadata] = await Promise.all([
    readJson(mhworldDataSnapshotPath),
    stat(mhworldDataSnapshotPath),
  ]);
  if (
    snapshot.schemaVersion !== 1
    || snapshot.sourceId !== "mhworlddata-armor-name-map"
    || snapshot.contentBaselineVersion !== "15.10.00"
    || !snapshot.tables
  ) {
    throw new Error("MHWData 开发快照结构或内容基线无效。");
  }
  const tableNameMap = {
    weaponBase: "weapons", armorBase: "armor", skillTranslations: "skills", decorationBase: "decorations",
    charmBase: "charms", itemTranslations: "items", weaponCrafting: "crafting", armorCrafting: "crafting",
    charmCrafting: "crafting", monsterBase: "monsters", monsterRewards: "monster_drops", questBase: "quests",
    questMonsters: "quests", questRewards: "quests", locationBase: "stages", locationItems: "stages",
  };
  const tables = Object.entries(snapshot.tables).map(([sourceTable, table]) => ({
    index: sourceTable,
    sheetTitle: sourceTable,
    tableName: tableNameMap[sourceTable] ?? sourceTable,
    rowCount: Array.isArray(table.rows) ? table.rows.length : 0,
    columnCount: Array.isArray(table.rows) && table.rows[0] ? Object.keys(table.rows[0]).length : 0,
    columns: Array.isArray(table.rows) && table.rows[0] ? Object.keys(table.rows[0]) : [],
  }));
  return {
    inputProfile: "mhworlddata-direct",
    packageName: "MHWData fixed-commit local development snapshot",
    sourceFileName: path.basename(mhworldDataSnapshotPath),
    sourceSha256: null,
    gameVersion: snapshot.contentBaselineVersion,
    sheetCount: tables.length,
    totalDataRows: tables.reduce((total, table) => total + table.rowCount, 0),
    formats: {
      csv: { fileCount: 0, totalBytes: 0 },
      jsonl: { fileCount: 0, totalBytes: 0 },
      sqlite: { fileCount: 0, totalBytes: 0 },
      snapshot: { fileCount: 1, totalBytes: metadata.size },
    },
    tables,
    sourcePage: "https://github.com/gatheringhallstudios/MHWorldData",
    redistribution: "snapshotIgnored-derivedFieldsRequireReleaseAudit",
  };
}

async function auditCuratedData() {
  const entries = await readdir(curatedRoot, { withFileTypes: true });
  const results = [];
  for (const entry of entries) {
    if (!entry.isFile() || path.extname(entry.name).toLowerCase() !== ".json") continue;
    const filePath = path.join(curatedRoot, entry.name);
    const metadata = await stat(filePath);
    let jsonMetadata = {};
    try {
      const json = await readJson(filePath);
      jsonMetadata = {
        schemaVersion: json.schemaVersion ?? null,
        gameVersion: json.gameVersion ?? null,
        locale: json.locale ?? null,
      };
    } catch {
      jsonMetadata = { parseError: true };
    }
    results.push({ fileName: entry.name, totalBytes: metadata.size, ...jsonMetadata });
  }
  return results.sort((left, right) => left.fileName.localeCompare(right.fileName, "zh-CN"));
}

async function auditQuestUnlockCoverage() {
  return {
    available: false,
    // 不再读取或抓取第三方任务解锁页面，避免将其派生条件重新引入知识包。
    message: "第三方任务解锁资料已移除；固定 MHWData 仅提供任务基础、目标与报酬，解锁链作为明确资料缺口处理。",
  };
}

async function auditDeliveryUnlockCoverage() {
  if (!(await pathExists(deliveryUnlockSourcePath))) {
    return {
      available: false,
      message: "尚未登记交货委托解锁来源。",
    };
  }
  const source = await readJson(deliveryUnlockSourcePath);
  if (
    source.schemaVersion !== 1
    || source.sourceId !== "kuroyonhon-delivery-unlocks"
    || !Array.isArray(source.entries)
  ) {
    throw new Error("交货委托解锁来源结构无效。");
  }
  // 固定 MHWData 只提供任务基础/目标/报酬行，交货委托前置关系不再写入数值库。
  const developmentPack = null;
  return {
    available: true,
    sourceId: source.sourceId,
    sourceUrl: source.sourceUrl,
    retrievedAt: source.retrievedAt,
    sourceEntryCount: source.entries.length,
    developmentPack,
    message: "交货委托条件保留社区来源原文；只有本地任务 ID 唯一核验的项目才生成 requiresQuest。",
  };
}

async function auditDevelopmentFacts() {
  if (!(await pathExists(developmentMhwdataPath))) {
    return {
      available: false,
      message: "尚未生成 MHWData 开发数据库；运行 knowledge:build-dev 后可统计实体和原始行覆盖。",
    };
  }

  const database = new DatabaseSync(developmentMhwdataPath, { readOnly: true });
  try {
    const scalar = (sql) => Number(Object.values(database.prepare(sql).get() ?? {})[0] ?? 0);
    const grouped = (sql, key) => database.prepare(sql).all().map((row) => ({
      [key]: row[key],
      count: Number(row.count),
    }));
    return {
      available: true,
      packKind: "mhwdata",
      gameVersion: String(Object.values(database.prepare("SELECT runtime_game_version FROM mhwdata_manifest").get() ?? {})[0] ?? "unknown"),
      entityCount: scalar("SELECT COUNT(*) FROM entities"),
      relationCount: scalar("SELECT COUNT(*) FROM record_entities"),
      unverifiedEntityCount: 0,
      entityKinds: grouped("SELECT kind, COUNT(*) AS count FROM entities GROUP BY kind ORDER BY count DESC", "kind"),
      relationPredicates: grouped("SELECT section AS predicate, COUNT(*) AS count FROM records GROUP BY section ORDER BY count DESC", "predicate"),
    };
  } finally {
    database.close();
  }
}

async function auditModLibrary(modLibraryRoot) {
  if (!modLibraryRoot) {
    return {
      available: false,
      privacy: "未找到本地 MOD 库，仅生成游戏数据覆盖审计。",
    };
  }

  const directoryEntries = (await readdir(modLibraryRoot, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .sort((left, right) => left.name.localeCompare(right.name, "zh-CN"));
  const extensionAggregates = new Map();
  const levelOneAggregates = new Map();
  const levelTwoAggregates = new Map();
  const schemaVersionCounts = new Map();
  const deployRootCounts = new Map();
  const detectionMethodCounts = new Map();
  const enabledCounts = new Map();
  let validManifestCount = 0;
  let invalidManifestCount = 0;
  let manifestFileCount = 0;
  let actualFileCount = 0;
  let actualTotalBytes = 0;
  let missingContentFileCount = 0;
  let orphanContentFileCount = 0;
  let unreadableEntryCount = 0;

  for (let modIndex = 0; modIndex < directoryEntries.length; modIndex += 1) {
    const modRoot = path.join(modLibraryRoot, directoryEntries[modIndex].name);
    let manifest = null;
    try {
      manifest = await readJson(path.join(modRoot, "manifest.json"));
      validManifestCount += 1;
      incrementCount(schemaVersionCounts, String(manifest.schemaVersion ?? "unknown"));
      incrementCount(deployRootCounts, String(manifest.deployRoot ?? "unknown"));
      incrementCount(detectionMethodCounts, String(manifest.detectionMethod ?? "unknown"));
      incrementCount(enabledCounts, manifest.enabled === true ? "enabled" : "disabled");
    } catch {
      invalidManifestCount += 1;
    }

    const contentRoot = path.join(modRoot, "content");
    const contentAudit = (await pathExists(contentRoot))
      ? await collectFiles(contentRoot)
      : { files: [], unreadableEntryCount: 0 };
    unreadableEntryCount += contentAudit.unreadableEntryCount;
    const actualRelativePaths = new Set(contentAudit.files.map((file) => file.relativePath.toLowerCase()));
    const manifestRelativePaths = new Set(
      Array.isArray(manifest?.files)
        ? manifest.files
          .map((file) => file?.libraryRelativePath)
          .filter((value) => typeof value === "string")
          .map((value) => normalizeRelativePath(value).replace(/^content\//i, "").toLowerCase())
        : [],
    );
    manifestFileCount += manifestRelativePaths.size;
    for (const manifestPath of manifestRelativePaths) {
      if (!actualRelativePaths.has(manifestPath)) missingContentFileCount += 1;
    }
    for (const actualPath of actualRelativePaths) {
      if (!manifestRelativePaths.has(actualPath)) orphanContentFileCount += 1;
    }

    for (const file of contentAudit.files) {
      actualFileCount += 1;
      actualTotalBytes += file.size;
      const deploymentPath = stripDeploymentRoot(file.relativePath);
      const segments = deploymentPath.split("/").filter(Boolean);
      addAggregateEntry(extensionAggregates, extensionOf(deploymentPath), modIndex, file.size);
      addAggregateEntry(levelOneAggregates, segments[0]?.toLowerCase() ?? "[根目录]", modIndex, file.size);
      addAggregateEntry(
        levelTwoAggregates,
        segments.length >= 2 ? `${segments[0].toLowerCase()}/${segments[1].toLowerCase()}` : segments[0]?.toLowerCase() ?? "[根目录]",
        modIndex,
        file.size,
      );
    }
  }

  return {
    available: true,
    privacy: "仅输出匿名聚合统计，不包含 MOD 名称、ID、文件名或任何路径。",
    modDirectoryCount: directoryEntries.length,
    validManifestCount,
    invalidManifestCount,
    manifestSchemaVersions: sortedCounts(schemaVersionCounts, "schemaVersion"),
    enabledStates: sortedCounts(enabledCounts, "state"),
    deployRoots: sortedCounts(deployRootCounts, "deployRoot"),
    detectionMethods: sortedCounts(detectionMethodCounts, "detectionMethod"),
    manifestFileCount,
    actualFileCount,
    actualTotalBytes,
    missingContentFileCount,
    orphanContentFileCount,
    unreadableEntryCount,
    fileExtensions: serializeAggregate(extensionAggregates, "extension"),
    pathPrefixesLevelOne: serializeAggregate(levelOneAggregates, "prefix"),
    pathPrefixesLevelTwo: serializeAggregate(levelTwoAggregates, "prefix"),
  };
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return "-";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function escapeCell(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}

function renderReport(baseline, modProfile) {
  const lines = [
    "# AcuAI 知识数据基线审计",
    "",
    "> 此报告由 `npm.cmd run knowledge:audit` 生成。MOD 统计已经匿名化，不包含 MOD 名称、ID、文件名或绝对路径。",
    "",
    "## 版本结论",
    "",
    `- 知识库目标版本：\`${baseline.targetGameVersion}\`。`,
    `- 当前结构化游戏数据基线：\`${baseline.baseGameVersion}\`。`,
    "- 项目确认 15.10 是最后一次明显新增游戏内容和机制的更新；15.10.00 可作为 15.23 的内容事实基线。外部字段和原始数据再分发许可仍须单独核验。",
    "",
    "## 当前构建输入",
    "",
    "",
    "## 原始资料字段覆盖",
    "",
    "| 主题 | 状态 | 现有内容 | 主要缺口 |",
    "| --- | --- | --- | --- |",
    ...baseline.coverage.map((item) => `| ${escapeCell(item.label)} | ${item.status} | ${escapeCell(item.available)} | ${escapeCell(item.missing)} |`),
    "",
    "## 开发事实包实际内容",
    "",
  ];

  if (baseline.rawPackage.inputProfile === "mhworlddata-direct") {
    lines.splice(
      lines.indexOf("## 原始资料字段覆盖"),
      0,
      `- 输入模式：\`mhworlddata-direct\`。构建器直接使用固定 commit 的 MHWData 本地快照。`,
      `- 数据表：${baseline.rawPackage.sheetCount} 张，共 ${baseline.rawPackage.totalDataRows} 行；快照 ${formatBytes(baseline.rawPackage.formats.snapshot.totalBytes)}。`,
      "- 该输入只用于本地开发验收；字段、中文名称覆盖和再分发许可均仍需发布审计。",
      "",
    );
  } else {
    lines.splice(
      lines.indexOf("## 原始资料字段覆盖"),
      0,
      `- 数据表：${baseline.rawPackage.sheetCount} 张，共 ${baseline.rawPackage.totalDataRows} 行。`,
      `- CSV：${baseline.rawPackage.formats.csv.fileCount} 个，${formatBytes(baseline.rawPackage.formats.csv.totalBytes)}。`,
      `- JSONL：${baseline.rawPackage.formats.jsonl.fileCount} 个，${formatBytes(baseline.rawPackage.formats.jsonl.totalBytes)}。`,
      `- SQLite：${formatBytes(baseline.rawPackage.formats.sqlite.totalBytes)}。`,
      "- 原始数据只用于本地研究，重新分发前需要来源方许可。",
      "",
    );
  }

  if (!baseline.developmentFacts.available) {
    lines.push(baseline.developmentFacts.message, "");
  } else {
    lines.push(
      `- 包类型：\`${baseline.developmentFacts.packKind}\`；目标版本：\`${baseline.developmentFacts.gameVersion}\`。`,
      `- 实体：${baseline.developmentFacts.entityCount}；关系：${baseline.developmentFacts.relationCount}；标记为 \'unverified\' 的实体：${baseline.developmentFacts.unverifiedEntityCount}。`,
      "",
      "| 实体类型 | 数量 |",
      "| --- | ---: |",
      ...baseline.developmentFacts.entityKinds.map((item) => `| ${escapeCell(item.kind)} | ${item.count} |`),
      "",
      "| 关系类型 | 数量 |",
      "| --- | ---: |",
      ...baseline.developmentFacts.relationPredicates.map((item) => `| ${escapeCell(item.predicate)} | ${item.count} |`),
      "",
    );
  }

  lines.push(
    "## 任务链覆盖",
    "",
  );

  if (!baseline.questUnlocks.available) {
    lines.push(baseline.questUnlocks.message, "");
  } else {
    lines.push(
      `- 解锁来源快照：${baseline.questUnlocks.sourceEntryCount} 条任务，其中 ${baseline.questUnlocks.parsedSourceEntryCount} 条已结构化解析，${baseline.questUnlocks.unparsedSourceEntryCount} 条待补充规则。`,
      `- 人工名称桥接：${baseline.questUnlocks.curatedNameBridgeCount} 条，仅用于外部英文标题与本地稳定任务无法直接一致时的逐项核对。`,
      `- ${baseline.questUnlocks.message}`,
      "",
      "| 来源页 | 等级 | 条目 | 已结构化 |",
      "| --- | --- | ---: | ---: |",
      ...baseline.questUnlocks.sourcePages.map((page) => `| ${escapeCell(page.sourceId)} | ${escapeCell(page.rank)} | ${page.entryCount} | ${page.parsedCount} |`),
      "",
    );
    if (baseline.questUnlocks.developmentPack) {
      lines.push(
        `- 当前开发包最终写入：${baseline.questUnlocks.developmentPack.taskCountWithUnlockData} 个任务具有解锁资料，${baseline.questUnlocks.developmentPack.prerequisiteRelationCount} 条前置关系，${baseline.questUnlocks.developmentPack.unlockConditionCount} 条解锁条件。`,
        "",
      );
    }
  }

  lines.push(
    "## 交货委托解锁覆盖",
    "",
  );
  if (!baseline.deliveryUnlocks.available) {
    lines.push(baseline.deliveryUnlocks.message, "");
  } else {
    lines.push(
      `- 来源：\`${escapeCell(baseline.deliveryUnlocks.sourceId)}\`；来源页面：${baseline.deliveryUnlocks.sourceUrl}；抓取时间：${baseline.deliveryUnlocks.retrievedAt}。`,
      `- 登记条目：${baseline.deliveryUnlocks.sourceEntryCount} 条。${baseline.deliveryUnlocks.message}`,
    );
    if (baseline.deliveryUnlocks.developmentPack) {
      lines.push(
        `- 当前开发包最终写入：${baseline.deliveryUnlocks.developmentPack.conditionCount} 条解锁条件，${baseline.deliveryUnlocks.developmentPack.prerequisiteRelationCount} 条任务前置关系；来源条目中无法映射本地交货实体：${baseline.deliveryUnlocks.developmentPack.unresolvedDeliveryCount} 条。`,
      );
    }
    lines.push("");
  }

  lines.push(
    "## 本地 MOD 库概况",
    "",
  );

  if (!modProfile.available) {
    lines.push("未找到本地 MOD 库。本次未生成 MOD 文件分布。", "");
    return `${lines.join("\n")}\n`;
  }

  lines.push(
    `- MOD 目录：${modProfile.modDirectoryCount}。`,
    `- 有效 manifest：${modProfile.validManifestCount}；无效 manifest：${modProfile.invalidManifestCount}。`,
    `- 实际内容文件：${modProfile.actualFileCount}，共 ${formatBytes(modProfile.actualTotalBytes)}。`,
    `- manifest 记录文件：${modProfile.manifestFileCount}。`,
    `- manifest 有记录但本地缺失：${modProfile.missingContentFileCount}；本地存在但 manifest 未记录：${modProfile.orphanContentFileCount}。`,
    "",
    "### 主要文件类型",
    "",
    "| 扩展名 | 文件数 | 涉及 MOD 数 | 总体积 |",
    "| --- | ---: | ---: | ---: |",
    ...modProfile.fileExtensions.slice(0, 25).map((item) => `| ${escapeCell(item.extension)} | ${item.fileCount} | ${item.modCount} | ${formatBytes(item.totalBytes)} |`),
    "",
    "### 主要二级目录",
    "",
    "| 相对前缀 | 文件数 | 涉及 MOD 数 | 总体积 |",
    "| --- | ---: | ---: | ---: |",
    ...modProfile.pathPrefixesLevelTwo.slice(0, 25).map((item) => `| ${escapeCell(item.prefix)} | ${item.fileCount} | ${item.modCount} | ${formatBytes(item.totalBytes)} |`),
    "",
    "## 下一步",
    "",
    "1. 如需任务解锁、特别任务或交货委托条件，使用受控联网资料并明确标记其来源，不将第三方页面派生条件写回知识包。",
    "2. 按真实 MOD 文件分布继续补充低频格式，并为每条规则完成来源和复现实验记录。",
    "3. 持续登记外部原始数据、派生字段和繁简文本桥的来源与人工分发审核状态。",
    "4. 使用真实 DeepSeek V4 按 docs/knowledge-acceptance.md 完成人工验收。",
    "",
  );
  return lines.join("\n");
}

async function main() {
  const argumentsMap = parseArguments(process.argv.slice(2));
  const outputRoot = argumentsMap["output-dir"]
    ? path.resolve(String(argumentsMap["output-dir"]))
    : defaultOutputRoot;
  const modLibraryRoot = await findModLibrary(argumentsMap["mod-root"]);
  const [rawPackage, curatedData, questUnlocks, deliveryUnlocks, developmentFacts, modProfile] = await Promise.all([
    auditRawPackage(),
    auditCuratedData(),
    auditQuestUnlockCoverage(),
    auditDeliveryUnlockCoverage(),
    auditDevelopmentFacts(),
    auditModLibrary(modLibraryRoot),
  ]);
  const availableTables = new Set(rawPackage.tables.map((table) => table.tableName));
  const coverage = coverageDefinitions.map((definition) => ({
    ...definition,
    tablesPresent: definition.tables.filter((table) => availableTables.has(table)),
  }));
  if (rawPackage.inputProfile === "mhworlddata-direct") {
    const byTopic = new Map(coverage.map((item) => [item.topic, item]));
    Object.assign(byTopic.get("weapons"), {
      status: "partial", available: "名称、类型、攻击、属性、孔位、升级关系与大部分制作素材",
      missing: "斩味、模型路径、可精确复核的游戏内稳定 ID",
    });
    Object.assign(byTopic.get("armor"), {
      status: "partial", available: "名称、部位、防御、耐性、孔位、技能和制作素材",
      missing: "幻化 ID、模型路径和部分中文名称",
    });
    Object.assign(byTopic.get("charms"), {
      status: "partial", available: "护石名称、技能、升级关系和制作素材",
      missing: "少量中文名称和游戏内稳定 ID",
    });
    Object.assign(byTopic.get("crafting"), {
      status: "partial", available: "武器、防具和护石制作/升级素材关系",
      missing: "道具合成、全部解锁条件与来源路线",
    });
    Object.assign(byTopic.get("monsters"), {
      status: "partial", available: "名称、生态摘要、弱点、肉质、陷阱与任务目标关系",
      missing: "招式、状态阈值和完整特殊形态说明",
    });
    Object.assign(byTopic.get("monsterDrops"), {
      status: "partial", available: "怪物奖励/剥取条目、数量和概率",
      missing: "部位破坏条件、调查任务与完整掉落语义",
    });
    Object.assign(byTopic.get("quests"), {
      status: "partial", available: "任务名称、目标、地点、怪物与奖励",
      missing: "任务前置、活动开放、特别任务、交货委托与同名或近似标题的解锁链",
    });
    Object.assign(byTopic.get("stages"), {
      status: "partial", available: "地图、16 个稳定场景映射、任务地点和采集关系",
      missing: "完整区域、营地解锁和环境机制",
    });
  }
  const baseline = {
    schemaVersion: 1,
    baseGameVersion: "15.10.00",
    targetGameVersion: "15.23",
    versionAudit: {
      status: "contentBaselineConfirmed",
      requirement: "项目确认 15.10 是最后一次明显新增游戏内容和机制的更新；发布前仍须完成字段、许可和抽样事实核验。",
    },
    rawPackage,
    curatedData,
    questUnlocks,
    deliveryUnlocks,
    developmentFacts,
    coverage,
  };

  await mkdir(outputRoot, { recursive: true });
  await Promise.all([
    writeFile(path.join(outputRoot, "mhwi-data-baseline.json"), `${JSON.stringify(baseline, null, 2)}\n`, "utf8"),
    writeFile(path.join(outputRoot, "mod-library-profile.json"), `${JSON.stringify(modProfile, null, 2)}\n`, "utf8"),
    writeFile(path.join(outputRoot, "baseline-report.md"), renderReport(baseline, modProfile), "utf8"),
  ]);

  console.log(`知识数据基线已生成: ${path.relative(projectRoot, outputRoot)}`);
  console.log(`目标版本 ${baseline.targetGameVersion}；当前数据基线 ${baseline.baseGameVersion}`);
  if (modProfile.available) {
    console.log(`已匿名审计 ${modProfile.modDirectoryCount} 个 MOD 目录、${modProfile.actualFileCount} 个内容文件。`);
  } else {
    console.log("未找到本地 MOD 库，已跳过 MOD 文件分布审计。");
  }
}

await main();
