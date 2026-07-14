# 架构设计

本项目优先做可靠的传统 MOD 管理器，再把 AI Agent 接入同一套受控操作入口。核心原则是：用户界面、Tauri 命令、Rust 业务逻辑和文件操作边界分清楚。

## 当前架构

当前项目已经具备最小可运行链路：

`Vue UI -> typed invoke wrapper -> Tauri command -> DTO response`

对应文件如下：

- `src/App.vue`：展示页面，并在挂载后调用后端。
- `src/api/app.ts`：封装 `invoke<AppInfo>("get_app_info")`，给前端提供类型明确的函数。
- `src-tauri/src/commands/app.rs`：定义 `AppInfo` DTO 和 `get_app_info` Tauri 命令。
- `src-tauri/src/lib.rs`：初始化 Tauri、注册插件、注册 command handler。

这条链路用于证明前端、Tauri IPC、Rust DTO 和 UI 状态更新都能正常工作。

## 推荐分层

随着功能扩展，建议保持下面的边界：

```text
src/
  api/              前端调用 Tauri command 的类型封装
  views/            页面级 UI
  components/       可复用 UI 组件
  stores/           前端状态，后续需要时再引入

src-tauri/src/
  commands/         Tauri command 入口，只做参数接收、DTO 转换、错误转换
  services/         业务逻辑，例如游戏目录检测、MOD 扫描、安装、启停
  models/           Rust 领域模型和 DTO，复杂后再拆
  storage/          配置、MOD 元数据、启用状态、排序和部署记录的读写
  reference_data/   MHW 文件 ID 表等静态或可更新参考数据，后续需要时再创建
```

当前代码量很小，暂时不需要一次性创建所有目录。每次实现一个功能时，只创建实际需要的模块。

## UI 架构

界面采用一个稳定的应用壳，而不是继续把所有功能堆放在单个纵向页面中：

```text
App shell
  ├─ 侧边导航
  │   ├─ MOD 库
  │   ├─ 导入 MOD
  │   ├─ 冲突管理
  │   └─ 设置
  ├─ 顶部状态栏
  ├─ 当前工作区
  └─ 悬浮 AI 助手
```

- `src/App.vue` 暂时继续持有既有的页面状态和操作函数，避免在 UI 重构时改变已经验证的 Tauri 调用链。
- `src/components/` 放应用壳的可复用组件，例如侧边导航、顶部栏和悬浮 AI 窗口；后续业务区块稳定后再逐步抽成 `src/views/` 下的页面。
- MOD 库、导入 MOD、冲突管理和设置是侧边导航中同级的 `WorkspaceView` 页面。切换页面时只替换右侧内容区中的当前页面，左侧导航和顶部状态区保持可用，并可直接切换到其它页面。
- UI 只负责切换当前页面和展示 DTO。切换页面不得重置未完成的导入预览、操作计划或后端状态。

悬浮 AI 助手是独立于当前页面的窗口层。它可以在浏览 MOD 库、导入或设置时打开，但未来只能通过与普通 UI 相同的 `src/api/* -> Tauri command -> Rust service -> OperationPlan` 链路提交动作；不能直接读写文件、绕过预览或跳过用户确认。

## 前端职责

前端负责：

- 展示游戏目录、MOD 列表、冲突排序、模型替换识别结果和操作结果。
- 调用 `src/api/*` 中的类型化函数，而不是在组件里直接散落 `invoke()`。
- 在危险操作前展示明确的确认信息。
- 通过 Tauri Webview 的原生拖拽事件接收文件系统路径；文件夹调用既有识别预览，压缩包调用既有解包导入。
- 展示 Rust 返回的结构化错误，不直接猜测底层文件状态。

前端不负责：

- 直接读写游戏目录。
- 自己拼接复杂文件路径。
- 绕过 Rust 服务执行 MOD 安装、删除、启停。

## Rust 职责

Rust 后端负责：

- 校验游戏目录和 MOD 文件路径。
- 扫描 MHW 的 `nativePC` 等 MOD 相关目录。
- 维护 Acumod 软件目录中的 MOD 库。
- 执行复制式安装、启用、禁用、卸载和冲突组重部署。
- 维护配置、MOD 元数据、启用状态、排序和部署记录。
- 根据 MHW 文件 ID 表识别模型替换目标。
- 生成可预览的操作计划，让用户确认后再执行。

Rust command 应保持薄：

- 解析参数。
- 调用 service。
- 返回 DTO。
- 把内部错误转换成前端可展示的错误信息。

复杂业务逻辑不要堆在 `#[tauri::command]` 函数里。

## DTO 约定

前后端通信使用显式 DTO：

- Rust DTO 使用 `serde::Serialize` / `Deserialize`。
- TypeScript 侧定义对应 interface/type。
- 字段名一旦用于 `invoke()` 参数或响应，修改时必须同步检查前后端。

示例：

```text
TypeScript AppInfo
  name: string
  version: string
  backend: string

Rust AppInfo
  name: &'static str
  version: &'static str
  backend: &'static str
```

## 后台任务与进度

读取大量 manifest、目录扫描、解包和游戏目录复制不能在 Tauri command 的同步主路径中执行。当前重任务统一经过 `src-tauri/src/operations.rs`：

```text
Vue 操作
  -> typed invoke wrapper
  -> async Tauri command
  -> run_blocking_operation
  -> Rust service 的同步文件操作
  -> acumod://operation-progress 事件
  -> App.vue + OperationStatusBar
```

- `OperationCoordinator` 同一时间只允许一个重任务，避免导入、部署和冲突重写同时修改 manifest 或游戏文件；页面导航保持可用，重复重任务会明确提示等待当前任务结束。
- `OperationReporter` 对目录扫描、压缩包解包、本地库复制、游戏目录复制和删除发送阶段与真实完成数。扫描总数未知时只展示当前发现项；文件清单确定后展示 `已完成 / 总数`。
- 每个任务结束后把类型、结果和耗时追加到 `AcumodData/logs/operation-timings.log`。该日志用于定位性能问题，不保存凭据或文件内容。
- 本项目不提供任务取消。复制、删除和 manifest 写入一旦开始必须顺序完成，以保持部署记录与实际游戏目录一致。
- `get_mod_workspace_snapshot` 在一次 manifest 读取后组装 MOD 列表、分类和冲突报告；MOD 库刷新优先使用它，避免前端并行调用三个全量扫描 command。

## MOD 库和复制式部署

当前确认的部署模型是复制式部署：

```text
安装 MOD
  -> 将 MOD 文件导入 Acumod 软件目录中的 MOD 库
  -> 不直接写入 MHW 游戏目录

启用 MOD
  -> 从 MOD 库复制文件到 MHW 游戏目录
  -> 记录这次部署写入了哪些目标文件

禁用 MOD
  -> 删除 MHW 游戏目录中的已部署副本
  -> 保留 Acumod MOD 库中的原始副本

卸载 MOD
  -> 删除 Acumod MOD 库中的 MOD 副本
  -> 如该 MOD 仍处于启用状态，也应先清理它部署到游戏目录的文件
```

这个模型的好处是语义直观：安装表示“纳入管理”，启用才表示“写入游戏目录”。Acumod 始终只维护一套实际部署到游戏目录的启用状态和冲突排序，不提供多套配置或配置切换。

实现时需要记录至少两类路径：

- `source_path`：Acumod 软件目录中保存的 MOD 文件。
- `deployed_path`：启用后复制到 MHW 游戏目录中的目标文件。

禁用和卸载只能删除 Acumod 记录过的 `deployed_path`，不能扫描游戏目录后按猜测删除。

当前存储位置分工：

```text
AppData/
  config.json              小配置，例如 MHW 游戏目录

<软件目录>/
  AcumodData/
    logs/
      operation-timings.log  后台任务类型、结果和耗时
    mods/
      installed/           已导入并由 Acumod 管理的 MOD 副本
      staging/
        imports/           压缩包解包和导入预览的暂存目录
        downloads/         Nexus 下载中的 .part 文件和待导入归档
```

MOD 文件和导入暂存可能很大，不放入 `AppData`。后续制作安装包时，需要确保软件目录对普通用户可写；如果安装到 `Program Files` 等受限目录，应提供 MOD 库位置设置或选择用户可写安装位置。

`staging/imports` 不是第二份 MOD 库。它只在压缩包识别期间暂存完整解压结果：进程首次访问 MOD 库时清理上次异常退出留下的内容，开始新压缩包导入前清理已放弃的候选，成功安装后立即删除本次暂存。`staging/downloads` 后续只保存 Nexus 下载中的 `.part` 文件和等待用户确认导入的归档。`installed/<mod_id>/content` 才是唯一长期副本，多分支压缩包只复制用户选择的候选分支。

`AcumodData/mods/categories.json` 使用 schema 3 保存统一的全局分类定义。公开字段包括稳定 ID、名称、可选 `parentId` 和创建时间；只支持顶级加一层子分类。内部 `recognitionKeys` 用于让新导入 MOD 复用识别类别，`suppressedRecognitionKeys` 防止用户删除的识别分类被自动重建。武器识别会创建或复用顶级“武器”及其子分类，例如“太刀”，MOD 实际关联子分类并显示为“武器·太刀”。每个 `installed/<mod_id>/manifest.json` 保存 `categoryIds[]`，一个 MOD 可关联零个或多个分类；若同时选择父分类和子分类，服务层只保存子分类。删除顶级分类时，直接子分类会保留并提升为顶级分类。分类操作不修改 MOD 内容。

## MOD 导入目录识别

文件夹导入和压缩包导入应共用同一套目录识别规则。压缩包只负责先解包到暂存目录；解包后的目录树仍然走同一个识别入口。

第一版识别顺序：

1. 优先查找 `nativePC` 目录，并把其中的文件映射为 `nativePC/...`。
2. 如果没有 `nativePC`，但内容根下出现 `weapon`、`wp`、`pl`、`plugins`、`common`、`npc`、`em`、`stage`、`sound`、`ui` 等常见 nativePC 内部目录，则自动补成 `nativePC/...`。
3. 如果用户直接选择了 nativePC 内部目录本身，例如 `plugins/` 或 `weapon/`，则保留目录名并映射为 `nativePC/plugins/...` 或 `nativePC/weapon/...`。
4. 如果出现多个同级候选内容根，不自动选择，返回候选列表；用户选择后 Rust service 会重新扫描源目录并校验该路径仍是候选，只复制所选分支。
5. 如果无法识别 `nativePC` 或常见内部目录，但目录内存在文件，则提示用户确认是否按游戏根目录相对路径导入。确认前不能自动执行。

这个规则覆盖两类常见情况：

```text
导入内容/nativePC/weapon/...
  -> nativePC/weapon/...

导入内容/weapon/...
  -> nativePC/weapon/...

导入内容/nativePC/plugins/...
用户直接选择 plugins/
  -> nativePC/plugins/...
```

也保留游戏根目录 MOD：

```text
导入内容/dinput8.dll
  -> dinput8.dll
```

游戏根目录 MOD 风险更高，因为它可能覆盖可执行文件同级内容，所以必须经过明确确认和路径预览。

## MHW 模型替换识别

MHW 的替换 MOD 通常可以通过资源路径和文件 ID 判断它替换的是哪一个游戏内对象。Acumod 内置维护精简 ID 索引，把底层 ID 映射成用户能理解的名称。MVP 覆盖武器、防具、发型、随从武器、随从防具、猎虫、挂件、NPC、猎人手臂上的投射器/飞翔爪和人物语音；同一个 MOD 可能识别出多个目标。Slice 14 在识别结果之上支持武器、防具、随从防具、投射器/飞翔爪和发型改绑；人物语音以及其它识别类别仍为只读。

当前识别链路：

```text
MOD 文件列表
  -> 解析 MHW 资源路径和文件 ID
  -> 查询 MHW 文件 ID 表
  -> 得到模型类型和游戏内名称
  -> 返回 ModelReplacement DTO
  -> UI 展示“该 MOD 替换了什么”
```

模型替换信息建议包含：

- 替换类型：武器、防具、发型、随从装备、猎虫、挂件、NPC、投射器/飞翔爪或人物语音。
- 具体类型：例如太刀、大剑、弓等。
- 原始目标 ID。
- 原始目标游戏内名称。

当前 `references/mhwi-data/curated/model-index.json` 由 `scripts/build-mhwi-model-index.ps1` 从 `15.10.00` 本地表和 curated 社区映射生成，并通过 `include_str!` 编译进 Rust。`model_recognition` service 只接受从 `nativePC` 开始的规范资源根目录：武器从 `wp/...`、防具从 `pl/f_equip/...` 或 `pl/m_equip/...`；`vfx/mod` 中即使包含相同模型 ID，也只被视为附属特效资源。一个模型可能被多个游戏对象共用，因此 DTO 保留名称和 ID 数组；同一防具模型必须命中头盔、铠甲、护手、腰甲和护腿五个标准部位才合并为一个套装 DTO，只有部分部位时继续返回独立 DTO。UI 遇到套装 DTO 时从官方分部位名称提取套装级名称，不得用第一条部位名称作为摘要。

新导入 MOD 使用 manifest schema 14 持久化 `modelReplacements`、`modelRemaps`、显示名称、备注和 `categoryIds[]`。schema 1 至 12 的旧 manifest 会先结合本地库路径和 `.evam` 内容重算识别结果；schema 13 及更早版本的 `categoryOverride` 会与识别得到的初始分类合并后写入 `categoryIds[]`，schema 10 以后已保存的改绑选择仍会保留。模型 ID 和装备部位只从目录组件识别；人物语音因资源格式没有独立 ID 目录，仅在 `sound/wwise/Windows` 下精确匹配完整 `.nbnk` 文件名。

投射器/飞翔爪接受 `wp/slg/slgNNN_NNNN` 和旧 `slgNNN` 目录。已核对条目来自原始 MOD 页面；其它规范目录只返回 `pathPattern` 底层 ID，不按防具同号猜测。普通文件名不参与投射器识别，也不识别 `Assets/gm/gm000` 下的投射器弹药。

防具手臂 `pl/{f,m}_equip/plNNN_NNNN/arm/mod/*.evam` 是防具到飞翔爪编号的直接绑定来源。识别器严格校验 26 字节长度、`EVAM` 标记和版本 3，再读取偏移 `0x10` 的小端编号。只有同一 MOD 内存在编号匹配的 `wp/slg` 模型时，才在该飞翔爪 `ModelReplacement.associations` 中附加关联防具；孤立 `.evam` 不产生替换结果。完整防具显示官方套装级名称，例如“【冰狼】服装”，不追加“（套装）”。

`model_remap` service 从原始 `ModelReplacement` 构建可改绑分组，并按类别校验目标。manifest 只保存用户选择；本地 `content/` 始终保持导入时的单份原始文件。列表展示和部署时，Rust 即时生成有效文件路径并重新识别有效目标。

```text
Vue 改绑对话框
  -> typed invoke wrapper
  -> preview_mod_remap / apply_mod_remap
  -> model_remap 校验同类目标并生成有效文件表
  -> manifest.json 只保存 modelRemaps
  -> preview_enable_mod / enable_mod / conflict service 复用有效文件表
```

目录改名使用按模型类别生成的受限路径规则，而不是全局替换数字：武器、随从防具、发型和飞翔爪只处理各自已验证的资源根与文件名 token；防具另外识别规范 `epv/{f,m}_<部位>NNN.epv3` 文件名，仅将三位套装号替换为目标套装号，不使用目标变体号。`vfx`、自定义资源目录和未知命名保持原路径。部分 `.mrl3` 材质文件内保存贴图资源路径，部署器只解析已验证的 MRL3 头和贴图表，对恰好对应已移动 `.tex` 文件的路径做精确替换；飞翔爪改绑还会在部署副本中同步修改已关联 `.evam` 的偏移 `0x10` 编号。字符串越界、EVAM 源 ID 不一致、格式异常或目标路径碰撞都会阻止保存或部署，本地 MOD 库原件始终不变。人物语音不进入该链路。

## Nexus Mods 下载边界

Slice 15 采用独立的 Nexus 适配层，网络协议和凭据不能进入现有 `mod_library` service：

```text
Vue 下载页
  -> typed invoke wrapper
  -> nexus commands
  -> nexus auth / metadata / download services
  -> staging/downloads/<task_id>/<file>.part
  -> 大小与哈希校验
  -> 现有 archive import preview
  -> 用户确认候选分支
  -> 本地 MOD 库 manifest
```

建议 Rust 模块为 `commands/nexus.rs`、`services/nexus/client.rs`、`auth.rs`、`download.rs` 和 `storage/credentials.rs`。Vue 不接收 API key、签名下载地址或任意磁盘目标路径，只传递后端签发的 task ID、MHW mod ID 和 file ID。

Nexus 当前主线元数据接口为 API v3，但下载链接仍需要由适配层兼容现有下载接口。正式版本使用已注册 Acumod application slug 的 Nexus SSO；开发期 Personal API key 只用于本机验证。Premium 用户可直接生成下载链接，免费用户必须从 Nexus 页面取得 NXM 的临时 `key/expires`。下载完成后仍复用 Acumod 已有的解包、候选选择、游戏根目录确认和同名去重规则，不允许 Nexus command 直接写入游戏目录。

## ID 表后续用途

MHW 文件 ID 表不只用于展示模型替换目标。MVP 之后，它还可以成为 MOD 分类、筛选和排序的依据：

- 根据识别结果自动标记 MOD 类型，例如武器外观、防具外观、发型替换。
- 根据武器种类、装备部位、游戏内名称筛选 MOD。
- 在 MOD 列表中按替换目标聚合或排序。
- 在冲突排序中优先突出替换同一模型或同一路径的 MOD。

这些能力应建立在只读识别结果上，不要求修改 MOD 文件。

当前索引进一步覆盖脸型、怪物、噗吱猪服装、家具和玩家/随从附件。怪物和噗吱猪服装由本地 `15.10.00` 中文表提供显示名；脸型、家具和附件先按稳定目录模式返回类别与原始 ID，避免把社区英文名称或猜测当作官方简体中文。冲突报告会比较同一组已启用 MOD 的 `modelKind + modelId`，把至少两个参与者共同替换的目标作为辅助提示返回；它不改变实际文件冲突规则。

## MVP 存储策略

MVP 先使用 JSON 文件保存配置、MOD 元数据、启用状态、排序和部署记录。

选择 JSON 的原因：

- 当前项目已经使用 `serde` / `serde_json`。
- MVP 查询关系还不复杂。
- 文件结构更适合学习和调试。
- 暂时不需要引入数据库依赖。

当前传统管理器增强阶段仍使用 JSON：每个 `installed/<mod_id>/manifest.json` 的 `enabled` 与 `deployedFiles` 表示当前真正部署到游戏目录的状态，`installed/conflict-orders.json` 保存各冲突组的优先级，`mods/mod-library-order.json` 保存手动浏览顺序。冲突组顺序的第一项是最终覆盖者，MOD 库顺序绝不参与部署。它们共同构成唯一的运行状态。

### 多配置移除迁移

多配置、状态快照和配置切换均不属于产品范围。移除该功能时不需要转换当前 MOD 状态：每个 manifest 已保存 `enabled` 与 `deployedFiles`，`conflict-orders.json` 已保存当前冲突顺序，它们继续作为唯一事实来源。

当前代码不再包含 Profile DTO、Tauri command、Rust 存储模块或服务层同步逻辑，也不会在启动、启停、还原、冲突排序或卸载时访问 `AcumodData/mods/profiles.json`。旧文件保持原样，程序不读取、写入或自动删除它；旧文件中的额外命名快照也不会再被恢复或切换。

如果后续出现大量 MOD、复杂搜索、历史记录或高频查询，再评估 SQLite。

## AI Agent 接入边界

AI Agent 不应该直接操作文件系统，也不应该直接执行删除、覆盖、移动等动作。

推荐未来链路：

```text
用户自然语言
  -> AI 解析意图
  -> 生成受控操作请求
  -> Rust service 生成 OperationPlan
  -> UI 展示计划和风险
  -> 用户确认
  -> Rust service 执行
  -> DTO 返回结果
```

也就是说，AI 只能进入与传统 UI 相同的操作入口。传统按钮能做什么，AI 才能申请做什么；传统管理器尚未实现的能力，AI 也不应绕过实现。MVP 不包含 AI Agent；Slice 14 已实现的五类模型改绑仍必须经过与传统 UI 相同的预览、校验和用户确认。

后续 AI Agent 可以扩展三类能力：

- MHW 术语感知翻译：翻译 MOD 名称和说明时使用 MHW 游戏术语表、文件 ID 表和已识别模型信息，保留原文，并避免误译武器名、装备名、怪物名、技能名。
- 联网搜索辅助：根据用户自然语言需求搜索候选 MOD，返回来源链接、摘要、适用类型和风险提示，让用户自行下载安装。
- Nexus Mods 集成：在用户完成 SSO 登录或授权后，通过 Nexus Mods API 搜索和下载 MOD，再交给传统导入、预览、安装流程处理。

AI Agent 的下载和安装能力仍应落到传统管理器的受控操作计划上，不应绕过本地 MOD 库、部署记录、冲突提示和用户确认。

## 已完成的第一个 MVP 切片

第一个 MVP 切片是“检测并保存 MHW 游戏目录”：

1. Vue 页面提供路径输入和自动检测入口。
2. `src/api/game.ts` 提供类型化 Tauri 调用。
3. Rust command 接收路径参数。
4. Rust service 检查目录是否存在、是否包含 `MonsterHunterWorld.exe`。
5. 返回 `GameDirectoryStatus` DTO。
6. JSON storage 保存已确认的游戏目录。
7. Vue 显示检测结果和错误原因。

这个切片能同时验证路径参数、错误处理、DTO、文件系统访问和 UI 展示，是后续 MOD 管理能力的基础。

第二个 MVP 切片是“MOD 库目录与导入内容识别预览”：

1. Rust service 在软件目录旁的 `AcumodData/` 下创建 `mods/installed` 和 `mods/staging/imports`。
2. 前端展示 MOD 库路径。
3. 用户输入本地 MOD 文件夹。
4. Rust service 识别 `nativePC`、常见 nativePC 内部目录、多候选目录和游戏根目录 fallback。
5. Vue 展示部署路径预览，不真正安装、不写入游戏目录。

第三个 MVP 切片是“文件夹 MOD 导入到本地 MOD 库”：

1. Vue 在导入预览为 `ready` 后提供“导入到 MOD 库”入口。
2. `src/api/modLibrary.ts` 调用 `install_mod_from_folder`。
3. Rust command 接收 `path` 和 `allow_game_root`。
4. Rust service 重新使用导入识别规则确认内容根，并收集完整文件列表。
5. Rust service 将文件复制到 `AcumodData/mods/installed/<mod_id>/content/`。
6. Rust service 写入 `manifest.json`，记录 MOD ID、名称、来源路径、内容根、识别方式、部署相对路径和启用状态。
7. Vue 展示本地库目录、内容目录、manifest 路径和已导入文件预览。

这个切片仍然只表示“安装到 Acumod 本地 MOD 库”，不表示“启用到 MHW 游戏目录”。压缩包导入可以在同一规则稳定后接入：先解包到 staging，再调用相同的导入识别和本地安装逻辑。

第四个 MVP 切片是“已安装 MOD 列表”：

1. Rust service 扫描 `AcumodData/mods/installed/`。
2. 每个 MOD 目录必须包含 `manifest.json`。
3. Rust service 读取 manifest，返回 MOD ID、名称、文件数、启用状态、部署根、识别方式和库内路径。
4. Vue 展示已安装 MOD 列表，并提供刷新入口。

第五个 MVP 切片是“压缩包 MOD 导入”：

1. Vue 提供 `.zip`、`.7z`、`.rar` 压缩包路径输入。
2. `src/api/modLibrary.ts` 调用 `install_mod_from_archive`。
3. Rust service 校验压缩包存在且扩展名受支持。
4. Rust service 查找 Acumod 随包携带的 7-Zip 解包组件：`resources/unpackers/7zip/7z.exe` 和 `7z.dll`。
5. Rust service 将压缩包解包到 `AcumodData/mods/staging/imports/<archive>-<stamp>/`。
6. 解包后的目录继续走文件夹导入识别和本地安装逻辑。
7. manifest 的 `source_path` 记录原始压缩包路径，`content_root_path` 记录解包后识别出的内容根。

当前没有新增 Rust 解包依赖。Acumod 采用随安装包分发解包组件的方式，避免要求用户另行安装 7-Zip；代价是发布包会增加几 MB，并且需要随包保留 7-Zip 许可文件。

第六个 MVP 切片是“启用和禁用已安装 MOD”：

1. Vue 在已安装 MOD 列表中提供启用、禁用入口。
2. `src/api/modLibrary.ts` 调用 `preview_enable_mod`、`enable_mod`、`preview_disable_mod` 和 `disable_mod`。
3. Rust service 读取已保存的 MHW 游戏目录，并再次校验 `MonsterHunterWorld.exe`。
4. 启用前生成部署计划，列出库内源文件、游戏目录目标文件、目标是否已存在，以及是否由 Acumod 记录为其他 MOD 部署。
5. 如果目标文件已存在且不是同一个 MOD 的已记录部署，前端必须确认后才调用真正启用。
6. 启用时从 `AcumodData/mods/installed/<mod_id>/content/` 复制文件到 MHW 游戏目录，并把 `deployedFiles` 写回 manifest。
7. 禁用前返回实际 `deployedFiles` 供 UI 预览；确认后只按这些记录删除游戏目录文件，然后清空部署记录并标记为未启用。

这个切片仍不解决 MOD 之间的最终覆盖顺序；冲突排序会在后续独立切片中实现。当前规则只保证启停链路可用，并且删除动作有明确记录依据。

第七个 MVP 切片是“卸载已安装 MOD”：

1. Vue 在已安装 MOD 列表中提供卸载入口。
2. `src/api/modLibrary.ts` 调用 `preview_uninstall_mod` 和 `uninstall_mod`。
3. Rust service 先读取该 MOD 的 manifest，生成卸载预览：库内文件数量、已记录部署文件数量、是否当前仍启用。
4. 用户确认后，如果该 MOD 有 `deployedFiles`，Rust service 先复用部署记录清理逻辑删除游戏目录中的已部署文件，并清理由这些文件留下的空目录。
5. 部署清理完成后，Rust service 删除 `AcumodData/mods/installed/<mod_id>/`，也就是 Acumod 管理的本地 MOD 副本。

卸载不扫描 MHW 游戏目录，不根据 MOD 名称猜测删除文件；它只处理 manifest 中记录过的部署文件和 Acumod 本地库中的对应 MOD 目录。

第八个 MVP 切片是“一键还原纯净状态”：

1. Vue 在已安装 MOD 区域提供一键还原入口。
2. `src/api/modLibrary.ts` 调用 `preview_restore_all_mods` 和 `restore_all_mods`。
3. Rust service 扫描 Acumod 本地 MOD 库中的所有 manifest，找出仍处于启用状态或仍有 `deployedFiles` 记录的 MOD。
4. 预览阶段只返回会影响的 MOD 数量和部署文件数量，不写入游戏目录。
5. 用户确认后，Rust service 复用部署记录清理逻辑，删除所有记录过的游戏目录部署文件，并把相关 MOD 标记为未启用、清空 `deployedFiles`。

一键还原不扫描或猜测 MHW 游戏目录中的未知文件，因此它只能还原 Acumod 记录过的部署内容。手动安装的文件或其他工具安装的文件不会被删除。

第九个 MVP 切片是“冲突检测和按 MOD 组排序”：

1. Rust service 只扫描当前已启用 MOD 的有效部署路径，先找出相同目标路径，再构建 MOD 冲突关系图；同一连通分量中的 MOD 归为一个冲突组，未启用 MOD 不显示。
2. 主 MOD 列表的默认手动浏览顺序保存在 `mods/mod-library-order.json`，可以拖拽调整；它不维护全局 MOD 优先级。
3. Vue 使用独立的冲突管理工作区，只显示每组包含哪些 MOD 和整体优先级，并可展开查看该组冲突文件路径；A/B 与 C/D 这类无关关系显示为两个组。
4. 用户移动参与 MOD 时，只更新 `AcumodData/mods/installed/conflict-orders.json` 中该 MOD 组合对应的顺序，不立即改写游戏目录。
5. 用户点击“应用此组优先级”后，Rust service 遍历组内全部冲突文件；每个文件由顺序中第一个已启用且实际包含该文件的 MOD 提供，并同步更新 `deployedFiles` 记录归属。
6. 每次成功启用 MOD 后，Rust service 把它放到相关冲突组最上方并立即按该优先级协调冲突文件，因此后启用的 MOD 默认覆盖先启用的 MOD。

冲突覆盖不额外创建备份，因为各 MOD 的原始文件已保存在 Acumod 本地 MOD 库中。若游戏目录目标文件存在但没有 Acumod 部署记录，应用前仍会要求用户确认覆盖。

第十个 MVP 切片是“多候选分支导入和模型替换识别”：

1. 文件夹或压缩包扫描到多个同级内容根时返回候选路径、部署根和文件数，Vue 使用单选列表让用户选择。
2. `install_mod_from_candidate` 重新校验源目录和候选路径，拒绝导入候选列表以外的目录，并只复制所选分支。
3. 生成脚本从 MHWI `15.10.00` 中文表及 curated 社区映射生成精简 JSON 索引，不把完整原始数据包编入应用。
4. Rust `model_recognition` service 返回 `ModelReplacement`；该切片最初使用 schema 9，当前 schema 14 继续持久化并兼容旧识别结果；`vfx/mod` 附属资源不作为装备替换目标显示。
5. Vue 在导入结果和已安装 MOD 列表显示替换类型、模型 ID、游戏 ID 和游戏名称摘要。

这个切片只读取路径并展示识别结果，不修改 MOD 的模型目标或文件内容；Slice 14 的受控改绑建立在该识别结果上。

MVP 收尾补充了已安装 MOD 的完整文件列表、主列表冲突状态，以及禁用操作的 Rust 端文件预览。至此 `docs/features.md` 中的 MVP 完成标准已全部形成可操作 UI 和受控 Rust service 链路。
