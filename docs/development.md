# 开发说明

本文档记录 Acumod 当前开发命令、目录约定和 Tauri invoke 修改检查清单。

## 环境和命令

本项目在 Windows + PowerShell 环境下开发。npm 命令建议使用 `npm.cmd`，可以减少 PowerShell 下可执行文件解析问题。

常用命令：

```powershell
npm.cmd install
npm.cmd run dev
npm.cmd run typecheck
npm.cmd run build
npm.cmd run knowledge:audit
npm.cmd run knowledge:fetch-quest-unlocks
npm.cmd run knowledge:build-dev
npm.cmd run knowledge:build-modding-dev
npm.cmd run knowledge:package-modding-dev
npm.cmd run knowledge:package-bundle
npm.cmd run knowledge:verify-dev
npm.cmd run knowledge:verify-e2e
npm.cmd run tauri:dev
npm.cmd run tauri:build
```

Rust 检查：

```powershell
Set-Location src-tauri
cargo check
cargo fmt
```

如果只修改 Markdown 文档，不需要运行前端或 Rust 构建命令。

## npm scripts

当前 `package.json` 已确认脚本：

- `dev`：启动 Vite 开发服务器。
- `typecheck`：运行 `vue-tsc --noEmit`。
- `build`：先运行 typecheck，再运行 Vite build。
- `knowledge:audit`：只读审计现有 MHW 数据基线与本地 MOD 库文件分布，生成匿名聚合报告；缺少受限的 `15.10.00` 原始表时，会明确切换并报告 MHWData 本地快照输入。
- `knowledge:fetch-quest-unlocks`：显式联网抓取公开可选任务解锁页面，只保存任务名、来源链接和可结构化解析的条件到本地忽略快照。
- `knowledge:build-mhwdata`：把固定 commit 的 MHWorldData 快照和可用的同键简繁名称桥构建为 `mhwdata.acumhwdb`；保留所有源 CSV 行，不构建事实图谱或 FTS。
- `knowledge:build-dev`：构建上述数值数据库，以及 MOD 技术、攻略、Acumod 使用说明三个不提交的文本包，用于完整查询链路验证。
- `knowledge:build-modding-dev`：只生成 MOD 技术文本包；用于独立验证技术知识检索。
- `knowledge:package-modding-dev`：把技术开发包打成可从应用设置页安装的单包 ZIP。
- `knowledge:package-bundle`：将一个 `.acumhwdb` 和三个 `.acukb` 文本包打成独立 ZIP，供本地安装测试或 release 上传；生成物位于被忽略的 `references/knowledge/release/`。
- `knowledge:verify-dev`：校验固定数据库的版本、源表、原始行关联与文本包结构。
- `knowledge:verify-e2e`：旧的四 `.acukb` 图谱 e2e 脚本已废止；当前由 Rust 的 `mhwdata` 安装/查询集成测试覆盖相同安装边界。
- `preview`：预览前端构建结果。
- `tauri`：调用 Tauri CLI。
- `tauri:dev`：启动 Tauri 桌面开发模式。
- `tauri:build`：构建 Tauri 桌面应用。
- 系统文件选择器使用 Tauri 官方 `dialog` 插件；前端只接收用户主动选择的文件或目录路径，Rust 仍负责最终扩展名、文件类型和知识包内容校验。

## 目录约定

当前目录：

```text
src/
  App.vue
  main.ts
  api/
    app.ts

src-tauri/
  tauri.conf.json
  capabilities/
    default.json
  src/
    lib.rs
    main.rs
    commands/
      mod.rs
      app.rs
```

后续功能扩展时建议：

- 前端 Tauri 调用统一放在 `src/api/`。
- Vue 页面级文件后续放在 `src/views/`。
- 可复用组件后续放在 `src/components/`。
- Rust Tauri command 放在 `src-tauri/src/commands/`。
- Rust 业务逻辑后续放在 `src-tauri/src/services/`。
- 配置、MOD 元数据、启用状态、排序和部署记录读写后续放在 `src-tauri/src/storage/`。
- 文本知识包运行时服务在 `src-tauri/src/services/knowledge.rs`，固定游戏数值库在 `src-tauri/src/services/mhwdata.rs`，MOD 文件分析在 `src-tauri/src/services/mod_analysis/`；三者不放进 DeepSeek client 或工具分发文件。
- 可重复的知识数据清洗、校验和打包脚本放在 `scripts/knowledge/`；开发源数据、来源清单和人工核对记录放在 `references/knowledge/`，生成的 `.acukb`/`.acumhwdb` 不作为主程序资源打包。
- 知识审计默认读取开发环境的 `src-tauri/target/debug/AcumodData/mods/installed`，也可使用 `npm.cmd run knowledge:audit -- --mod-root <目录>` 指定本地 MOD 库。报告不得记录 MOD 名称、MOD ID、绝对路径或文件名。

## Tauri invoke 检查清单

修改或新增前后端命令时，必须同步检查：

1. 前端 `invoke()` 的命令名。
2. 前端传入参数名。
3. 前端 TypeScript 返回类型。
4. Rust `#[tauri::command]` 函数名。
5. Rust 参数名和类型。
6. Rust 返回 DTO 是否实现 `serde::Serialize`。
7. 需要接收前端参数时，参数类型是否实现 `serde::Deserialize`。
8. `src-tauri/src/lib.rs` 的 `tauri::generate_handler![...]` 是否注册命令。

## 后台任务检查清单

涉及目录扫描、解包、批量复制、删除或全库 manifest 读取时：

1. command 使用 `async fn`，并通过 `operations::run_blocking_operation` 调用同步文件 service。
2. service 在已知文件总数后通过 `OperationReporter` 上报真实完成数；未知总数的扫描只上报阶段和当前项。
3. 前端不要重复调用列表、分类、冲突三个全量读取 command；启动以及改变部署状态的操作完成后使用 `getModWorkspaceSnapshot()` 读取缓存，名称、备注和分类编辑直接使用 command 返回值局部更新。只有用户主动刷新时使用 `refreshModWorkspaceSnapshot()` 强制全量重建。
4. 任务事件统一为 `acumod://operation-progress`，由 `OperationStatusBar` 展示；不要为单个功能另造加载浮层或取消按钮。
5. 完成后检查软件目录旁的 `AcumodData/logs/operation-timings.log` 是否写入结果和耗时。日志不能记录凭据、令牌或文件内容。
6. 新增会修改 manifest、分类、冲突顺序或浏览顺序的 command 时，必须同步更新工作区快照；禁止在操作完成后用全量刷新代替快照一致性维护。

当前已确认示例：

```text
src/api/app.ts
  getAppInfo()
  -> invoke<AppInfo>("get_app_info")

src-tauri/src/commands/app.rs
  #[tauri::command]
  get_app_info() -> AppInfo

src-tauri/src/lib.rs
  tauri::generate_handler![commands::app::get_app_info]
```

当前游戏目录切片：

```text
src/api/game.ts
  getGameDirectoryStatus()
  -> invoke<GameDirectoryStatus>("get_game_directory_status")

  detectGameDirectory()
  -> invoke<GameDirectoryStatus>("detect_game_directory")

  saveGameDirectory(path)
  -> invoke<GameDirectoryStatus>("save_game_directory", { path })

src-tauri/src/commands/game.rs
  #[tauri::command]
  get_game_directory_status(app) -> Result<GameDirectoryStatus, String>
  detect_game_directory(app) -> Result<GameDirectoryStatus, String>
  save_game_directory(app, path) -> Result<GameDirectoryStatus, String>
```

当前 MOD 导入与状态同步调用链：

显式接管原型中的 UI、typed wrapper、DTO 和 Rust command 已成组移除。当前调用链如下：

```text
src/api/modLibrary.ts
  getModLibraryStatus()
  -> invoke<ModLibraryStatus>("get_mod_library_status")

  listInstalledMods()
  -> invoke<InstalledModList>("list_installed_mods")

  getModWorkspaceSnapshot()
  -> invoke<ModWorkspaceSnapshot>("get_mod_workspace_snapshot")

  refreshModWorkspaceSnapshot()
  -> invoke<ModWorkspaceSnapshot>("refresh_mod_workspace_snapshot")

  previewModImport(path, allowGameRoot)
  -> invoke<ModImportPreview>("preview_mod_import", { path, allowGameRoot })

  installModFromFolder(path, allowGameRoot)
  -> invoke<ModInstallResult>("install_mod_from_folder", { path, allowGameRoot })

  installModFromArchive(path, allowGameRoot)
  -> invoke<ModArchiveImportOutcome>("install_mod_from_archive", { path, allowGameRoot })

  installModFromCandidate(sourcePath, candidateRootPath, originalArchivePath)
  -> invoke<ModInstallResult>("install_mod_from_candidate", { sourcePath, candidateRootPath, originalArchivePath })

  scanLegacyBoxMods(boxPath)
  -> invoke<LegacyBoxScan>("scan_legacy_box_mods", { boxPath })

  importLegacyBoxMods(boxPath, moduleIds)
  -> invoke<LegacyBoxImportResult>("import_legacy_box_mods", { boxPath, moduleIds })
  -> 导入完成后由 Rust 自动执行状态同步，结果写入 LegacyBoxImportResult.stateSync

  refreshGameModStates()
  -> invoke<ModStateSyncResult>("refresh_game_mod_states")

  previewEnableMod(modId)
  -> invoke<ModDeploymentPlan>("preview_enable_mod", { modId })

  enableMod(modId, confirmOverwrite)
  -> invoke<ModDeploymentResult>("enable_mod", { modId, confirmOverwrite })

  disableMod(modId)
  -> invoke<ModDeploymentResult>("disable_mod", { modId })

  batchUpdateMods(action, modIds)
  -> invoke<BatchModOperationResult>("batch_update_mods", { action, modIds })

  previewUninstallMod(modId)
  -> invoke<ModUninstallPlan>("preview_uninstall_mod", { modId })

  uninstallMod(modId)
  -> invoke<ModUninstallResult>("uninstall_mod", { modId })

  previewRestoreAllMods()
  -> invoke<RestoreAllPlan>("preview_restore_all_mods")

  restoreAllMods()
  -> invoke<RestoreAllResult>("restore_all_mods")

  getModConflictReport()
  -> invoke<ModConflictReport>("get_mod_conflict_report")

  moveConflictParticipant(groupId, modId, direction)
  -> invoke<ModConflictMoveResult>("move_conflict_participant", { groupId, modId, direction })

  previewApplyConflictOrder(groupId)
  -> invoke<ApplyConflictOrderPlan>("preview_apply_conflict_order", { groupId })

  applyConflictOrder(groupId, confirmOverwrite)
  -> invoke<ApplyConflictOrderResult>("apply_conflict_order", { groupId, confirmOverwrite })

src-tauri/src/commands/mod_library.rs
  #[tauri::command]
  get_mod_library_status(app) -> Result<ModLibraryStatus, String>
  list_installed_mods(app) -> Result<InstalledModList, String>
  preview_mod_import(path, allow_game_root) -> Result<ModImportPreview, String>
  install_mod_from_folder(app, path, allow_game_root) -> Result<ModInstallResult, String>
  install_mod_from_archive(app, path, allow_game_root) -> Result<ModArchiveImportOutcome, String>

  install_mod_from_candidate(app, source_path, candidate_root_path, original_archive_path) -> Result<ModInstallResult, String>

  scan_legacy_box_mods(app, box_path) -> Result<LegacyBoxScan, String>

  import_legacy_box_mods(app, box_path, module_ids) -> Result<LegacyBoxImportResult, String>

  refresh_game_mod_states(app) -> Result<ModStateSyncResult, String>

  preview_enable_mod(app, mod_id) -> Result<ModDeploymentPlan, String>

  enable_mod(app, mod_id, confirm_overwrite) -> Result<ModDeploymentResult, String>

  disable_mod(app, mod_id) -> Result<ModDeploymentResult, String>

  batch_update_mods(app, action, mod_ids) -> Result<BatchModOperationResult, String>

  preview_uninstall_mod(app, mod_id) -> Result<ModUninstallPlan, String>

  uninstall_mod(app, mod_id) -> Result<ModUninstallResult, String>

  preview_restore_all_mods(app) -> Result<RestoreAllPlan, String>

  restore_all_mods(app) -> Result<RestoreAllResult, String>

  get_mod_conflict_report(app) -> Result<ModConflictReport, String>

  move_conflict_participant(app, group_id, mod_id, direction) -> Result<ModConflictMoveResult, String>

  preview_apply_conflict_order(app, group_id) -> Result<ApplyConflictOrderPlan, String>

  apply_conflict_order(app, group_id, confirm_overwrite) -> Result<ApplyConflictOrderResult, String>

src-tauri/src/services/mod_library.rs
  创建 MOD 库目录
  MOD 库位于软件目录旁的 AcumodData/，不放入 AppData
  识别 nativePC、nativePC 内部目录、多候选目录和游戏根目录确认 fallback
  将 ready 状态的文件夹 MOD 复制到 AcumodData/mods/installed/<mod_id>/content/
  写入 manifest.json，记录来源、识别方式、部署相对路径和启用状态
  读取 installed/*/manifest.json 生成已安装 MOD 列表
  使用 Acumod 内置 7-Zip 解包组件解包 .zip/.7z/.rar，再复用文件夹导入逻辑
  多候选时重新校验并只导入用户选择的一个内容根
  盒子导入完成后调用 mod_state_sync service；完全匹配自动启用，可完整解释的部分匹配自动启用并纳入冲突顺序，其余自动保持未启用
  观察所得部署记录删除前重新比较本地库有效内容；外部改动、归属不明或仍有等价提供者时保留文件
  调用 model_recognition service，将路径识别和 EVAM 关联结果写入当前 manifest schema 16
  调用 model_remap service，校验并保存五类改绑选择，生成有效部署文件、MRL3 贴图路径修正和 EVAM 飞翔爪绑定修正
  启用 MOD 前生成部署计划，确认覆盖后复制到 MHW 游戏目录，并把 deployedFiles 写回 manifest
  禁用 MOD 时只删除 manifest 中记录过的 deployedFiles
  批量启用、禁用和卸载在一个后台任务内顺序复用单项核心函数；单项失败写入结果并继续后续项目
  卸载 MOD 时先预览，再清理已记录部署文件，最后删除 Acumod 本地库中的该 MOD 目录
  一键还原时扫描本地 MOD 库 manifest，清理所有记录过的 deployedFiles，并将相关 MOD 标记为未启用
  扫描 deployRelativePath 构建 MOD 冲突关系图，将每个独立冲突组的优先级保存到 conflict-orders.json，并应用组内全部冲突文件；最上方项目最终覆盖

src-tauri/src/services/model_recognition.rs
  读取编译进应用的 references/mhwi-data/curated/model-index.json
  识别武器模型路径、防具模型 ID 与部位标记、发型路径和投射器/飞翔爪资源
  严格解析防具手臂 EVAM，并仅在匹配的飞翔爪模型同时存在时附加关联防具
  返回 ModelReplacement DTO；不修改 MOD 文件或部署路径

src-tauri/src/services/model_remap.rs
  读取同一份精简索引中的可选目标
  仅支持武器、防具、随从防具、投射器和玩家发型
  根据 manifest.modelRemaps 以类别化路径规则生成有效部署路径；防具规范 EPV 文件按三位套装号重命名，角色套装表只对已核实的固定性别目标强制专用资源根和投射器绑定，人物语音始终只读
```

已新增 `src-tauri/src/services/mod_state_sync.rs`，避免继续扩大 `mod_library.rs`。职责边界：

- `legacy_box.rs`：解析第三方盒子结构，只返回来源模块和文件。
- `mod_library.rs`：安装本地副本、读取和写入 manifest、使用有效部署计划比较文件、执行既有启停与冲突操作。
- `mod_state_sync.rs`：只根据逐文件比对事实推导状态与冲突有向图，返回不写文件的 `ModStateSyncPlan`。
- `commands/mod_library.rs`：通过后台任务调用状态同步，再让 `mod_library` 一次提交 manifest 和冲突顺序。

`ModStateSyncResult` 对前端只暴露简化结果：

```text
enabledModCount
partiallyOverriddenModCount
disabledModCount
mixedConflictGroupCount
mods[]:
  modId
  enabled
  partiallyOverridden
  message
warnings[]
```

内部逐文件匹配类别不作为 MOD 状态暴露给前端，只用于算法和可展开诊断。

状态同步测试矩阵至少覆盖：

1. 单个 MOD 全部文件匹配，自动启用。
2. 单个 MOD 部分匹配且存在缺失文件，保持未启用。
3. B 完全匹配、A 其余文件全部被 B 覆盖，A 标记为已启用且部分被覆盖，顺序为 `B -> A`。
4. `C -> B -> A` 多层覆盖链通过迭代分析稳定建立。
5. A、B 同路径且内容相同，不产生冲突边。
6. A、B 在不同路径互相覆盖形成环，两者保持未启用并报告一个混合冲突组。
7. MOD 完全无匹配，即使所有路径都有其它提供者，仍保持未启用。
8. 已由 Acumod 管理的启用 MOD 作为可信提供者，但内容漂移时不能解释新 MOD 的覆盖。
9. EVAM 和 MRL3 改绑后的有效内容使用部署转换结果比较。
10. 观察所得文件被外部修改后，禁用和卸载均保留目标文件。

当前传统管理器增强命令：

```text
src/api/modLibrary.ts
  updateModMetadata(modId, patch)
  -> update_mod_metadata

  listModCategories()
  createModCategory(name)
  renameModCategory(categoryId, name)
  deleteModCategory(categoryId)
  -> 对应 list/create/rename/delete_mod_category

src-tauri/src/commands/mod_library.rs
  只转发参数与 DTO，不在 command 中写文件逻辑

src-tauri/src/services/mod_library.rs
  写 manifest 元数据、两级分类与 MOD 库手动排序；manifest 的 enabled/deployedFiles 和 conflict-orders.json 共同维护当前唯一的部署状态，mod-library-order.json 仅维护浏览顺序
```

## 薄端到端切片

新增能力时优先做小而完整的链路，而不是先写一大片底层代码。

推荐模板：

```text
Vue UI
  -> src/api/* typed wrapper
  -> Tauri command
  -> Rust service
  -> DTO response
  -> Vue UI 状态展示
```

例如“检测 MHW 游戏目录”：

1. Vue 页面触发检测。
2. `src/api/game.ts` 调用 `invoke<GameDirectoryStatus>("save_game_directory", { path })`。
3. Rust command 接收 `path`。
4. Rust service 检查路径。
5. 返回 DTO。
6. Vue 显示成功、失败和原因。

这个方式适合学习和验证，因为每次都能看到完整的数据流。

例如“预览 MOD 导入目录”：

1. Vue 调用 `previewImportPath(false)`。
2. `src/api/modLibrary.ts` 调用 `invoke<ModImportPreview>("preview_mod_import", { path, allowGameRoot })`。
3. Rust command 接收 `path` 和 `allow_game_root`。
4. Rust service 扫描目录并生成部署路径预览。
5. 如果需要游戏根目录 fallback，Vue 先提示用户确认，再调用 `previewImportPath(true)`。

例如“导入文件夹 MOD 到本地库”：

1. Vue 在预览 `ready` 后调用 `installPreviewedMod()`。
2. `src/api/modLibrary.ts` 调用 `invoke<ModInstallResult>("install_mod_from_folder", { path, allowGameRoot })`。
3. Rust command 接收 `path` 和 `allow_game_root`。
4. Rust service 复用预览识别规则，确认 MOD 内容根。
5. Rust service 将文件复制到 `AcumodData/mods/installed/<mod_id>/content/`。
6. Rust service 写入 `manifest.json`。
7. Vue 显示 MOD ID、库内目录、manifest 路径和文件数。

例如“显示已安装 MOD 列表”：

1. Vue 挂载后调用 `loadInstalledMods()`。
2. `src/api/modLibrary.ts` 调用 `invoke<InstalledModList>("list_installed_mods")`。
3. Rust service 扫描 `AcumodData/mods/installed/`。
4. Rust service 读取每个 `manifest.json`。
5. Vue 显示 MOD 名称、ID、文件数、启用状态和部署根。

例如“打开已安装 MOD 文件夹”：

1. Vue 在对应 MOD 行点击“打开文件夹”。
2. `src/api/modLibrary.ts` 调用 `invoke<void>("open_installed_mod_folder", { modId })`。
3. Rust command 校验 `modId` 并读取该 MOD 的 manifest，不接受前端直接传入任意文件系统路径。
4. Rust service 通过已启用的 opener plugin 在系统资源管理器打开 `installed/<mod_id>/content/`。

例如“导入压缩包 MOD”：

1. Vue 调用 `installArchive()`。
2. `src/api/modLibrary.ts` 调用 `invoke<ModArchiveImportOutcome>("install_mod_from_archive", { path, allowGameRoot })`。
3. Rust service 校验 `.zip/.7z/.rar` 扩展名。
4. Rust service 调用 Acumod 内置 7-Zip 解包组件，解包到 `AcumodData/mods/staging/imports/`。
5. Rust service 复用文件夹导入识别和本地安装逻辑。
6. 如果返回 `ambiguous`，Vue 显示候选列表，再调用 `install_mod_from_candidate` 导入所选分支；否则直接刷新已安装 MOD 列表。
7. 成功安装后删除解压暂存；多分支压缩包只把所选候选复制到 `installed/<mod_id>/content/`。

例如“识别模型替换目标”：

1. 导入 service 根据最终 `deployRelativePath` 和库内 `.evam` 文件调用内容感知识别入口。
2. Rust 查询编译进应用的武器、防具、发型、随从装备、猎虫、挂件、NPC、投射器/飞翔爪和人物语音精简索引；投射器未核实名称保留资源 ID，不按防具同号推断。
3. 只有实际存在匹配 `wp/slg` 模型时，严格通过格式校验的 `.evam` 才作为该飞翔爪的关联防具写入 `associations`；孤立 `.evam` 不生成识别结果。
4. 结果随 `ModInstallResult` 返回并写入当前 manifest schema 16。
5. `list_installed_mods` 读取 schema 1 至 12 的旧 manifest 时结合库内文件内容重算识别结果；武器和防具只在规范资源根目录识别，`vfx/mod` 中的同名模型只作为附属资源保留。
6. Vue 展示模型类型、子类型、模型 ID、游戏名称和可选关联防具摘要。

例如“修改模型替换目标”：

1. Vue 调用 `get_mod_remap_details`，只显示后端判定为可改绑的五类分组和同类型目标。
2. 用户选择目标并点击保存后，Vue 在内部调用 `preview_mod_remap`；Rust 校验原路径、有效路径、MRL3 修正、EVAM 绑定、DAT 型防具的逐部位映射和碰撞，不写 manifest。DAT 的来源/外观 ID、部位主模型号和核心文件均吻合时，或其它来源槽位已别名到当前资源目录时，预览会提示有效部署将排除 `armor.am_dat`；选择角色套装时还会说明固定或保留的资源性别、已验证投射器，以及未自动处理的脸、头发、语音边界；前端只显示必要警告，不展示技术统计。
3. 校验通过后调用 `apply_mod_remap`；Rust 再次校验 MOD 未启用、目标类型和路径碰撞，然后把选择写入当前 schema 16 manifest。
4. `preview_enable_mod`、`enable_mod`、冲突检测和冲突顺序应用不再直接使用原始 `files[].deployRelativePath`，而是统一调用有效部署文件生成器。
5. 部署 `.mrl3` 时只重写精确命中的已移动贴图资源路径；飞翔爪改绑只修改已关联 `.evam` 的绑定字段；DAT 型防具只改写有效部署路径并自动跳过全局 DAT；本地库原文件保持不变。

例如“外部来源页面与本地导入”：

1. `search_mod_sources` 只返回已验证的具体来源页面，统一由系统浏览器打开。
2. 应用不提交或保存站点 API Key、会员状态、文件 ID、一次性下载链接或 `nxm` 参数。
3. 用户下载归档后，通过现有 `install_mod_from_archive` 预览链路导入；多候选和游戏根目录 fallback 仍由现有 UI 确认。

重新生成模型索引：

```powershell
.\scripts\build-mhwi-model-index.ps1
```

外观装备菜单录屏抄录或防具模型索引更新后，重新关联并生成程序使用的防具顺序：

```powershell
npm run data:armor-menu-order
```

当前压缩包导入不新增 Rust 依赖，但开发和发布包中需要提供 `resources/unpackers/7zip/7z.exe`、`7z.dll` 和 7-Zip 许可文件。用户不需要单独安装 7-Zip。

例如“启用和禁用已安装 MOD”：

1. Vue 在已安装 MOD 列表中点击启用。
2. `src/api/modLibrary.ts` 调用 `preview_enable_mod` 获取部署计划。
3. Rust service 根据 manifest 文件列表生成 `源文件 -> MHW 目标文件` 映射，并检查目标是否已存在。
4. 如果需要覆盖确认，Vue 弹出确认，再调用 `enable_mod`。
5. Rust service 将库内文件复制到 MHW 游戏目录，并把 `deployedFiles` 写回 manifest。
6. 禁用时 Vue 先调用 `preview_disable_mod` 展示记录过的部署文件，确认后再调用 `disable_mod`。

例如“卸载已安装 MOD”：

1. Vue 在已安装 MOD 列表中点击卸载。
2. `src/api/modLibrary.ts` 调用 `preview_uninstall_mod` 获取卸载预览。
3. Rust service 返回库内文件数量、已记录部署文件数量和是否仍启用。
4. Vue 展示确认提示。
5. 用户确认后调用 `uninstall_mod`。
6. Rust service 先清理该 MOD 的 `deployedFiles`，再删除 `AcumodData/mods/installed/<mod_id>/`。

例如“一键还原纯净状态”：

1. Vue 点击一键还原。
2. `src/api/modLibrary.ts` 调用 `preview_restore_all_mods` 获取还原预览。
3. Rust service 扫描本地 MOD 库 manifest，统计仍启用或仍有部署记录的 MOD。
4. Vue 展示确认提示。
5. 用户确认后调用 `restore_all_mods`。
6. Rust service 删除所有 manifest 中记录过的部署文件，清理空目录，并把相关 MOD 标记为未启用。

例如“冲突检测和排序”：

1. Vue 读取已安装 MOD 后调用 `get_mod_conflict_report`，主列表只显示普通序号；手动拖拽排序调用 `move_mod_library_item` 并只写 `mod-library-order.json`。
2. Rust service 在刷新时扫描所有 manifest 中的 `deployRelativePath`，将直接或间接相互冲突的 MOD 聚合为独立冲突组并写入工作区快照。
3. 用户打开冲突管理界面并选择一个 MOD 组；Vue 调用 `move_conflict_participant` 上移或下移组内 MOD。列表从上到下为优先级，第一项是最终覆盖者。
4. 排序移动由前端提交当前组的完整 ID 顺序，Rust 只更新 `conflict-orders.json` 中这个组合及工作区快照对应组，不重新读取全部 manifest，也不立即写入 MHW 游戏目录。
5. 用户点击应用此组顺序后，Vue 先调用 `preview_apply_conflict_order`。
6. 用户确认后调用 `apply_conflict_order`，Rust service 遍历组内全部冲突文件，按组优先级选择各文件的最终提供者并更新部署记录。
7. 冲突报告只使用已启用 MOD；`enable_mod` 成功后把本次启用的 MOD 放到相关组最上方，再应用组优先级。

Windows 上保留 Tauri Webview 的原生文件拖入，以便接收文件系统路径并走现有导入确认流程；MOD 库排序使用序号列拖拽把手的 Pointer Events 实现，不使用 HTML5 drag-and-drop。两者不能混用，因为 Tauri 原生 `dragDropEnabled` 与 Windows 的 HTML5 拖放互斥。

## 验证标准

按修改类型选择验证：

- 只改 Markdown：阅读生成文件，确认链接、标题、术语一致。
- 改 Vue/TypeScript：运行 `npm.cmd run build`。
- 改 Tauri/Rust：运行 `cd src-tauri` 后的 `cargo check` 和 `cargo fmt`。
- 改前后端通信：同时运行前端 build 和 Rust check，并手动启动 `npm.cmd run tauri:dev` 验证窗口行为。
- 改知识数据或构建脚本：运行对应 ETL、schema/SQLite 完整性、行数、重复 ID、外键、官方简繁覆盖检查；同时记录生成包的版本、SHA-256、压缩大小与安装大小。

验证失败时，必须保留错误信息并先说明失败位置，再决定下一步。
