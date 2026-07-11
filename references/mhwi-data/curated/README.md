# Curated MHWI Data

本目录保存由本地原始 MHWI 数据包生成、可直接供 Acumod 使用的精简索引。原始数据位于 `references/mhwi-data/raw/`，不会提交到 Git。

## model-index.json

`model-index.json` 由两张 `15.10.00` 数据表和一份社区整理表生成：

- `02_weapons.csv`：武器类型、武器 ID、游戏名称、主模型和附件模型路径。
- `03_armor.csv`：防具部位、防具 ID、幻化 ID、游戏名称和模型 ID。
- `sources/hairstyles.json`：发型模型路径、角色创建界面槽位、Wiki 英文原名和官方简体中文名称，来源记录在文件内。

生成命令：

```powershell
.\scripts\build-mhwi-model-index.ps1
```

脚本会过滤 `Unavailable`、`Invalid Message` 和 `dummy` 名称，按模型路径和类型合并共用同一模型的游戏内容。当前索引包含 906 条武器模型映射、1233 条防具“模型 + 部位”映射和 89 条发型映射，并通过 Rust `include_str!` 编译进应用。

主要字段：

- `weaponModels[].modelPath`：例如 `wp/swo/bs_swo001`。
- `weaponModels[].modelPart`：`main` 或 `accessory`。
- `weaponModels[].weaponType`、`weaponIds`、`displayNames`。
- `armorModels[].modelPath`：例如 `pl001_0000`。
- `armorModels[].armorPart`、`armorIds`、`layeredArmorIds`、`displayNames`。
- `hairModels[].modelPath`、`modelId`、`gameIds`、`displayNames`。

发型路径映射来自社区维护的 [Monster Hunter World Modding Wiki](https://github.com/AniBullet/MonsterHunterWorldModding/wiki/Hairstyle-IDs)。显示名称面向简体中文用户：数字槽位始终排在最前，例如 `发型 11-2、优美`；DLC 名称使用 CAPCOM 发布的 Steam 简体中文商品名，NPC 名称优先使用本地 `17_npc.csv` 游戏文本。英文 Wiki 名称仅保留在 curated 源文件中用于追溯，不进入应用显示。发布前还需要确认原始数据、Wiki 表及其衍生索引的再分发许可。
