import { createHash } from "node:crypto";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const outputPath = path.join(
  projectRoot,
  "references/knowledge/raw/game8-quest-unlocks/current.json",
);
const sourceHtmlDirectory = process.env.ACUMOD_QUEST_HTML_DIR;

const sources = [
  {
    id: "game8-assigned-base",
    rank: "HR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/292425",
  },
  {
    id: "game8-assigned-iceborne",
    rank: "MR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/292419",
  },
  {
    id: "game8-optional-base",
    rank: "HR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/292426",
  },
  {
    id: "game8-optional-iceborne",
    rank: "MR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/296709",
    parser: "unlockRequirements",
  },
  {
    id: "game8-event-base",
    rank: "HR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/296738",
    parser: "eventList",
    questCategory: "event",
  },
  {
    id: "game8-event-iceborne",
    rank: "MR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/292420",
    parser: "eventList",
    questCategory: "event",
  },
  {
    id: "game8-arena",
    rank: "HR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/292416",
    parser: "arena",
    questCategory: "arena",
  },
  {
    id: "game8-challenge",
    rank: "HR",
    url: "https://game8.co/games/Monster-Hunter-World/archives/292417",
    parser: "challengeList",
    questCategory: "challenge",
  },
];

function decodeHtml(value) {
  return value
    .replace(/<br\s*\/?>/giu, " ")
    .replace(/<[^>]+>/gu, " ")
    .replace(/&quot;/giu, '"')
    .replace(/&#39;|&apos;/giu, "'")
    .replace(/&amp;/giu, "&")
    .replace(/&nbsp;/giu, " ")
    .replace(/\s+/gu, " ")
    .trim();
}

function tableCells(row) {
  // Game8 的部分旧表格省略了最后一个 </td>；以单元格或行结束作为边界，避免静默丢掉挑战任务。
  return [...row.matchAll(/<td\b[^>]*>([\s\S]*?)(?:<\/td>|(?=<td\b)|(?=<th\b)|(?=<\/tr>))/giu)].map((match) => match[1]);
}

function firstLinkText(cell) {
  const match = cell.match(/<a\b[^>]*>([\s\S]*?)<\/a>/iu);
  return match ? decodeHtml(match[1]) : null;
}

function linkTexts(cell) {
  return [...cell.matchAll(/<a\b[^>]*>([\s\S]*?)<\/a>/giu)]
    .map((match) => decodeHtml(match[1]))
    .filter(Boolean);
}

function headingText(html) {
  return decodeHtml(html.replace(/<[^>]+>/gu, ""));
}

function requirementRank(text) {
  const normalized = text.replace(/[()]/gu, " ").replace(/\s+/gu, " ").trim();
  const match = normalized.match(/(?:Reach\s+)?(HR|MR)\s*(\d+)(?:\s+or\s+higher)?/iu);
  return match ? { kind: "reachRank", rank: match[1].toUpperCase(), level: Number(match[2]) } : null;
}

function requirementEntries(text) {
  const normalizedText = text.replace(/[“”]/gu, '"');
  const requirements = [];
  if (/(?:Complete|Clear|Clearing)\b/iu.test(normalizedText)) {
    const quotedQuestNames = [...normalizedText.matchAll(/(["'])(.*?)\1/gu)];
    for (const questName of quotedQuestNames) {
      const value = (questName[2] ?? "").trim();
      if (value) {
        requirements.push({ kind: "completeQuest", questNameEn: value });
      }
    }
    // 引号内的任务名是来源页的精确名称；不要再用宽松正则把后续的“并解锁地区”等条件并入任务名。
    if (quotedQuestNames.length === 0) {
      const danglingQuotedName = normalizedText.match(/(?:Complete|Clear|Clearing)\s+["'](.+)$/iu);
      if (danglingQuotedName?.[1]?.trim()) {
        // Game8 的黑龙行缺少闭合引号；只恢复行尾的完整任务标题，不尝试切分任意自然语言。
        requirements.push({ kind: "completeQuest", questNameEn: danglingQuotedName[1].trim() });
      }
      for (const questName of normalizedText.matchAll(/(?:Complete|Clear|Clearing)\s+(?:the\s+)?(?:ques+t|assigned quest|optional quest|special assignment|mission)\s+([^.,;]+?)(?=[.,;]|$)/giu)) {
        const value = questName[1].replace(/["']/gu, "").trim();
        if (value) {
          requirements.push({ kind: "completeQuest", questNameEn: value });
        }
      }
    }
  }
  if (normalizedText === "The Best Kind of Quest") {
    requirements.push({ kind: "completeQuest", questNameEn: normalizedText });
  }
  for (const match of normalizedText.matchAll(/(?:Reach|Achieve)\s+(HR|MR)\s*(\d+)/giu)) {
    requirements.push({ kind: "reachRank", rank: match[1].toUpperCase(), level: Number(match[2]) });
  }
  for (const match of normalizedText.matchAll(/(?:HR|MR)\s*(?:rank\s*)?(?:above|or higher)\s*(\d+)/giu)) {
    requirements.push({ kind: "reachRank", rank: match[0].toUpperCase().startsWith("MR") ? "MR" : "HR", level: Number(match[1]) });
  }
  for (const match of normalizedText.matchAll(/Master Rank★\s*(\d+)\s+or higher/giu)) {
    requirements.push({ kind: "reachRank", rank: "MR", level: Number(match[1]) });
  }
  const monsterText = normalizedText.replace(/Discover and Hunt/giu, "Hunt");
  for (const match of monsterText.matchAll(/\b(Hunt|hunting|hunted|Discover|discovering|discovered|capture|captured)\s+(?:an?\s+)?(?:(?:high|master)\s+rank\s+)?([A-Za-z][A-Za-z' -]+?)(?=\s+(?:and|after)\b|[.,;]|$)/giu)) {
    const verb = match[1].toLowerCase();
    const monsterNameEn = match[2].trim();
    const kind = verb.startsWith("hunt")
      ? "huntMonster"
      : verb.startsWith("discover")
        ? "discoverMonster"
        : "captureMonster";
    requirements.push({ kind, monsterNameEn });
  }
  for (const match of normalizedText.matchAll(/(?:Speak|Talk)\s+(?:to|with)\s+(?:the\s+)?([^.,;]+?)(?=\s+after|[.,;]|$)/giu)) {
    requirements.push({ kind: "talkToNpc", npcNameEn: match[1].trim() });
  }
  for (const match of normalizedText.matchAll(/Discover a campsite in (?:the )?([^.,;]+?)(?=\s+then|[.,;]|$)/giu)) {
    requirements.push({ kind: "discoverCamp", locationNameEn: match[1].trim() });
  }
  for (const match of normalizedText.matchAll(/(?:first visit in|after discovering the)\s+(?:the )?([^.,;]+?)(?=\s+and|[.,;]|$)/giu)) {
    requirements.push({ kind: "discoverLocation", locationNameEn: match[1].trim() });
  }
  for (const match of normalizedText.matchAll(/(?:Increase|Maximize)\s+([^.,;]+?)'?s\s+research level(?:\s+to\s+(\d+))?/giu)) {
    requirements.push({ kind: "researchLevel", monsterNameEn: match[1].trim(), level: match[2] ? Number(match[2]) : null });
  }
  if (/Requires the Iceborne Expansion/iu.test(normalizedText)) {
    requirements.push({ kind: "requiresExpansion", expansion: "Iceborne" });
  }
  if (/Clear all M★1 to M★5 optional quests/iu.test(normalizedText)) {
    requirements.push({ kind: "completeOptionalRange", rank: "MR", from: 1, to: 5 });
  }
  const namedStoryMatch = normalizedText.match(/Complete the story of ([A-Za-z ]+?)(?=\s+and\b|[.,;]|$)/iu);
  if (namedStoryMatch) {
    requirements.push({ kind: "completeStory", storyNameEn: namedStoryMatch[1].trim() });
  } else if (/Complete (?:the )?(?:Base Game's story mode|Main Storyline)/iu.test(normalizedText)) {
    requirements.push({ kind: "completeStory", storyNameEn: null });
  }
  for (const match of normalizedText.matchAll(/Appears after the\s+(.+?)\s+operation/giu)) {
    requirements.push({ kind: "completeStoryOperation", operationNameEn: match[1].trim() });
  }
  if (/Available from the Event start time/iu.test(normalizedText)) {
    requirements.push({ kind: "eventAvailability" });
  }
  if (/Cultural Exchange: Hoarfrost Reach bounty chain/iu.test(normalizedText)) {
    requirements.push({ kind: "completeBountyChain", chainNameEn: "Cultural Exchange: Hoarfrost Reach" });
  }
  for (const match of normalizedText.matchAll(/(?:Randomly appears after discovering|After your first visit in)\s+(?:the )?["']?([^.,;"']+)["']?/giu)) {
    const locationNameEn = match[1].trim();
    if (locationNameEn) {
      requirements.push({ kind: "discoverLocation", locationNameEn });
    }
  }
  for (const match of normalizedText.matchAll(/Gain access to\s+(\d+)\s+star rank quests/giu)) {
    requirements.push({ kind: "unlockRank", rank: "HR", level: Number(match[1]) });
  }
  if (/Available from the start/iu.test(normalizedText)) {
    requirements.push({ kind: "availableFromStart" });
  }
  for (const match of normalizedText.matchAll(/Return to ([A-Za-z' -]+?)\s+after/giu)) {
    requirements.push({ kind: "returnToLocation", locationNameEn: match[1].trim() });
  }
  const guidingLandsRegions = normalizedText.match(/Unlock the ([A-Za-z ]+?) regions? of the Guiding Lands/iu);
  if (guidingLandsRegions) {
    requirements.push({
      kind: "unlockGuidingLandsRegions",
      regions: guidingLandsRegions[1].split(/\s+and\s+/iu).map((region) => region.trim()).filter(Boolean),
    });
  }
  const seen = new Set();
  return requirements.filter((requirement) => {
    const key = JSON.stringify(requirement);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function parseUnlockRequirementRows(html, source) {
  const entries = [];
  for (const rowMatch of html.matchAll(/<tr\b[^>]*>([\s\S]*?)<\/tr>/giu)) {
    const cells = tableCells(rowMatch[1]);
    if (cells.length < 2) continue;
    const questNameEn = firstLinkText(cells[0]);
    const details = decodeHtml(cells.at(-1) ?? "");
    const unlockMatch = details.match(/Unlock Requirements\s*:\s*(.+)$/iu);
    if (!questNameEn || !unlockMatch) continue;
    const unlockText = unlockMatch[1].trim();
    const requirements = requirementEntries(unlockText);
    const rankMatch = decodeHtml(cells[0]).match(/Reach\s+(HR|MR)\s*(\d+)/iu);
    entries.push({
      questNameEn,
      sourceId: source.id,
      sourceUrl: source.url,
      rank: rankMatch?.[1]?.toUpperCase() ?? source.rank,
      rankLevel: rankMatch ? Number(rankMatch[2]) : null,
      requirements,
      parseStatus: requirements.length > 0 ? "parsed" : "unparsed",
      ...(requirements.length === 0 ? { unparsedCondition: unlockText } : {}),
    });
  }
  return entries;
}

function parseEventRows(html, source) {
  const entries = [];
  for (const table of html.matchAll(/<table\b[^>]*>([\s\S]*?)<\/table>/giu)) {
    const tableHtml = table[1];
    const header = headingText(tableHtml.match(/<tr\b[^>]*>([\s\S]*?)<\/tr>/iu)?.[1] ?? "");
    if (!/Quest Name/iu.test(header) || !/Target/iu.test(header)) continue;
    for (const row of tableHtml.matchAll(/<tr\b[^>]*>([\s\S]*?)<\/tr>/giu)) {
      const cells = tableCells(row[1]);
      if (cells.length !== 2) continue;
      const questText = firstLinkText(cells[0]);
      if (!questText || /^Quest Name$/iu.test(questText)) continue;
      const questNameEn = questText.replace(/^(?:M)?[★*]\s*\d+\s*/iu, "").trim();
      if (!questNameEn) continue;
      const targetNames = linkTexts(cells[1]);
      entries.push({
        questNameEn,
        sourceId: source.id,
        sourceUrl: source.url,
        rank: source.rank,
        rankLevel: null,
        questCategory: source.questCategory,
        targetNamesEn: targetNames,
        requirements: [{ kind: "eventAvailability" }],
        parseStatus: "parsed",
      });
    }
  }
  return entries;
}

function tableValueForHeader(tableHtml, headerText) {
  const rows = [...tableHtml.matchAll(/<tr\b[^>]*>([\s\S]*?)<\/tr>/giu)];
  for (let index = 0; index < rows.length; index += 1) {
    if (!new RegExp(headerText, "iu").test(headingText(rows[index][1]))) continue;
    const cells = tableCells(rows[index][1]);
    if (cells.length > 0) return headingText(cells.at(-1));
    const nextCells = tableCells(rows[index + 1]?.[1] ?? "");
    if (nextCells.length > 0) return headingText(nextCells.at(-1));
  }
  return "";
}

function tableCellForHeader(tableHtml, headerText) {
  const rows = [...tableHtml.matchAll(/<tr\b[^>]*>([\s\S]*?)<\/tr>/giu)];
  for (let index = 0; index < rows.length; index += 1) {
    if (!new RegExp(headerText, "iu").test(headingText(rows[index][1]))) continue;
    const cells = tableCells(rows[index][1]);
    if (cells.length > 0) return cells.at(-1);
    const nextCells = tableCells(rows[index + 1]?.[1] ?? "");
    if (nextCells.length > 0) return nextCells.at(-1);
  }
  return "";
}

function parseArenaRows(html, source) {
  const entries = [];
  const pattern = /<h4\b[^>]*>([\s\S]*?)<\/h4>\s*<table\b[^>]*>([\s\S]*?)<\/table>/giu;
  for (const match of html.matchAll(pattern)) {
    const title = headingText(match[1]);
    if (!/^Arena (?:Master )?Quest\s+(?:Master Rank\s+)?\d+$/iu.test(title)) continue;
    const tableHtml = match[2];
    const unlockCell = tableCellForHeader(tableHtml, "Unlock Condition");
    const unlockText = headingText(unlockCell);
    const targetText = tableValueForHeader(tableHtml, "Target Monster");
    if (!unlockText || !targetText) continue;
    const linkedQuestName = firstLinkText(unlockCell)
      ?.replace(/^(?:M)?★\s*\d+\s*/iu, "")
      .trim();
    // 斗技场表的“Unlock Condition”直接链接对应前置任务，属于来源明确表达的前置关系。
    const requirement = linkedQuestName
      ? { kind: "completeQuest", questNameEn: linkedQuestName }
      : requirementEntries(unlockText)[0] ?? { kind: "sourceText", textEn: unlockText };
    entries.push({
      questNameEn: title,
      sourceId: source.id,
      sourceUrl: source.url,
      rank: /Master/iu.test(title) ? "MR" : source.rank,
      rankLevel: null,
      questCategory: source.questCategory,
      targetValidatedMatch: true,
      targetNamesEn: linkTexts(targetText).length > 0 ? linkTexts(targetText) : [targetText],
      requirements: [requirement],
      parseStatus: "parsed",
    });
  }
  return entries;
}

function parseChallengeRows(html, source) {
  const entries = [];
  for (const table of html.matchAll(/<table\b[^>]*>([\s\S]*?)<\/table>/giu)) {
    const tableHtml = table[1];
    const header = headingText(tableHtml.match(/<tr\b[^>]*>([\s\S]*?)<\/tr>/iu)?.[1] ?? "");
    if (!/Quest Name\/Rank Requirement/iu.test(header) || !/Target/iu.test(header)) continue;
    for (const row of tableHtml.matchAll(/<tr\b[^>]*>([\s\S]*?)<\/tr>/giu)) {
      // 挑战任务页有省略 </td> 的旧表格；直接从行中读取首个任务链接与等级文本。
      const questNameEn = firstLinkText(row[1]);
      if (!questNameEn || /^Quest Name/iu.test(questNameEn)) continue;
      const rank = requirementRank(headingText(row[1]));
      if (!rank) continue;
      const targetNamesEn = linkTexts(row[1]).filter((name) => name !== questNameEn);
      entries.push({
        questNameEn,
        sourceId: source.id,
        sourceUrl: source.url,
        rank: rank.rank,
        rankLevel: rank.level,
        questCategory: source.questCategory,
        targetNamesEn,
        requirements: [rank],
        parseStatus: "parsed",
      });
    }
  }
  return entries;
}

function parseRows(html, source) {
  switch (source.parser ?? "unlockRequirements") {
    case "eventList": return parseEventRows(html, source);
    case "arena": return parseArenaRows(html, source);
    case "challengeList": return parseChallengeRows(html, source);
    default: return parseUnlockRequirementRows(html, source);
  }
}

async function loadHtml(source) {
  if (sourceHtmlDirectory) {
    return readFile(path.join(sourceHtmlDirectory, `${source.id}.html`), "utf8");
  }
  const response = await fetch(source.url, {
    headers: {
      "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36",
      Accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
      "Accept-Language": "en-US,en;q=0.9",
    },
  });
  if (!response.ok) {
    throw new Error(`无法下载 ${source.id}：${response.status} ${response.statusText}`);
  }
  return response.text();
}

const pages = await Promise.all(sources.map(async (source) => {
  const html = await loadHtml(source);
  return {
    source,
    sha256: createHash("sha256").update(html).digest("hex"),
    entries: parseRows(html, source),
  };
}));

const snapshot = {
  schemaVersion: 1,
  sourceKind: "communityQuestUnlockGuide",
  retrievedAt: new Date().toISOString(),
  pages: pages.map(({ source, sha256 }) => ({ ...source, sha256 })),
  entries: pages.flatMap(({ entries }) => entries),
};

await mkdir(path.dirname(outputPath), { recursive: true });
const temporaryPath = `${outputPath}.tmp`;
await writeFile(temporaryPath, `${JSON.stringify(snapshot, null, 2)}\n`, "utf8");
await rename(temporaryPath, outputPath);

const parsedCount = snapshot.entries.filter((entry) => entry.parseStatus === "parsed").length;
console.log(`已保存任务解锁开发快照：${path.relative(projectRoot, outputPath)}`);
console.log(`任务条目 ${snapshot.entries.length}，可结构化解析 ${parsedCount}，待人工规则补充 ${snapshot.entries.length - parsedCount}`);
