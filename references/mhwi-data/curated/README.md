# Curated MHWI Data

本目录保存由本地原始 MHWI 数据包生成、可直接供 Acumod 使用的精简索引。原始数据位于 `references/mhwi-data/raw/`，不会提交到 Git。

## model-index.json

`model-index.json` 由七张 `15.10.00` 数据表和两份 curated 社区映射生成：

- `02_weapons.csv`：武器类型、武器 ID、游戏名称、主模型和附件模型路径。
- `03_armor.csv`：防具部位、防具 ID、幻化 ID、游戏名称和模型 ID。
- `09_palico_weapons.csv`、`10_palico_armor.csv`：随从武器和防具模型路径、ID 与中文名称。
- `12_pendants.csv`、`13_kinsects.csv`、`17_npc.csv`：挂件、猎虫和 NPC 模型 ID 与中文名称。
- `06_monsters.csv`、`18_poogie.csv`：怪物与噗吱猪服装的本地简体中文名称。
- `sources/hairstyles.json`：发型模型路径、角色创建界面槽位、Wiki 英文原名和官方简体中文名称，来源记录在文件内。
- `sources/extended-assets.json`：投射器模型目录（含通用 `slg000_0000`）以及男女各 20 个角色创建语音序号与 `.nbnk` 文件名映射。
- `sources/additional-assets.json`：噗吱猪服装 ID 到 `pg` 资源目录的社区路径映射；名称仍来自本地中文表。

生成命令：

```powershell
.\scripts\build-mhwi-model-index.ps1
```

脚本会过滤 `Unavailable`、`Invalid Message` 和 `dummy` 名称，按模型路径和类型合并共用同一模型的游戏内容。当前索引包含 906 条武器、1233 条防具、89 条发型、598 条扩展模型映射和 40 条人物语音映射，并通过 Rust `include_str!` 编译进应用。

主要字段：

- `weaponModels[].modelPath`：例如 `wp/swo/bs_swo001`。
- `weaponModels[].modelPart`：`main` 或 `accessory`。
- `weaponModels[].weaponType`、`weaponIds`、`displayNames`。
- `armorModels[].modelPath`：例如 `pl001_0000`。
- `armorModels[].armorPart`、`armorIds`、`layeredArmorIds`、`displayNames`。
- `hairModels[].modelPath`、`modelId`、`gameIds`、`displayNames`。
- `assetModels[]`：随从装备、猎虫、挂件、NPC、投射器、怪物和噗吱猪服装的统一模型映射。
- `voiceModels[]`：人物语音文件名、性别、角色创建序号和显示名称。

发型和投射器路径映射来自社区维护的 Monster Hunter World Modding Wiki；语音序号映射来自 MIT 许可的 [MHW Voice Changer](https://github.com/NathanCruz98/MHWVoiceChanger)。显示名称面向简体中文用户：发型数字槽位始终排在最前，例如 `发型 11-2、优美`；DLC 名称使用 CAPCOM 发布的 Steam 简体中文商品名，NPC 名称优先使用本地游戏文本。发布前仍需确认原始数据与 Wiki 衍生索引的再分发许可。
