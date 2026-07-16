import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourcePath = path.join(
  projectRoot,
  "references/mhwi-data/curated/sources/armor-layered-menu-order-zh-hant.md",
);
const modelIndexPath = path.join(
  projectRoot,
  "references/mhwi-data/curated/model-index.json",
);
const traditionalTextPath = path.join(
  projectRoot,
  "references/mhwi-data/curated/game-text-zh-hant.json",
);
const outputPath = path.join(
  projectRoot,
  "references/mhwi-data/curated/armor-menu-order.json",
);

const sectionDefinitions = [
  { key: "lowHighRank", expectedCount: 116 },
  { key: "masterRank", expectedCount: 147 },
];

// These two recording labels differ from the official text index, but their model targets are
// unambiguous. Keeping the exception beside the generator makes the mismatch reviewable.
const manualMatches = new Map([
  [
    "lowHighRank:37",
    {
      expectedNameTraditional: "\u3010\u5de8\u7532\u87f2\u3011\u670d\u88dd",
      targetIds: ["armor:pl017_0000"],
    },
  ],
  [
    "masterRank:138",
    {
      expectedNameTraditional: "\u3010\u9ab7\u9acf\u982d\u5dfe\u3011\u670d\u88dd",
      targetIds: ["armor:pl099_0000"],
    },
  ],
]);

// The recordings did not show four unlock-dependent layered sets. Their layered IDs and official
// names are present in the 15.10.00 table, so insert them beside the adjacent recorded IDs.
const masterRankSupplements = [
  {
    afterSourceMenuOrder: 123,
    displayNameTraditional: "\u3010\u7cbe\u82f1\u00b7\u89f8\u89d2 \u03b3\u3011\u670d\u88dd",
  },
  {
    afterSourceMenuOrder: 120,
    displayNameTraditional: "\u3010\u7cbe\u82f1\u00b7\u9f8d \u03b1\u3011\u670d\u88dd",
  },
  {
    afterSourceMenuOrder: 120,
    displayNameTraditional: "\u3010\u7cbe\u82f1\u00b7\u9f8d \u03b2\u3011\u670d\u88dd",
  },
  {
    afterSourceMenuOrder: 18,
    displayNameTraditional: "\u3010\u7cbe\u82f1\u00b7\u5996\u6c34\u3011\u670d\u88dd",
  },
];

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function normalizeArmorName(value) {
  return value
    .normalize("NFKC")
    .toLocaleLowerCase("zh-Hant")
    .replace(/\s+/gu, "")
    .replace(/[\u3010\u3011\[\]()\uff08\uff09]/gu, "")
    .replace(/[\u00b7\u30fb\u2027.]/gu, "")
    .replace(/\u670d\u88dd|\u670d\u88c5/gu, "")
    .replace(
      /(?:\u8173\u90e8|\u8eab\u9ad4|\u982d\u90e8|\u8155\u90e8|\u8170\u90e8|\u811a\u90e8|\u8eab\u4f53|\u5934\u90e8)$/u,
      "",
    );
}

function parseSource(markdown) {
  const sections = [];
  let currentSection = null;

  for (const line of markdown.split(/\r?\n/u)) {
    const heading = line.match(/^##\s+(.+)$/u);
    if (heading) {
      const definition = sectionDefinitions[sections.length];
      if (!definition) {
        throw new Error(`Unexpected extra section: ${heading[1]}`);
      }
      currentSection = {
        key: definition.key,
        titleTraditional: heading[1].trim(),
        items: [],
      };
      sections.push(currentSection);
      continue;
    }

    const row = line.match(/^\|\s*(\d+)\s*\|\s*([^|]+?)\s*\|$/u);
    if (row && currentSection) {
      currentSection.items.push({
        menuOrder: Number(row[1]),
        displayNameTraditional: row[2].trim(),
      });
    }
  }

  if (sections.length !== sectionDefinitions.length) {
    throw new Error(`Expected ${sectionDefinitions.length} sections, found ${sections.length}.`);
  }

  for (const [index, section] of sections.entries()) {
    const expectedCount = sectionDefinitions[index].expectedCount;
    if (section.items.length !== expectedCount) {
      throw new Error(
        `${section.key} expected ${expectedCount} rows, found ${section.items.length}.`,
      );
    }
    section.items.forEach((item, itemIndex) => {
      if (item.menuOrder !== itemIndex + 1) {
        throw new Error(`${section.key} has a non-contiguous order at row ${itemIndex + 1}.`);
      }
    });
  }

  return sections;
}

function buildAliasIndex(armorTargets, traditionalNames) {
  const aliases = new Map();
  for (const target of armorTargets) {
    for (const simplifiedName of target.displayNames) {
      const localizedName = traditionalNames[simplifiedName] ?? simplifiedName;
      const alias = normalizeArmorName(localizedName);
      if (!alias) {
        continue;
      }
      const targetIds = aliases.get(alias) ?? [];
      if (!targetIds.includes(target.targetId)) {
        targetIds.push(target.targetId);
      }
      aliases.set(alias, targetIds);
    }
  }
  return aliases;
}

function addMenuSupplements(sections) {
  const masterRank = sections.find((section) => section.key === "masterRank");
  if (!masterRank) {
    throw new Error("The master-rank armor section is missing.");
  }

  const supplementsByAnchor = new Map();
  for (const supplement of masterRankSupplements) {
    const items = supplementsByAnchor.get(supplement.afterSourceMenuOrder) ?? [];
    items.push(supplement);
    supplementsByAnchor.set(supplement.afterSourceMenuOrder, items);
  }

  // Work backwards so each anchor still refers to the original recording row number.
  for (const [anchorOrder, supplements] of [...supplementsByAnchor].sort(
    ([left], [right]) => right - left,
  )) {
    const anchorIndex = masterRank.items.findIndex((item) => item.menuOrder === anchorOrder);
    if (anchorIndex < 0) {
      throw new Error(`Missing master-rank supplement anchor ${anchorOrder}.`);
    }
    masterRank.items.splice(
      anchorIndex + 1,
      0,
      ...supplements.map((supplement) => ({
        menuOrder: 0,
        displayNameTraditional: supplement.displayNameTraditional,
      })),
    );
  }

  masterRank.items.forEach((item, index) => {
    item.menuOrder = index + 1;
  });
  return sections;
}

const modelIndex = readJson(modelIndexPath);
const traditionalText = readJson(traditionalTextPath);
const sourceSections = addMenuSupplements(parseSource(fs.readFileSync(sourcePath, "utf8")));
const knownTargetIds = new Set(modelIndex.armorRemapTargets.map((target) => target.targetId));
const aliases = buildAliasIndex(modelIndex.armorRemapTargets, traditionalText.names);
const targetOrders = {};
let globalOrder = 0;

const sections = sourceSections.map((section) => ({
  ...section,
  items: section.items.map((item) => {
    globalOrder += 1;
    const manualKey = `${section.key}:${item.menuOrder}`;
    const manualMatch = manualMatches.get(manualKey);
    if (
      manualMatch &&
      normalizeArmorName(manualMatch.expectedNameTraditional) !==
        normalizeArmorName(item.displayNameTraditional)
    ) {
      throw new Error(`Manual armor alias name changed at ${manualKey}.`);
    }
    const targetIds =
      manualMatch?.targetIds ?? aliases.get(normalizeArmorName(item.displayNameTraditional));
    if (!targetIds?.length) {
      throw new Error(`No armor target matched ${manualKey}: ${item.displayNameTraditional}`);
    }
    for (const targetId of targetIds) {
      if (!knownTargetIds.has(targetId)) {
        throw new Error(`Unknown manual armor target ${targetId} for ${manualKey}.`);
      }
      const order = {
        sectionKey: section.key,
        menuOrder: item.menuOrder,
        globalOrder,
        displayNameTraditional: item.displayNameTraditional,
      };
      if (!targetOrders[targetId] || globalOrder < targetOrders[targetId].globalOrder) {
        targetOrders[targetId] = order;
      }
    }
    return {
      ...item,
      globalOrder,
      targetIds,
      matchType: manualMatch ? "manualAlias" : "officialTraditionalName",
    };
  }),
}));

const unorderedTargetIds = modelIndex.armorRemapTargets
  .map((target) => target.targetId)
  .filter((targetId) => !targetOrders[targetId])
  .sort();

const output = {
  schemaVersion: 1,
  gameVersion: modelIndex.gameVersion,
  source: {
    path: "sources/armor-layered-menu-order-zh-hant.md",
    locale: "zh-Hant",
    provenance: "userGameRecordingTranscription",
  },
  menuItemCount: globalOrder,
  sections: sections.map((section) => ({
    key: section.key,
    titleTraditional: section.titleTraditional,
    itemCount: section.items.length,
    globalOrderStart: section.items[0].globalOrder,
    globalOrderEnd: section.items.at(-1).globalOrder,
  })),
  targetOrders,
  unorderedTargetIds,
};

fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, "utf8");
console.log(
  `Wrote ${path.relative(projectRoot, outputPath)}: ${globalOrder} menu items, ` +
    `${Object.keys(targetOrders).length} ordered targets, ${unorderedTargetIds.length} fallback targets.`,
);
