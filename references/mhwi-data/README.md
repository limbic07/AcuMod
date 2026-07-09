# MHWI Data Reference

This directory is for MHW Iceborne game data tables used to design future model replacement recognition.

The raw package is kept as local reference material only. It is not imported by the app yet, not bundled with Tauri, and should not be committed or redistributed unless the project has permission to do so.

## Current Raw Package

- Source folder moved from: `MHWI数据表/`
- Local target: `references/mhwi-data/raw/15.10.00-agent-package/`
- Source package name: `Monster Hunter World: Iceborne data table 15.10.00 agent package`
- Formats included by the package: CSV, JSONL, SQLite, manifest, data dictionary.

## Tables Needed First

For the MVP model replacement recognition, the most useful tables are:

| Table | Why It Matters |
| --- | --- |
| `weapons` | Maps weapon type, weapon ID, weapon name, model type, main model path, and attachment model path. |
| `armor` | Maps armor part, armor ID, armor name, layered ID, and model path. |
| `armor_series` | Maps armor or layered armor series name to model path; useful for grouping and display. |

Potential later tables:

| Table | Possible Use |
| --- | --- |
| `palico_weapons` / `palico_armor` | Palico appearance MOD recognition after the first MVP range. |
| `pendants` | Pendant model recognition. |
| `kinsects` | Insect glaive kinsect model recognition. |
| `npc` | NPC model recognition, if the app later supports this category. |
| `monsters`, `items`, `skills`, `decorations`, `stages`, `quests` | MHW terminology support for AI translation and search. |

## Current Gap

The package does not expose an obvious hair or hairstyle table. MVP still lists hairstyle replacement as a target category, so hair recognition will need either a separate source or a path-pattern based first pass.

## Future Curated Data

When model recognition begins, create a small curated table under `curated/` instead of reading the whole raw package from the app. A likely first shape is:

- `model_kind`: `weapon`, `armor`, `hair`, etc.
- `sub_kind`: weapon type or armor part.
- `game_id`: original game ID.
- `display_name`: Chinese in-game name.
- `model_path`: path fragment used to match MOD files.
- `source_table`: raw table name used to derive the row.

