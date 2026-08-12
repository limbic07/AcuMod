# AcuAI Knowledge Reference

这里保存 AcuAI 知识包的开发资料、来源登记和可重复生成的审计结果，不作为主程序资源直接打包。

## 版本边界

- 运行兼容目标为 Steam/PC 最终版本 `15.23`；知识库的最终内容与数值基线固定为 `15.10.00`。
- 固定提交的 `MHWorldData` 是当前唯一游戏数值输入；构建器必须保留来源提交、每张 CSV 的哈希、原始字段和 `15.10.00` 内容基线，不能伪装为直接从 `15.23` 资源提取。
- 后续版本未新增实际游戏内容或机制，因此不要求为数值知识全量重新解包 `15.23` 游戏文件。字段缺口、来源和抽样核验要求见 [`docs/knowledge-data-standard.md`](../../docs/knowledge-data-standard.md)。

## 目录

```text
references/knowledge/
  sources/catalog.json       # 来源、许可边界、许可审计状态和用途登记
  audits/                    # knowledge:audit 生成的匿名基线报告
  build/                     # 本地构建输出，不提交 Git
  pack-format.md             # .acukb 固定格式
```

运行：

```powershell
npm.cmd run knowledge:audit
npm.cmd run knowledge:audit-licenses
npm.cmd run knowledge:fetch-mhw-db
npm.cmd run knowledge:fetch-mhworlddata
npm.cmd run knowledge:build-game-text-bridge
npm.cmd run knowledge:build-dev
npm.cmd run knowledge:verify-dev
npm.cmd run knowledge:verify-e2e
npm.cmd run knowledge:verify-release
```

也可以显式指定本地 MOD 库：

```powershell
npm.cmd run knowledge:audit -- --mod-root "D:\path\to\AcumodData\mods\installed"
```

## 任务解锁资料

为消除第三方攻略页面派生内容的再分发风险，当前构建链路不再抓取、保存或导入第三方任务解锁页面。固定 MHWData 只提供任务基础、目标和报酬；任务前置、活动开放、交货委托和营地条件没有可审计的本地字段时，AcuAI 必须明确说明资料缺口，或在用户提问时按受控联网规则读取页面摘录，不能写入知识包。

## 数据与隐私边界

- 本地 MOD 库仅用于统计文件类型、相对目录前缀、数量和体积。
- 审计报告不得包含 MOD 名称、MOD ID、MOD 文件名、来源路径、游戏目录或其他绝对路径。
- `references/mhwi-data/raw/` 中的数据只作为本地研究资料；未经来源方许可，不重新分发原始数据或将其打进知识包。
- 每条外部知识都要登记来源、适用游戏版本、许可边界、许可审计状态和核对状态。
- 一个 `.acumhwdb` 数值数据库和两个 `.acukb` 文本包由独立 ZIP 统一下载和发布，不随 Acumod 主程序安装包发布。

`knowledge:build-dev` 生成 `acumod-mhwdata-15.10.acumhwdb`、`acumod-dev-modding.acukb` 与 `acumod-dev-acumod-help.acukb`。数值数据库保留上游原始行，不建立事实图谱或 FTS；两个文本包各自重建 FTS5。实际体积在每次构建后重新测量；构建产物不提交 Git，通过独立 ZIP 分发。

### 无原始表时的可重复开发构建

`15.10.00-agent-package` 是受限本地研究资料，不会随仓库提供。开发机没有该表时，构建器会自动切换到 `mhworlddata-fallback`：使用固定 commit 的 MHWData 快照生成武器、防具、装饰珠、护石、物品、技能、怪物、肉质、任务、地图、制作、奖励和采集关系。该模式不伪造原始表的稳定 ID；实体 ID 会带 `mhwdata` 命名空间。通过字段覆盖、结构校验和抽样后，其中有来源的字段可作为 `15.10.00` 最终内容事实回答，未导入或上游缺失字段仍必须明确说明缺口。

首次准备步骤如下：

```powershell
npm.cmd run knowledge:fetch-mhworlddata
git clone https://github.com/Synthlight/MHW-Editor.git src-tauri/target/analysis/MHW-Editor-source
git -C src-tauri/target/analysis/MHW-Editor-source checkout a9fd86fd7dbd29fc3f85b1a2ea8ecb0f47458a94
npm.cmd run knowledge:build-game-text-bridge
npm.cmd run knowledge:fetch-mhw-db
npm.cmd run knowledge:build-dev
npm.cmd run knowledge:verify-dev
npm.cmd run knowledge:verify-e2e
```

当前 MHW-Editor 文本源固定为 commit `a9fd86fd7dbd29fc3f85b1a2ea8ecb0f47458a94`；克隆后应以 `git -C src-tauri/target/analysis/MHW-Editor-source rev-parse HEAD` 核对。构建器只按同一文本键配对简中、繁中和英文，绝不做逐字简繁转换；没有唯一同键简中名称的任务等条目保留官方繁中或英文回退。任务前置、特别任务、交货委托和同名或近似标题的解锁链不再从第三方页面导入，查询这些条件时必须明确说明资料缺口。

程序只支持整套 ZIP 导入：选择一个包含一个 `.acumhwdb` 和两个 `.acukb` 文件的 ZIP 并点击“整套安装”。程序会先解包并校验固定数值数据库、MOD 技术包和 Acumod 帮助包，全部通过后再安装；旧 `mhw-game-facts` 图谱与 `mhw-game-guides` 攻略包会在读取状态或安装新整包时移除，避免被后续检索误用。

`knowledge:build-dev` 构建固定数值数据库和两个文本包。`knowledge:verify-dev` 验证 `15.10.00` 内容基线、`15.23` 运行兼容标记、50 张源表、8,500 以上实体、3 万以上原始行，以及武器斩味、防具技能、怪物肉质/奖励、任务报酬、技能等级等固定 section；还会验证 MOD3、MRL3、武器特效边界、EVAM、DAT 改绑边界、SPL 等 17 条离线基础规则和 Acumod 使用说明。

旧 `knowledge:verify-e2e` 面向四 `.acukb` 图谱，已由 Rust `mhwdata` 的安装/查询集成测试取代：测试临时安装刚生成的数值数据库，查询防卫队大剑基础行和其斩味 section，再删除临时目录。

`knowledge:fetch-mhw-db` 是一次显式联网的开发资料抓取：它保存物品、技能和防具的结构化快照到被忽略的 `references/knowledge/raw/`，当前用于技能等级补充。`knowledge:fetch-mhworlddata` 抓取固定 commit 的武器、防具、装饰珠、护石、怪物、任务、地图、制作/报酬/采集字段以及英文与可用的官方繁中名称；`knowledge:build-game-text-bridge` 从本机 MHW-Editor 的同文本键简繁文件生成完整名称桥。装备、怪物和素材构建时只接受“英文名 -> 官方繁中 -> 同一游戏文本键的本地简中 -> 唯一实体”的双重映射；任务使用本地和外部共有的数值任务 ID，地图使用人工核对的英文名、外部地点 ID 和本地 `STxxx` 场景 ID 三重映射，均不做字形转换或跨来源 ID 猜测。外部来源未标出 `15.23` 不构成准入阻碍：它们以 `15.10.00` 内容基线、固定提交/文件哈希和字段级校验进入包；来源不明、字段缺失或校验失败时才保留 `unverified`。

## 知识库制作流程

知识库由项目维护者制作，按“来源登记 -> 清洗建模 -> 自动校验 -> 人工抽样 -> 打包发布”的顺序完成。知识库不是把网页直接塞给模型，也不是让 DeepSeek 自行生成事实。

### 1. 游戏事实包

`mhw-game-facts` 以 Steam/PC `15.23` 为目标运行版本。内容事实以 `15.10.00` 为基线：项目确认 `15.10` 是最后一次有明显游戏内容和机制变化的更新，`15.11` 至 `15.23` 主要为修复、Steam Deck/语言适配、系统文件和宣传数据更新，并未新增怪物、武器或大型任务；覆盖矩阵仍要记录每类来源、许可、语言和字段缺口。

随后用确定性 ETL 统一稳定 ID、官方简体中文、官方繁體中文、英文/常用别名、数值字段和实体关系。每次导入都生成重复 ID、悬空外键、语言缺失、内容基线、数值范围和抽样事实报告。外部字段未完成来源、数值或再分发审计时，仍不能进入正式包。

### 2. MOD 技术包

`mhw-modding` 是离线安全基础包，保留 17 条可由本地分析器、现有改绑实现或稳定格式边界直接支撑的规则：`nativePC` 路径、MOD3/MRL3/TEX、CTC/CCL、特效与 EVWP 边界、EVAM、`armor.am_dat`、原生插件/Lua Framework/SharpPluginLoader（SPL）及组件证据等级。它不再承担完整的 MOD 制作百科；详细制作、格式和排错问题由 AcuAI 按需读取受控 ModdingWiki 页面。技术包只保存项目原创说明和来源链接，不复制外部 Wiki、工具代码或游戏数据。

我会先统计真实 MOD 库中的路径、扩展名和组件组合，再按覆盖率实现只读解析器。每条规则都保存来源、适用版本、证据等级和验证方式：二进制内部引用是确定证据，规范路径和 ID 是高可信证据，仅同目录共现是启发式证据。无法确认的格式保留为“未知”，不由模型猜测后写入知识包。

### 3. 通用问答与 RAG

当前知识底座包含固定 `mhwdata`、`mhw-modding` 和 `acumod-help` 三份资料。游戏攻略不再作为本地知识包；开放式推荐必须先用 MHWData 核验具体实体和字段，再按受控联网规则读取当轮白名单页面摘录。

用户问题先由 AcuAI 提取目标、限制和术语，再调用固定工具查询实体、原始行和技术/帮助文档。开放式问题也走这条通用链路：例如配装推荐会先查询玩家进度、目标武器、可用装备和技能数据，再按受控联网规则补充并由模型给出多个可解释方案。

每条返回结果都带 `evidenceId`、来源、游戏版本、知识包版本和可信度。Rust 只会把实际查询到且通过知识包结构校验的实体、关系和来源发送给前端；AcuAI 回答下方展示本轮实际使用的来源。事实、攻略建议、本地文件分析、受控联网参考和未知缺口分开标注。游戏精确数据先走 MHWData；MOD 技术先走本地分析与离线规则，不足时只可从指定 ModdingWiki 的同轮候选页面读取摘录。没有可靠资料时明确说明缺口，不用模型记忆或普通联网搜索补写精确游戏数据或本地文件行为。

### 5. 发布验收

构建脚本输出 `.acukb`、来源/许可清单、覆盖报告和 SHA-256；自动验收覆盖知识包结构、SQLite 完整性、实体/关系/全文检索和已确认的复杂 MOD 样本回归。新的人工问题集完成后再加入语义验收。通过后知识包作为独立下载物发布，主程序只保留安装、校验、启用和删除能力。
