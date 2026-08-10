import { createHash } from "node:crypto";
import { mkdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const outputPath = path.join(
  projectRoot,
  "references/knowledge/raw/mhworlddata/armor-name-map.json",
);
const sourceCommit = "be7362213d7d1e30b794e3b58d3f87712035658d";
const sourcePaths = {
  weaponBase: "source_data/weapons/weapon_base.csv",
  weaponTranslations: "source_data/weapons/weapon_base_translations.csv",
  weaponCrafting: "source_data/weapons/weapon_craft.csv",
  weaponSharpness: "source_data/weapons/weapon_sharpness.csv",
  weaponAmmo: "source_data/weapons/weapon_ammo.csv",
  weaponBow: "source_data/weapons/weapon_bow_ext.csv",
  weaponMelodies: "source_data/weapons/weapon_melody_base.csv",
  weaponMelodyNotes: "source_data/weapons/weapon_melody_notes.csv",
  kinsectBase: "source_data/weapons/kinsect_base.csv",
  kinsectTranslations: "source_data/weapons/kinsect_base_translations.csv",
  kinsectCrafting: "source_data/weapons/kinsect_craft_ext.csv",
  armorBase: "source_data/armors/armor_base.csv",
  armorTranslations: "source_data/armors/armor_base_translations.csv",
  armorSkills: "source_data/armors/armor_skills_ext.csv",
  armorCrafting: "source_data/armors/armor_craft_ext.csv",
  armorSets: "source_data/armors/armorset_base.csv",
  armorSetTranslations: "source_data/armors/armorset_base_translations.csv",
  armorSetBonuses: "source_data/armors/armorset_bonus_base.csv",
  armorSetBonusTranslations: "source_data/armors/armorset_bonus_base_translations.csv",
  decorationBase: "source_data/decorations/decoration_base.csv",
  decorationTranslations: "source_data/decorations/decoration_base_translations.csv",
  decorationDropRates: "source_data/decorations/decoration_droprates.csv",
  charmBase: "source_data/charms/charm_base.csv",
  charmTranslations: "source_data/charms/charm_base_translations.csv",
  charmCrafting: "source_data/charms/charm_craft.csv",
  itemBase: "source_data/items/item_base.csv",
  monsterBase: "source_data/monsters/monster_base.csv",
  monsterTranslations: "source_data/monsters/monster_base_translations.csv",
  monsterAilments: "source_data/monsters/monster_ailments.csv",
  monsterBreaks: "source_data/monsters/monster_breaks.csv",
  monsterHabitats: "source_data/monsters/monster_habitats.csv",
  monsterWeaknesses: "source_data/monsters/monster_weaknesses.csv",
  monsterHitzones: "source_data/monsters/monster_hitzones.csv",
  monsterRewards: "source_data/monsters/monster_rewards.csv",
  rewardConditions: "source_data/monsters/reward_conditions_base.csv",
  questBase: "source_data/quests/quest_base.csv",
  questTranslations: "source_data/quests/quest_base_translations.csv",
  questMonsters: "source_data/quests/quest_monsters.csv",
  questRewards: "source_data/quests/quest_rewards.csv",
  locationBase: "source_data/locations/location_base.csv",
  locationCamps: "source_data/locations/location_camps.csv",
  locationItems: "source_data/locations/location_items.csv",
  gatheringStacks: "source_data/locations/gather_stacks.csv",
  toolBase: "source_data/tools/tool_base.csv",
  toolTranslations: "source_data/tools/tool_base_translations.csv",
  skillBase: "source_data/skills/skill_base.csv",
  skillTranslations: "source_data/skills/skill_base_translations.csv",
  skillLevels: "source_data/skills/skill_levels.csv",
  itemTranslations: "source_data/items/item_base_translations.csv",
  itemCombinations: "source_data/items/item_combination_list.csv",
};

function parseCsv(text) {
  const rows = [];
  let row = [];
  let value = "";
  let quoted = false;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (quoted && character === '"' && text[index + 1] === '"') {
      value += '"';
      index += 1;
    } else if (character === '"') {
      quoted = !quoted;
    } else if (!quoted && character === ",") {
      row.push(value);
      value = "";
    } else if (!quoted && character === "\n") {
      row.push(value.replace(/\r$/u, ""));
      rows.push(row);
      row = [];
      value = "";
    } else {
      value += character;
    }
  }
  if (value || row.length) {
    row.push(value.replace(/\r$/u, ""));
    rows.push(row);
  }
  const headers = rows.shift() ?? [];
  return rows
    .filter((values) => values.some((entry) => entry.trim()))
    .map((values) => Object.fromEntries(headers.map((header, index) => [header, values[index] ?? ""])));
}

const entries = await Promise.all(Object.entries(sourcePaths).map(async ([name, sourcePath]) => {
  const sourceUrl = `https://raw.githubusercontent.com/gatheringhallstudios/MHWorldData/${sourceCommit}/${sourcePath}`;
  const response = await fetch(sourceUrl, { headers: { Accept: "text/plain" } });
  if (!response.ok) {
    throw new Error(`无法下载 MHWData ${sourcePath}：${response.status} ${response.statusText}`);
  }
  const csv = await response.text();
  return [name, {
    sourcePath,
    sourceUrl,
    sha256: createHash("sha256").update(csv).digest("hex"),
    rows: parseCsv(csv),
  }];
}));
const tables = Object.fromEntries(entries);
const armorTranslations = tables.armorTranslations.rows;
const duplicateEnglishName = armorTranslations.find((row, index) => armorTranslations.findIndex((other) => other.name_en === row.name_en) !== index);
if (duplicateEnglishName) {
  throw new Error(`MHWData 防具英文名称重复：${duplicateEnglishName.name_en}`);
}

const snapshot = {
  schemaVersion: 1,
  sourceId: "mhworlddata-armor-name-map",
  sourceCommit,
  contentBaselineVersion: "15.10.00",
  retrievedAt: new Date().toISOString(),
  tables,
};

await mkdir(path.dirname(outputPath), { recursive: true });
const temporaryPath = `${outputPath}.tmp`;
await writeFile(temporaryPath, `${JSON.stringify(snapshot, null, 2)}\n`, "utf8");
await rename(temporaryPath, outputPath);
console.log(`已保存 MHWData 游戏事实开发快照：${path.relative(projectRoot, outputPath)}`);
console.log(`武器 ${tables.weaponBase.rows.length}，防具 ${tables.armorBase.rows.length}，装饰珠 ${tables.decorationBase.rows.length}，护石 ${tables.charmBase.rows.length}，怪物 ${tables.monsterBase.rows.length}`);
