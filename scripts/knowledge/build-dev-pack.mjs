import { mkdir, readFile, rm } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const sourceCatalogPath = path.join(projectRoot, "references/knowledge/sources/catalog.json");
const documentInputs = [
  {
    sourcePath: path.join(projectRoot, "references/knowledge/sources/modding-documents.json"),
    outputName: "acumod-dev-modding.acukb",
    packId: "acumod-dev-modding",
    displayName: "AcuAI MOD 技术开发包",
    kind: "mhw-modding",
    version: "0.3.0-dev",
    description: "项目已验证技术规则的开发包，用于回答 MOD 技术问题。",
  },
  {
    sourcePath: path.join(projectRoot, "references/knowledge/sources/game-guide-documents.json"),
    outputName: "acumod-dev-game-guides.acukb",
    packId: "acumod-dev-game-guides",
    displayName: "AcuAI 游戏攻略开发包",
    kind: "mhw-game-guides",
    version: "0.3.0-dev",
    description: "带来源与适用条件的游戏攻略文本；精确数值必须再查询 MHWData。",
  },
  {
    sourcePath: path.join(projectRoot, "references/knowledge/sources/acumod-help-documents.json"),
    outputName: "acumod-dev-acumod-help.acukb",
    packId: "acumod-dev-acumod-help",
    displayName: "AcuAI Acumod 使用说明开发包",
    kind: "acumod-help",
    version: "0.3.0-dev",
    description: "Acumod 传统管理器与 AcuAI 的使用说明，不是游戏数值来源。",
  },
];
const targetGameVersion = "15.23";
const packApplicationId = 0x4143554B; // "ACUK"

function outputDirectory(argv) {
  const outputIndex = argv.findIndex((argument) => argument === "--output");
  const inline = argv.find((argument) => argument.startsWith("--output="));
  const value = outputIndex >= 0 ? argv[outputIndex + 1] : inline?.slice("--output=".length);
  return value ? path.dirname(path.resolve(value)) : path.join(projectRoot, "references/knowledge/build");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function validateDocuments(store, sourceIds, label) {
  assert(store?.schemaVersion === 1 && Array.isArray(store.documents), `${label}结构无效。`);
  const ids = new Set();
  for (const document of store.documents) {
    for (const field of ["id", "domain", "title", "body", "gameVersion", "sourceId"]) {
      assert(typeof document[field] === "string" && document[field].trim(), `${label}缺少字段 ${field}。`);
    }
    assert(!ids.has(document.id), `${label}文档 ID 重复：${document.id}`);
    assert(sourceIds.has(document.sourceId), `${label}引用了未登记来源：${document.sourceId}`);
    assert(typeof document.confidence === "number" && document.confidence >= 0 && document.confidence <= 1, `${label}置信度无效：${document.id}`);
    ids.add(document.id);
  }
  return store.documents;
}

async function buildDocumentPack(outputPath, catalog, documents, definition) {
  await mkdir(path.dirname(outputPath), { recursive: true });
  await rm(outputPath, { force: true });
  const database = new DatabaseSync(outputPath);
  database.exec(`
    PRAGMA application_id = ${packApplicationId};
    PRAGMA user_version = 1;
    PRAGMA foreign_keys = ON;
    CREATE TABLE pack_manifest (
      pack_id TEXT PRIMARY KEY, display_name TEXT NOT NULL, kind TEXT NOT NULL,
      version TEXT NOT NULL, game_version TEXT NOT NULL, locale TEXT NOT NULL,
      min_app_version TEXT NOT NULL, description TEXT NOT NULL
    );
    CREATE TABLE sources (
      id TEXT PRIMARY KEY, title TEXT NOT NULL, url TEXT, kind TEXT NOT NULL,
      game_version TEXT NOT NULL, license_note TEXT NOT NULL
    );
    CREATE TABLE entities (
      id TEXT PRIMARY KEY, kind TEXT NOT NULL, domain TEXT NOT NULL,
      canonical_name TEXT NOT NULL, name_zh_hans TEXT, name_zh_hant TEXT,
      summary TEXT NOT NULL, game_version TEXT NOT NULL, confidence REAL NOT NULL,
      source_id TEXT, data_json TEXT NOT NULL, FOREIGN KEY (source_id) REFERENCES sources(id)
    );
    CREATE TABLE aliases (
      entity_id TEXT NOT NULL, locale TEXT NOT NULL, alias TEXT NOT NULL,
      PRIMARY KEY (entity_id, locale, alias), FOREIGN KEY (entity_id) REFERENCES entities(id)
    );
    CREATE TABLE relations (
      id TEXT PRIMARY KEY, subject_id TEXT NOT NULL, predicate TEXT NOT NULL,
      object_id TEXT NOT NULL, game_version TEXT NOT NULL, confidence REAL NOT NULL,
      source_id TEXT, data_json TEXT NOT NULL, FOREIGN KEY (source_id) REFERENCES sources(id)
    );
    CREATE TABLE documents (
      id TEXT PRIMARY KEY, namespace TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
      game_version TEXT NOT NULL, confidence REAL NOT NULL, source_id TEXT,
      FOREIGN KEY (source_id) REFERENCES sources(id)
    );
    CREATE VIRTUAL TABLE knowledge_fts USING fts5(
      result_id UNINDEXED, result_kind UNINDEXED, domain UNINDEXED, title, body, tokenize='trigram'
    );
    CREATE INDEX aliases_alias_index ON aliases(alias);
    CREATE INDEX entities_kind_index ON entities(kind);
    CREATE INDEX relations_subject_index ON relations(subject_id, predicate);
    CREATE INDEX relations_object_index ON relations(object_id, predicate);
  `);
  database.exec("BEGIN IMMEDIATE");
  try {
    database.prepare("INSERT INTO pack_manifest VALUES (?, ?, ?, ?, ?, ?, ?, ?)").run(
      definition.packId, definition.displayName, definition.kind, definition.version,
      targetGameVersion, "zh-Hans", "0.1.0", definition.description,
    );
    const usedSourceIds = new Set(documents.map((document) => document.sourceId));
    const insertSource = database.prepare("INSERT INTO sources VALUES (?, ?, ?, ?, ?, ?)");
    for (const source of catalog.sources.filter((item) => usedSourceIds.has(item.id))) {
      insertSource.run(
        source.id, source.title ?? source.id, source.url ?? null, source.kind, source.gameVersion,
        [`用途：${source.usage}`, `分发：${source.redistribution}`, `许可状态：${source.licenseStatus}`, ...(source.notes ?? [])].join("；"),
      );
    }
    const insertDocument = database.prepare("INSERT INTO documents VALUES (?, ?, ?, ?, ?, ?, ?)");
    const insertFts = database.prepare("INSERT INTO knowledge_fts VALUES (?, ?, ?, ?, ?)");
    for (const document of documents) {
      insertDocument.run(document.id, document.domain, document.title, document.body, document.gameVersion, document.confidence, document.sourceId);
      insertFts.run(document.id, "document", document.domain, document.title, document.body);
    }
    database.exec("COMMIT; VACUUM");
    const integrity = database.prepare("PRAGMA integrity_check(1)").get();
    assert(integrity.integrity_check === "ok", `${definition.displayName}完整性检查失败：${integrity.integrity_check}`);
  } catch (error) {
    try { database.exec("ROLLBACK"); } catch { /* 已提交的事务无需回滚。 */ }
    throw error;
  } finally {
    database.close();
  }
  console.log(`${definition.displayName}已生成：${path.relative(projectRoot, outputPath)}，文档 ${documents.length} 篇`);
}

const argv = process.argv.slice(2);
const catalog = JSON.parse(await readFile(sourceCatalogPath, "utf8"));
assert(Array.isArray(catalog.sources) && catalog.sources.length > 0, "知识来源目录不能为空。 ");
const sourceIds = new Set(catalog.sources.map((source) => source.id));
const selected = argv.includes("--modding-only") ? documentInputs.slice(0, 1) : documentInputs;
const outputRoot = outputDirectory(argv);
for (const definition of selected) {
  const documents = validateDocuments(
    JSON.parse(await readFile(definition.sourcePath, "utf8")),
    sourceIds,
    definition.displayName,
  );
  await buildDocumentPack(path.join(outputRoot, definition.outputName), catalog, documents, definition);
}
