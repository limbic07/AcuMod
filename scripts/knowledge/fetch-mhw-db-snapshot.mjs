import { createHash } from "node:crypto";
import { mkdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const outputDirectory = path.join(projectRoot, "references/knowledge/raw/mhw-db");
const outputPath = path.join(outputDirectory, "current.json");
const endpoints = ["items", "skills", "armor"];
const armorIdMapUrl = "https://raw.githubusercontent.com/wiki/Ezekial711/MonsterHunterWorldModding/Armor-IDs.md";

async function fetchJson(endpoint) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 30_000);
  try {
    const response = await fetch(`https://mhw-db.com/${endpoint}`, {
      headers: { "User-Agent": "Acumod knowledge source audit" },
      signal: controller.signal,
    });
    if (!response.ok) {
      throw new Error(`${endpoint} 返回 HTTP ${response.status}`);
    }
    const data = await response.json();
    if (!Array.isArray(data)) {
      throw new Error(`${endpoint} 未返回数组数据`);
    }
    return data;
  } finally {
    clearTimeout(timeout);
  }
}

async function fetchText(url) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 30_000);
  try {
    const response = await fetch(url, {
      headers: { "User-Agent": "Acumod knowledge source audit" },
      signal: controller.signal,
    });
    if (!response.ok) {
      throw new Error(`防具模型映射返回 HTTP ${response.status}`);
    }
    return response.text();
  } finally {
    clearTimeout(timeout);
  }
}

const resources = {};
for (const endpoint of endpoints) {
  resources[endpoint] = await fetchJson(endpoint);
}
const armorIdMapMarkdown = await fetchText(armorIdMapUrl);
const serializedResources = JSON.stringify(resources);
const snapshot = {
  schemaVersion: 1,
  sourceId: "mhw-db-live-snapshot",
  sourceUrl: "https://mhw-db.com/",
  retrievedAt: new Date().toISOString(),
  // 该 API 没有提供可机器校验的游戏版本标识，构建阶段必须保留这个限制。
  gameVersion: "unverified",
  resources,
  armorIdMapUrl,
  armorIdMapMarkdown,
  contentSha256: createHash("sha256").update(serializedResources).digest("hex"),
};

await mkdir(outputDirectory, { recursive: true });
const temporaryPath = `${outputPath}.tmp`;
await writeFile(temporaryPath, `${JSON.stringify(snapshot, null, 2)}\n`, "utf8");
await rename(temporaryPath, outputPath);
console.log(`已写入 MHW DB 本地快照：${path.relative(projectRoot, outputPath)}`);
console.log(`物品 ${resources.items.length}，技能 ${resources.skills.length}，防具 ${resources.armor.length}；版本状态：未核验。`);
