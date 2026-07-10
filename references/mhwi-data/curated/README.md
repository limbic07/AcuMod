# Curated MHWI Data

本目录保存由本地原始 MHWI 数据包生成、可直接供 Acumod 使用的精简索引。原始数据位于 `references/mhwi-data/raw/`，不会提交到 Git。

## model-index.json

`model-index.json` 由以下两张 `15.10.00` 数据表生成：

- `02_weapons.csv`：武器类型、武器 ID、游戏名称、主模型和附件模型路径。
- `03_armor.csv`：防具部位、防具 ID、幻化 ID、游戏名称和模型 ID。

生成命令：

```powershell
.\scripts\build-mhwi-model-index.ps1
```

脚本会过滤 `Unavailable`、`Invalid Message` 和 `dummy` 名称，按模型路径和类型合并共用同一模型的游戏内容。当前索引包含 906 条武器模型映射和 1233 条防具“模型 + 部位”映射，压缩后约 510 KB，并通过 Rust `include_str!` 编译进应用。

主要字段：

- `weaponModels[].modelPath`：例如 `wp/swo/bs_swo001`。
- `weaponModels[].modelPart`：`main` 或 `accessory`。
- `weaponModels[].weaponType`、`weaponIds`、`displayNames`。
- `armorModels[].modelPath`：例如 `pl001_0000`。
- `armorModels[].armorPart`、`armorIds`、`layeredArmorIds`、`displayNames`。

当前原始数据包没有独立的发型名称映射，因此发型暂时只按 MOD 资源路径识别 ID，不在索引中伪造游戏名称。发布前还需要确认原始数据及其衍生索引的再分发许可。
