import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const sourceRoot = path.resolve(
  process.argv[2] ?? path.join(projectRoot, "src-tauri/target/analysis/MHW-Editor-source/Assets"),
);
const outputPath = path.join(
  projectRoot,
  "references/knowledge/raw/mhw-editor/game-text-bridge.json",
);

if (!fs.existsSync(sourceRoot)) {
  throw new Error(`找不到 MHW-Editor Assets 目录：${sourceRoot}`);
}

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

const names = new Map();
const ambiguous = new Set();
const englishNames = new Map();
const ambiguousEnglishNames = new Set();
for (const relativePath of findSimplifiedTextFiles(sourceRoot)) {
  const simplifiedPath = path.join(sourceRoot, relativePath);
  const traditionalPath = simplifiedPath.replace(`${path.sep}chS_`, `${path.sep}chT_`);
  const englishPath = simplifiedPath.replace(`${path.sep}chS_`, `${path.sep}eng_`);
  if (!fs.existsSync(traditionalPath)) {
    continue;
  }
  const simplified = JSON.parse(fs.readFileSync(simplifiedPath, "utf8"));
  const traditional = JSON.parse(fs.readFileSync(traditionalPath, "utf8"));
  const english = fs.existsSync(englishPath)
    ? JSON.parse(fs.readFileSync(englishPath, "utf8"))
    : {};
  for (const [key, nameZhHans] of Object.entries(simplified)) {
    const nameZhHant = traditional[key];
    if (
      typeof nameZhHans !== "string"
      || !nameZhHans.trim()
      || typeof nameZhHant !== "string"
      || !nameZhHant.trim()
    ) {
      continue;
    }
    const normalizedHans = nameZhHans.trim();
    const normalizedHant = nameZhHant.trim();
    const existing = names.get(normalizedHans);
    if (existing && existing !== normalizedHant) {
      names.delete(normalizedHans);
      ambiguous.add(normalizedHans);
      continue;
    }
    if (!ambiguous.has(normalizedHans)) {
      names.set(normalizedHans, normalizedHant);
    }
    const nameEn = english[key];
    if (typeof nameEn !== "string" || !nameEn.trim()) {
      continue;
    }
    const normalizedEnglish = nameEn.trim();
    const existingEnglish = englishNames.get(normalizedHans);
    if (existingEnglish && existingEnglish !== normalizedEnglish) {
      englishNames.delete(normalizedHans);
      ambiguousEnglishNames.add(normalizedHans);
      continue;
    }
    if (!ambiguousEnglishNames.has(normalizedHans)) {
      englishNames.set(normalizedHans, normalizedEnglish);
    }
  }
}

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify({
  schemaVersion: 1,
  sourceId: "mhw-editor-game-text",
  gameVersion: "15.10.00",
  source: {
    title: "Synthlight/MHW-Editor paired chS/chT game text",
    url: "https://github.com/Synthlight/MHW-Editor",
    method: "按同一游戏文本键配对，不进行简繁字形转换",
  },
  names: Object.fromEntries([...names].sort(([left], [right]) => left.localeCompare(right, "zh-Hans-CN"))),
  excludedAmbiguousNames: [...ambiguous].sort((left, right) => left.localeCompare(right, "zh-Hans-CN")),
  englishNames: Object.fromEntries([...englishNames].sort(([left], [right]) => left.localeCompare(right, "zh-Hans-CN"))),
  excludedAmbiguousEnglishNames: [...ambiguousEnglishNames].sort((left, right) => left.localeCompare(right, "zh-Hans-CN")),
}, null, 2)}\n`, "utf8");
console.log(`已生成 ${names.size} 条简繁和 ${englishNames.size} 条简英游戏文本桥，分别排除 ${ambiguous.size} 和 ${ambiguousEnglishNames.size} 条歧义文本：${path.relative(projectRoot, outputPath)}`);
