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

## MOD 分支边界

分支组是 MOD 库的组织层，不是新的部署层：

```text
branch-groups.json（组名、成员 MOD ID）
  -> 多个 installed/<mod_id>/manifest.json
  -> 各分支独立生成有效部署路径
  -> 复用现有启停、冲突排序、禁用恢复和模型改绑
```

- `AcumodData/mods/branch-groups.json` 只保存组 ID、组名、成员 ID 和创建时间。
- 每个分支仍是完整的普通 MOD，具有独立 `content/`、manifest、启用状态和部署记录；允许同时启用多个分支。
- 分支组不额外保存一份分类。组行编辑分类时，前端根据各成员当前 `categoryIds[]` 生成批量 assignment，`update_mod_categories` 在写入前统一校验 MOD 与分类 ID，随后一次更新全部成员 manifest 和工作区快照；这样筛选、排序和普通 MOD 分类逻辑仍只有一个数据来源。
- `workspace-snapshot.json` 缓存分支组 DTO，组关系变化时局部更新；刷新时按实际已安装 ID 清理失效成员，只剩一个成员的组自动拆散。
- 冲突报告仍使用真实 MOD ID；Vue 展示时组合为“组名（分支名）”，不会把组伪装成可部署 MOD。
- 自动分组建议只读取工作区快照中的部署路径、原始名称、导入来源、识别目标和冲突报告。两个单文件 MOD 部署到完全相同的路径时直接建立候选关系；多文件 MOD 必须同时达到“共同路径覆盖较小文件集至少 90%”和“共同路径占文件并集至少 75%”，其中双文件 MOD 还需满足名称相似并具有相同来源或共同目标。组装使用完整链接校验，任意新成员必须与组内每个成员都达标，组级真实交集也必须继续满足相同阈值；不使用冲突传递合并，超过 16 项的候选直接抑制。它不扫描磁盘、不自动写入，用户确认后仍逐组调用 `create_mod_branch_group`，由 Rust 校验成员并更新存储和快照。
- 内嵌压缩包复用随包 7-Zip，最多递归两层和 32 个文件；压缩包直接进入导入暂存，含内嵌包的文件夹先完整复制到导入暂存，每个内嵌包再在隔离子目录解包。候选 DTO 同时保留原始来源路径和压缩包来源链，安装完成后清理暂存，不修改用户原目录。

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
- `OperationReporter` 对目录扫描、压缩包解包、本地库复制、游戏目录复制、删除和游戏实际状态比对发送阶段与真实完成数。扫描总数未知时只展示当前发现项；文件清单确定后展示 `已完成 / 总数`。状态同步必须把“读取清单”“准备有效文件”“逐文件比对”“分析覆盖关系”和“保存状态”分成独立阶段，不能在比对期间停留在清单完成画面。
- 每个任务结束后把类型、结果和耗时追加到 `AcumodData/logs/operation-timings.log`。该日志用于定位性能问题，不保存凭据或文件内容。
- 本项目不提供任务取消。复制、删除和 manifest 写入一旦开始必须顺序完成，以保持部署记录与实际游戏目录一致。
- `get_mod_workspace_snapshot` 优先读取 `AcumodData/mods/workspace-snapshot.json`；缓存缺失、损坏或 manifest schema 变化时才执行一次全量扫描并重建。快照 schema 4 在展示 DTO 之外保存每个 MOD 的有效部署路径、有效识别目标索引和自动分组需要的原始导入来源。普通导入、启停、卸载、批量操作、模型改绑和冲突应用只重读受影响的 manifest，再基于索引在内存中更新冲突报告；用户点击“刷新”时调用 `refresh_mod_workspace_snapshot` 强制重读全部 manifest、重新识别并分析冲突。
- 工作区快照只是可丢弃、可重建的读取缓存，不保存独立部署状态。manifest、`conflict-orders.json` 和 `mod-library-order.json` 仍是事实来源。
- `mod-library-order.json` schema 2 同时保存 `manualModIds` 和 `importModIds`。前者只控制 MOD 库手动浏览顺序，后者记录最早导入在上的恢复基准；启停、冲突应用、模型改绑和元数据编辑不能重排已有项。只有持有完整 MOD 列表的全量刷新可以规范化并持久化失效 ID；单个 MOD 的局部快照更新只能替换摘要，不得调用完整顺序归一化。旧 schema 1 `modIds` 作为 `manualModIds` 读取并保持原顺序，缺失的导入基准按安装时间和稳定 MOD ID 补建。
- 排序保存链路为 `ModLibraryToolbar -> App.vue 完整库排序 -> replaceModLibraryOrder typed invoke -> replace_mod_library_order Tauri command -> Rust 顺序校验与存储 -> ModLibraryOrderResult -> Vue 原地重排`。恢复导入顺序使用独立 command；两个命令都通过后台任务执行，优先使用工作区快照校验完整 MOD ID，快照缺失时只读取 manifest，不重新扫描模型或游戏目录。
- 禁用 MOD 后恢复低优先级版本时，先按索引筛选包含同一有效路径的已启用候选，只加载这些 manifest；冲突预览和应用同样只加载当前冲突组及相关部署记录所有者。索引缺失时回退到全库兼容扫描，不牺牲旧数据可用性。
- 单项启用预检在 `ModDeploymentPlan.conflicts` 中按已启用 MOD 汇总有效部署路径交集。快照存在时直接使用 `mod_index`，快照缺失时才读取 manifest；前端只在该数组非空时显示可展开的冲突确认框，确认后继续调用原有 `enable_mod`。普通游戏原文件和未跟踪文件不伪装成冲突 MOD。

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

## 狩技 MOD 盒子导入与自动状态同步

只读扫描、批量导入、重复内容关联和自动状态同步已经实现；显式接管原型及其 UI、IPC 和 command 已移除。用户可见状态保持为“已启用”和“未启用”，已启用 MOD 只增加一个可选的“部分被冲突覆盖”提示。

当前链路：

```text
导入页输入盒子目录
  -> scanLegacyBoxMods()
  -> scan_legacy_box_mods command + 后台任务
  -> legacy_box service 读取 config.ini / info.xml / files
  -> 默认选择全部模块

用户导入
  -> importLegacyBoxMods()
  -> import_legacy_box_mods command + 后台任务
  -> 复用标准文件夹导入
  -> 识别或关联重复 MOD
  -> AcumodData/mods/installed/<mod_id>/content
  -> mod_state_sync service
  -> 自动判定启用状态、部分覆盖关系和冲突顺序
  -> 一次返回导入与状态同步结果

用户刷新实际状态
  -> refreshGameModStates()
  -> refresh_game_mod_states command + 后台任务
  -> 复用同一 mod_state_sync service
```

`legacy_box` service 继续只读用户选择目录内的 `Mods_582010/<数字模块 ID>/info.xml` 和 `files/`，忽略备份、下载、存档和临时目录。`quick-xml` 只负责结构化解析第三方 `info.xml`。

### 重复 MOD 关联

重复关系不能只按显示名称或来源路径判断。状态同步按以下顺序关联盒子模块与本地 MOD：

1. 优先使用已经保存的盒子模块来源关联。
2. 尚无来源关联时，比较部署相对路径集合与文件大小结构；只有结构完全相同的候选才继续逐文件流式比较。
3. 全部路径和内容一致时，将盒子模块作为现有本地 MOD 的来源别名，不再创建第二份本地内容，也不覆盖现有 Acumod 启用状态。
4. 名称相同但内容不同不视为重复，会作为独立 MOD 导入本地库。

这一过程使用现有分块读取和完整字节比较，不依赖文件名猜测，也不需要新增内容哈希依赖。一次盒子批量导入只建立一份本地 manifest 内存索引；每次新增或关联 MOD 后只更新对应索引项，不能为每个盒子模块重新扫描整个本地库。

### 文件提供者索引

`mod_library` 读取所有 manifest，并通过现有有效部署文件生成器应用模型改绑、MRL3 路径修正和 EVAM 绑定修正，再完成与游戏目录的内容比较。纯内存的 `mod_state_sync` 接收这些比对事实，随后建立：

```text
规范化部署路径
  -> 一个或多个 MOD 提供者
  -> 每个提供者在该路径上的有效内容
  -> 游戏目录当前内容
```

- 同路径、同内容的提供者归入一个等价内容组，不产生冲突，也不能用于推断唯一归属。
- 同路径、不同内容的提供者才产生冲突关系。
- 比较时先检查文件存在性和大小，再进行流式完整内容比较；转换后的 EVAM/MRL3 使用与实际部署相同的转换函数生成预期内容。
- 已经由 Acumod 启用且有部署记录的 MOD 是可信的已启用提供者，不重新推断其启用状态；实际文件不符合记录时只产生操作警告，不能用于解释新导入 MOD 的覆盖关系。

### 启用状态推导

新导入或此前由文件检测管理的 MOD 按以下算法推导：

1. `完全匹合集合`：有效部署文件全部与游戏目录对应内容一致的 MOD 直接判定为已启用。
2. `部分匹配候选`：至少一个自身文件匹配、但不是全部匹配的 MOD 进入冲突解释阶段。
3. 对候选的每个不匹配路径，游戏文件必须存在，并准确匹配某个已经确认启用提供者的同路径不同内容；由此建立“实际赢家优先于当前候选”的有向边。
4. 候选不能包含缺失文件、未知内容或无法找到已启用赢家的路径；覆盖关系加入后不得形成环。
5. 满足条件的候选判定为已启用并标记“部分被冲突覆盖”，随后加入已确认集合；重复执行步骤 3 至 5，直到没有新候选加入，以支持多层覆盖链。
6. 其余部分匹配 MOD 判定为未启用。
7. 完全没有自身文件匹配的 MOD 判定为未启用，即使其全部路径理论上可能被其他 MOD 覆盖。
8. A 在部分路径优先于 B、B 又在其它路径优先于 A 时会形成环，说明当前目录是无法由整体 MOD 顺序表达的混合部署；相关新导入 MOD 均判定为未启用。

冲突组对上述有向边执行稳定拓扑排序；无约束部分优先保留现有 `conflict-orders.json` 顺序，其次使用稳定 MOD ID，确保多次扫描结果一致。盒子 `index` 在未确认升降序语义前只作来源信息，不参与自动优先级。最上方 MOD 仍表示最终覆盖者。

### 状态写入

分析阶段先生成完整的 `ModStateSyncPlan`，确认整个计划内部一致后才写元数据：

- 完全匹配 MOD：`enabled = true`，当前实际提供的路径写入 `deployedFiles`。
- 部分被覆盖 MOD：`enabled = true`；只把当前实际由它提供的赢家路径写入 `deployedFiles`，被高优先级 MOD 覆盖的路径由对应赢家记录。
- 未启用 MOD：`enabled = false`，不建立观察所得部署记录。
- 同路径、同内容的多个已启用提供者只选择一个规范记录者：优先沿用已有 Acumod 记录，否则使用稳定 MOD ID；其它等价提供者保持已启用但不重复占有该路径。
- 可确定的冲突顺序一次写入 `conflict-orders.json`。
- 观察游戏目录得到的部署记录与 Acumod 主动复制产生的记录必须保留内部来源差异。schema 15 的每条 `deployedFiles[]` 使用 `deploymentOrigin: "copied" | "observed"`；未带该字段的旧记录按 `copied` 读取，上一版原型留下的 `isAdopted` 记录会迁移为 `observed`。
- 盒子来源别名保存为可选的 `legacySources[]`，至少包含规范化盒子路径与模块 ID；它不参与用户显示名称和同名判断。

写入前不修改游戏目录，也不需要用户确认。实现时先写同目录临时 JSON，再原子替换正式 manifest；任一元数据写入失败时不得继续报告状态同步成功。

### 后续文件操作

观察所得记录不能直接套用普通删除逻辑：

- 禁用、卸载或一键还原前，重新生成该 MOD 当前有效内容并与游戏目标比较。
- 内容不一致、路径无法确认或文件已缺失时，不删除目标。
- 同路径仍有其它已启用等价内容提供者时，不删除目标；只更新记录归属。
- 删除当前赢家后，仅在该路径原内容仍与记录一致时，才部署冲突顺序中的下一个已启用提供者。
- 上述保护按规则自动执行，不弹出逐文件确认；被保留的文件集中写入操作警告。

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

MHW 的替换 MOD 通常可以通过资源路径和文件 ID 判断它替换的是哪一个游戏内对象。Acumod 内置维护精简 ID 索引，把底层 ID 映射成用户能理解的名称。MVP 覆盖武器、防具、发型、随从武器、随从防具、猎虫、挂件、NPC、猎人手臂上的投射器/飞翔爪和人物语音；后续路径规则增加 `nativePC/plugins` 插件和按武器类型识别的语音资源。同一个 MOD 可能识别出多个目标。Slice 14 在识别结果之上支持武器、防具、随从防具、投射器/飞翔爪和发型改绑；人物语音、武器语音、插件以及其它识别类别仍为只读。

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

- 替换类型：武器、防具、发型、随从装备、猎虫、挂件、NPC、投射器/飞翔爪、人物语音、武器语音或插件。
- 具体类型：例如太刀、大剑、弓等。
- 原始目标 ID。
- 原始目标游戏内名称。

当前 `references/mhwi-data/curated/model-index.json` 由 `scripts/build-mhwi-model-index.ps1` 从 `15.10.00` 本地表和 curated 社区映射生成，并通过 `include_str!` 编译进 Rust。外观装备菜单录屏抄录保存在 `curated/sources/armor-layered-menu-order-zh-hant.md`，`scripts/build-mhwi-armor-menu-order.mjs` 通过官方繁中名称关联防具模型目标并生成 `curated/armor-menu-order.json`；Rust 改绑列表和 Vue 分类排序共同读取该文件。`model_recognition` service 只接受从 `nativePC` 开始的规范资源根目录：武器从 `wp/...`、防具从 `pl/f_equip/...` 或 `pl/m_equip/...`；`vfx/mod` 中即使包含相同模型 ID，也只被视为附属特效资源。一个模型可能被多个游戏对象共用，因此 DTO 保留名称和 ID 数组；同一防具模型必须命中头盔、铠甲、护手、腰甲和护腿五个标准部位才合并为一个套装 DTO，只有部分部位时继续返回独立 DTO。UI 遇到套装 DTO 时从官方分部位名称提取套装级名称，不得用第一条部位名称作为摘要。

繁体游戏名称使用独立的 `game-text-zh-hant.json`，由 `scripts/build-mhwi-traditional-game-text.mjs` 按 MHW-Editor 成对简体/繁体游戏文本键生成。manifest 和工作区快照继续保存稳定识别 ID 与现有名称；前端 `gameText` 解析层只在展示时替换名称，因此切换 `config.json.gameTextLanguage` 不需要扫描 MOD、迁移 manifest 或重新计算冲突。未收录名称必须回退原文，不能自动逐字转换。

新导入 MOD 使用 manifest schema 17 持久化 `modelReplacements`、`modelRemaps`、显示名称、备注、`categoryIds[]`、状态同步元数据和 `deploymentExclusions[]`。模型识别规则版本独立保持为 16。旧 manifest 会在读取时按迁移规则保留现有元数据并重新识别缺失或过期的模型结果。模型 ID 和装备部位只从目录组件识别；人物语音与武器语音因资源格式没有独立 ID 目录，仅在 `sound/wwise/Windows` 下精确匹配完整 `.nbnk` 文件名。武器语音只接受 `wp_<代码>_(cmn|epvsp)` 或 `wpNN_<代码>_(cmn|epvsp)`，并映射到 14 种武器；无法确定武器的公共音频包不猜测分类。`nativePC/plugins` 下的内容统一识别为“插件”。

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

当前传统管理器增强阶段仍使用 JSON：每个 `installed/<mod_id>/manifest.json` 的 `enabled` 与 `deployedFiles` 表示当前真正部署到游戏目录的状态，`installed/conflict-orders.json` 保存各冲突组的优先级，`mods/mod-library-order.json` 保存手动浏览顺序。`mods/workspace-snapshot.json` 只是上述数据的读取缓存。冲突组顺序的第一项是最终覆盖者，MOD 库顺序绝不参与部署。前三者共同构成唯一的运行状态。

### 多配置移除迁移

多配置、可切换的部署状态快照和配置切换均不属于产品范围。移除该功能时不需要转换当前 MOD 状态：每个 manifest 已保存 `enabled` 与 `deployedFiles`，`conflict-orders.json` 已保存当前冲突顺序，它们继续作为唯一事实来源。这里不包含可随时重建的 `workspace-snapshot.json` 读取缓存。

当前代码不再包含 Profile DTO、Tauri command、Rust 存储模块或服务层同步逻辑，也不会在启动、启停、还原、冲突排序或卸载时访问 `AcumodData/mods/profiles.json`。旧文件保持原样，程序不读取、写入或自动删除它；旧文件中的额外命名快照也不会再被恢复或切换。

如果后续出现大量 MOD、复杂搜索、历史记录或高频查询，再评估 SQLite。

## AI Agent 接入边界

AI Agent 不是第二套 MOD 管理器。它只负责理解自然语言、查询受控数据和生成操作草案，不直接操作文件系统，也不能把任意字符串转换为 Tauri command、路径或 shell 命令。

AI Agent 是完全可选模块，严格遵守单向依赖：`services/agent` 可以调用传统查询、预览和执行 service，传统导入、部署、冲突、改绑和存储 service 不得导入 Agent 模块，也不得等待 DeepSeek 或网络结果。用户未配置访问密钥、断网或从不打开悬浮助手时，传统管理器行为必须与未实现 AI 时一致。

### 推荐链路

```text
FloatingAgentPanel
  -> src/api/agent.ts typed invoke wrapper
  -> commands/agent.rs
  -> services/agent/mod.rs（会话和单 turn 协调）
  -> services/agent/deepseek.rs（DeepSeek V4 Chat Completions）
  -> services/agent/tools.rs（固定工具白名单）
  -> 既有查询 service 或 OperationPlan 预览 service
  -> AgentTurn / AgentActionPlan DTO
  -> 悬浮窗口展示结果或待确认计划
  -> 用户确认 planId
  -> Rust 重新校验计划并调用既有执行 service
```

模型侧使用 JSON Schema 描述函数工具。首期使用 DeepSeek 标准端点而不是 Beta `strict` 端点，因此 Rust 必须把模型返回的工具名和参数视为不可信请求，再完成枚举解析、未知字段拒绝、ID 存在性检查、状态检查和业务校验。禁止设计 `execute_command(name, args)`、`run_path(path)` 之类的通用工具。

### Rust 模块边界

当前模块：

- `commands/agent.rs`：设置读取、连接测试、发起对话、确认或拒绝计划等 Tauri 边界。
- `services/agent/mod.rs`：保存内存会话、协调单次 turn、管理 DeepSeek 设置和 Windows 凭据，不包含 MOD 文件操作实现。
- `services/agent/deepseek.rs`：封装 DeepSeek V4 请求、流式 SSE 解析、工具调用消息和错误转换；项目不设计多供应商抽象。
- `services/agent/tools.rs`：声明允许模型调用的工具、严格参数 schema、分页和 Rust handler。
- `services/agent/cleanup.rs`：校验全量候选分类、生成清理审查 DTO 和默认选择；不执行任何文件操作。

后续按实际复杂度在 `services/agent/` 下增加 `plans.rs`、`knowledge.rs` 和 `sources/` 等模块，分别承担短时计划、知识检索和联网来源适配；不要预先拆出没有独立职责的空文件。Agent DTO 继续靠近该模块维护，只有出现跨业务复用时再迁移到公共模型目录。

现有 `mod_library`、冲突、模型改绑和任务 service 继续拥有业务规则。Agent handler 只能调用这些 service 的查询、预览和执行入口，不能复制一套部署逻辑。

### 五项能力的服务边界

- **冗余文件清理**：传统部署 service 只增加通用的“部署排除记录和重新协调”能力；扫描和 AI 分类仅由 Agent 主动调用。模型只看到候选相对路径等精简元数据，清理计划确认后由部署 service 删除游戏副本或恢复其他冲突所有者，本地 MOD 库原始副本始终保留。
- **联网搜索与安装**：新增来源适配层，Nexus 由官方 API adapter 负责，其他站点首期只返回可验证的外部链接。DeepSeek Chat Completions 负责选择固定工具和整理候选，不充当浏览器、下载器或站点抓取器。
- **自然语言控制**：模型把意图映射为稳定 ID 和操作枚举；`AgentActionPlan` 确认后复用现有 `OperationCoordinator`，不新增第二套启停、卸载、冲突和改绑实现。
- **MOD 知识分析**：知识条目以 MOD ID、来源、版本/哈希、路径特征和文本来源为边界。精确冲突与部署状态继续查询工作区快照，检索文本只用于解释和诊断建议。
- **游戏知识问答**：复用同一检索接口但使用独立 `game` 命名空间。素材、技能等精确数据来自版本化结构化索引，攻略和配装资料必须保留来源、版本与适用条件。

知识检索第一阶段采用结构化筛选加全文检索，不预设向量数据库。只有语义召回质量确实不足且数据来源、包体和更新机制明确后，才评估本地向量索引。MOD README、网页文本和社区攻略均作为不可信引用数据注入上下文，不能成为 system 指令或工具参数来源。

建议的首批 Tauri command：

- `get_agent_settings`：只返回 DeepSeek V4 模型、是否已配置 Key 和脱敏标识。
- `set_agent_api_key` / `delete_agent_api_key`：只在 Rust 中写入或删除系统凭据。
- `test_agent_connection`：发送最小请求，返回服务、模型、耗时和中文错误。
- `start_agent_turn`：接收 `AgentTurnRequest` 和 Tauri `Channel<AgentEvent>`，立即开始异步对话。
- `confirm_agent_action_plan` / `cancel_agent_action_plan`：按 `planId` 确认或丢弃 Rust 内存中的计划。
- `create_agent_cleanup_plan`：把用户在审查卡片中的候选选择提交给 Rust，换取普通短时操作计划；前端不能直接应用排除项。

`AgentEvent` 使用有序 Channel 而不是全局广播事件，避免多个窗口或会话串线。事件类型固定为 `started`、`textDelta`、`toolStarted`、`toolFinished`、`cleanupReviewReady`、`planReady`、`completed` 和 `failed`，每项携带 `turnId` 与递增序号。`cleanupReviewReady` 只携带经过 Rust 全量校验的候选审查 DTO，前端不能自行构造执行参数。前端只拼接 `textDelta`，不解析 DeepSeek 的原始 SSE 数据。第一版不提供停止生成；单次请求设置总超时，进行中禁用重复发送。

### 工具分级

只读工具可在一次对话中自动执行：

- `search_local_mods`：按名称、备注、分类、状态和替换目标查询 MOD，返回稳定 MOD ID。
- `get_mod_details`：读取选中 MOD 的状态、识别摘要和冲突摘要。
- `get_enabled_conflicts`：查询当前已启用冲突组及优先级。
- `lookup_mhw_terms`：从本地简中/繁中游戏文本和 ID 索引查询术语。
- `get_game_directory_status`：只返回是否已配置和是否有效，不向模型发送完整本地路径。
- `scan_mod_cleanup_candidates`：从全部已安装 MOD 中返回可能为预览图、说明或教程的候选元数据，不修改文件。
- `search_mod_knowledge` / `search_game_knowledge`：分别查询带来源的 MOD 与游戏知识片段。
- `search_mod_sources`：通过固定来源 adapter 查询 MOD 候选，不接受任意 URL。

写操作工具只生成 `AgentActionPlan`，不能在工具调用阶段执行：

- 启用、禁用或卸载一组 MOD。
- 修改冲突组优先级。
- 修改已支持类型的模型替换目标。
- 接受 AI 翻译建议并写入用户可编辑名称或备注。
- 应用或恢复一组已确认的部署排除项。
- 下载并导入用户已选择的 Nexus 文件。

每个计划至少包含 `planId`、操作枚举、目标稳定 ID、中文摘要、警告、状态版本和过期时间。当前计划有效期为 5 分钟，只保存在 Rust 内存；前端只能提交 `planId`，不能修改执行参数。用户确认后，Rust 使用当前 manifest 和工作区快照重新检查目标状态，再通过 `OperationCoordinator` 调用传统 service；目标已卸载、状态已变化或计划过期时拒绝执行并要求重新生成。AI 发起的所有写操作统一确认，即使传统 UI 对某些低风险操作可以直接执行。

### 模型与网络

Acumod 只接入 DeepSeek V4，不支持 OpenAI API、其它模型供应商或自定义兼容地址。Rust 直接请求 `https://api.deepseek.com/chat/completions`，不在 Vue/WebView 中请求模型，也不引入 OpenAI SDK。DeepSeek 提供的 OpenAI 兼容格式只作为 HTTP 消息格式使用，所有网络请求仍只发送到 DeepSeek 官方域名。

按 2026-07-17 的官方接口，默认模型为成本和速度优先的 `deepseek-v4-flash`，设置中可以切换 `deepseek-v4-pro`。不使用即将于 2026-07-24 停用的 `deepseek-chat` 和 `deepseek-reasoner` 旧别名。首个切片显式发送 `thinking: { type: "disabled" }`，使用标准工具调用和流式输出：这样不依赖 Beta `strict` 端点，也不需要在多轮工具调用中维护 `reasoning_content`。等只读工具链稳定后，再单独评估是否为复杂规划开启思考模式。

Rust 负责 HTTPS、超时、流式响应解析和错误归一化。DeepSeek 访问密钥只在用户输入和提交期间短暂存在于前端，提交后立即清空；Rust 不向前端回传明文，也不写入日志或普通 JSON 配置。模型选择只接受上述两个固定枚举，不允许用户输入任意模型名或 Base URL。

AI 文本请求使用独立的异步状态，不占用全局文件任务锁；真正执行启停、卸载、冲突应用或模型改绑时，仍进入现有 `OperationCoordinator`。首期同一时间只允许一个 Agent turn，网络失败或超时不会阻塞 MOD 库操作。

### 上下文和会话

- 不把整个 MOD 库、文件清单或游戏目录一次性发送给模型。先在 Rust 本地筛选并分页；普通查询只返回完成回答所需的精简结果，用户明确要求完整列表时按 `nextOffset` 查询到末页。
- 默认只发送稳定 ID、显示名、分类、启用状态、替换摘要和冲突数量，不发送本地绝对路径、文件内容、API Key 或完整日志。
- MHW 术语通过 `lookup_mhw_terms` 按需查询，不把完整 ID 表塞进 system prompt。
- 清理分析默认只发送候选文件的 MOD ID、相对路径、扩展名、大小和部署状态；不上传图片、二进制文件或整个 MOD 压缩包。
- 知识问答只发送命中的最小片段及来源元数据，`mods` 与 `game` 两个知识域不得混成无来源的长上下文。
- 第一版只保存当前运行期间的会话，不把聊天记录持久化；设置中后续再增加可选历史记录。
- DeepSeek API Key 应保存到 Windows Credential Manager。开发阶段可使用进程环境变量 `DEEPSEEK_API_KEY`，禁止写入 `AppData/config.json` 或 `AcumodData/`。

### Nexus Mods 边界

Nexus 搜索和下载由独立 Rust service 实现，不能写在模型工具循环中。该 service 可由 Agent 的待确认计划调用，传统管理器本身不依赖 Agent；后续是否增加独立下载页面不影响此边界。Nexus v3 是当前活跃 API；公开发行前需要按官方流程注册应用，开发期个人 API Key 只用于测试。Agent 不保存临时下载链接，不抓取网页代替 API，也不绕过会员或下载权限。

外部接口依据（核对于 2026-07-17）：

- [DeepSeek V4 模型与 Chat Completions](https://api-docs.deepseek.com/)
- [DeepSeek V4 工具调用](https://api-docs.deepseek.com/guides/tool_calls)
- [DeepSeek 思考模式和工具消息规则](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode)
- [DeepSeek API 更新记录](https://api-docs.deepseek.com/updates)
- [Nexus Mods API v3](https://api-docs.nexusmods.com/)
- [Nexus Mods API 使用政策与应用注册](https://help.nexusmods.com/article/114-api-acceptable-use-policy)

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
6. 解包后的目录继续走文件夹导入识别；若包含内嵌压缩包，则在隔离目录递归解包两层并合并候选。
7. 用户可选择一个或多个候选，编辑分支名并决定作为分支组或独立 MOD 导入；每个候选分别生成普通 MOD manifest。
8. manifest 的 `source_path` 记录原始压缩包路径，`content_root_path` 记录对应分支识别出的内容根。

当前没有新增 Rust 解包依赖。Acumod 采用随安装包分发解包组件的方式，避免要求用户另行安装 7-Zip；代价是发布包会增加几 MB，并且需要随包保留 7-Zip 许可文件。

第六个 MVP 切片是“启用和禁用已安装 MOD”：

1. Vue 在已安装 MOD 列表中提供启用、禁用入口。
2. `src/api/modLibrary.ts` 调用 `preview_enable_mod`、`enable_mod`、`preview_disable_mod` 和 `disable_mod`。
3. Rust service 读取已保存的 MHW 游戏目录，并再次校验 `MonsterHunterWorld.exe`。
4. 启用前生成部署计划，列出库内源文件、游戏目录目标文件、目标是否已存在，以及是否由 Acumod 记录为其他 MOD 部署。
5. 如果目标文件已存在且不是同一个 MOD 的已记录部署，前端必须确认后才调用真正启用。
6. 启用时从 `AcumodData/mods/installed/<mod_id>/content/` 复制文件到 MHW 游戏目录，并把 `deployedFiles` 写回 manifest。
7. 禁用按钮直接按实际 `deployedFiles` 记录删除游戏目录文件，然后清空部署记录并标记为未启用；不弹出确认框。

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
4. Rust `model_recognition` service 返回 `ModelReplacement`；该切片最初使用 schema 9，当前 manifest schema 17 继续持久化并兼容旧识别结果，模型识别规则版本保持 16；`vfx/mod` 附属资源不作为装备替换目标显示。
5. Vue 在导入结果和已安装 MOD 列表显示替换类型、模型 ID、游戏 ID 和游戏名称摘要。

这个切片只读取路径并展示识别结果，不修改 MOD 的模型目标或文件内容；Slice 14 的受控改绑建立在该识别结果上。

MVP 收尾补充了已安装 MOD 的完整文件列表、主列表冲突状态，以及禁用操作的 Rust 端文件预览。至此 `docs/features.md` 中的 MVP 完成标准已全部形成可操作 UI 和受控 Rust service 链路。
