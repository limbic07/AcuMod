# `.acukb` Format 1

`.acukb` 是 Acumod 使用的单文件 SQLite 知识包。文件只保存声明式数据，不包含脚本、动态库、模型提示词或任意可执行 SQL。

## SQLite 标识

- `PRAGMA application_id = 0x4143554B`，ASCII 为 `ACUK`。
- `PRAGMA user_version = 1`。
- 当前应用接受 `mhw-modding`、`mhw-game-facts`、`mhw-game-guides` 和 `acumod-help` 四种包类型。

## 必要数据表

| 表 | 用途 |
| --- | --- |
| `pack_manifest` | 唯一一条包 ID、类型、包版本、游戏版本、语言和最低应用版本 |
| `sources` | 来源标题、URL、类型、适用版本、许可说明和许可审计状态 |
| `entities` | 稳定实体、名称、摘要、版本、可信度和类型化 JSON 数据 |
| `aliases` | 实体的简繁中文、英文、资源 ID 和常用别名 |
| `relations` | 实体之间带来源、版本和可信度的有向关系 |
| `documents` | MOD 技术资料与攻略正文 |
| `knowledge_fts` | 使用 trigram tokenizer 的中文子串全文索引 |

运行时不执行知识包提供的 SQL。Rust 只使用固定、参数化查询，并拒绝未知 table、trigger 和 view；FTS5 自动生成的 `knowledge_fts_*` shadow table 是唯一允许的额外数据表。

## ZIP 发布与安装流程

1. 发布物是一个 ZIP，内部必须包含四个 `.acukb`：`mhw-game-facts`、`mhw-modding`、`mhw-game-guides` 和 `acumod-help`。四个文件可以位于 ZIP 根目录或其子目录中。
2. 用户在设置页选择 ZIP；前端通过系统文件选择器取得路径，不要求复制路径。
3. Rust 使用应用内置 7-Zip 解包到 `AcumodData/knowledge/staging/`，不依赖用户额外安装 7-Zip。
4. 解包后必须找到且仅找到四个 `.acukb`，逐个校验文件上限、SQLite 标识、schema、完整性、manifest、包类型、最低应用版本、结构化 JSON、可信度范围和实体/来源引用完整性。
5. 四个包全部通过后，分别移动到 `packs/<pack-id>/<version>-<hash>.acukb`，更新 `index.json`；同一包 ID 的旧版本保留但不再活动。

ZIP 解包或任一包校验失败时，不安装未完成校验的整套包，并清理本次解包暂存目录。

开发包由 `npm.cmd run knowledge:build-dev` 可重复生成并写入忽略目录。正式知识包还必须附带来源许可、`15.10.00` 内容基线记录和字段核验；人工问题集待重新设计后再加入发布验收。
