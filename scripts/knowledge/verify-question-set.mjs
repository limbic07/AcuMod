import { readFile } from "node:fs/promises";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const questionSetPath = path.join(projectRoot, "references/knowledge/sources/question-set.json");
const packPaths = {
  gameFacts: path.join(projectRoot, "references/knowledge/build/acumod-dev-game-facts.acukb"),
  modding: path.join(projectRoot, "references/knowledge/build/acumod-dev-modding.acukb"),
  guides: path.join(projectRoot, "references/knowledge/build/acumod-dev-game-guides.acukb"),
};

function expect(value, message) {
  if (!value) throw new Error(`固定问题集验收失败：${message}`);
}

function containsName(database, value) {
  return database.prepare(
    "SELECT COUNT(*) AS count FROM entities WHERE canonical_name LIKE ?1 OR name_zh_hans LIKE ?1 OR name_zh_hant LIKE ?1",
  ).get(`%${value}%`).count > 0;
}

function verifyExpectation(database, expectation) {
  if (expectation.type === "entity") {
    return database.prepare("SELECT COUNT(*) AS count FROM entities WHERE id = ?1").get(expectation.id).count === 1;
  }
  if (expectation.type === "document") {
    return database.prepare("SELECT COUNT(*) AS count FROM documents WHERE id = ?1").get(expectation.id).count === 1;
  }
  if (expectation.type === "relation") {
    // 同一实体对可因难度、部位或地区保留多条条件不同的关系，固定问题集只要求存在可用证据。
    return database.prepare(
      "SELECT COUNT(*) AS count FROM relations WHERE subject_id = ?1 AND predicate = ?2 AND object_id = ?3",
    ).get(expectation.subjectId, expectation.predicate, expectation.objectId).count > 0;
  }
  if (expectation.type === "entityData") {
    const row = database.prepare("SELECT data_json FROM entities WHERE id = ?1").get(expectation.id);
    return row && JSON.parse(row.data_json)[expectation.field] === expectation.equals;
  }
  if (expectation.type === "entityDataArrayMinimum") {
    const row = database.prepare("SELECT data_json FROM entities WHERE id = ?1").get(expectation.id);
    const value = row ? JSON.parse(row.data_json)[expectation.field] : null;
    return Array.isArray(value) && value.length >= expectation.minimum;
  }
  if (expectation.type === "nameContains") {
    return containsName(database, expectation.value);
  }
  throw new Error(`固定问题集包含未知断言类型：${expectation.type}`);
}

const source = JSON.parse(await readFile(questionSetPath, "utf8"));
expect(source.schemaVersion === 1 && Array.isArray(source.questions), "问题集结构无效。");
const databases = new Map(Object.entries(packPaths).map(([key, filePath]) => [key, new DatabaseSync(filePath, { readOnly: true })]));
try {
  for (const question of source.questions) {
    expect(
      typeof question.id === "string"
        && typeof question.question === "string"
        && Array.isArray(question.expect)
        && Array.isArray(question.searchResultIds)
        && question.searchResultIds.length > 0
        && question.searchResultIds.every((id) => typeof id === "string" && id.length > 0),
      "问题缺少 ID、问句、断言或自然检索预期结果。",
    );
    for (const expectation of question.expect) {
      const database = databases.get(expectation.pack);
      expect(database, `${question.id} 使用了未知知识包 ${expectation.pack}。`);
      expect(verifyExpectation(database, expectation), `${question.id} 缺少 ${expectation.type} 证据。`);
    }
  }
} finally {
  for (const database of databases.values()) database.close();
}

console.log(`固定问题集验收通过：${source.questions.length} 类问题均具备所需知识证据。`);
