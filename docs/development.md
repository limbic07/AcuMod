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
- `preview`：预览前端构建结果。
- `tauri`：调用 Tauri CLI。
- `tauri:dev`：启动 Tauri 桌面开发模式。
- `tauri:build`：构建 Tauri 桌面应用。

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

当前 MOD 导入识别切片：

```text
src/api/modLibrary.ts
  getModLibraryStatus()
  -> invoke<ModLibraryStatus>("get_mod_library_status")

  listInstalledMods()
  -> invoke<InstalledModList>("list_installed_mods")

  previewModImport(path, allowGameRoot)
  -> invoke<ModImportPreview>("preview_mod_import", { path, allowGameRoot })

  installModFromFolder(path, allowGameRoot)
  -> invoke<ModInstallResult>("install_mod_from_folder", { path, allowGameRoot })

  installModFromArchive(path, allowGameRoot)
  -> invoke<ModArchiveImportOutcome>("install_mod_from_archive", { path, allowGameRoot })

  installModFromCandidate(sourcePath, candidateRootPath, originalArchivePath)
  -> invoke<ModInstallResult>("install_mod_from_candidate", { sourcePath, candidateRootPath, originalArchivePath })

  previewEnableMod(modId)
  -> invoke<ModDeploymentPlan>("preview_enable_mod", { modId })

  enableMod(modId, confirmOverwrite)
  -> invoke<ModDeploymentResult>("enable_mod", { modId, confirmOverwrite })

  disableMod(modId)
  -> invoke<ModDeploymentResult>("disable_mod", { modId })

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

  preview_enable_mod(app, mod_id) -> Result<ModDeploymentPlan, String>

  enable_mod(app, mod_id, confirm_overwrite) -> Result<ModDeploymentResult, String>

  disable_mod(app, mod_id) -> Result<ModDeploymentResult, String>

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
  调用 model_recognition service，将路径识别和 EVAM 关联结果写入当前 manifest schema 14
  调用 model_remap service，校验并保存五类改绑选择，生成有效部署文件、MRL3 贴图路径修正和 EVAM 飞翔爪绑定修正
  启用 MOD 前生成部署计划，确认覆盖后复制到 MHW 游戏目录，并把 deployedFiles 写回 manifest
  禁用 MOD 时只删除 manifest 中记录过的 deployedFiles
  卸载 MOD 时先预览，再清理已记录部署文件，最后删除 Acumod 本地库中的该 MOD 目录
  一键还原时扫描本地 MOD 库 manifest，清理所有记录过的 deployedFiles，并将相关 MOD 标记为未启用
  扫描 deployRelativePath 构建 MOD 冲突关系图，将每个独立冲突组的整体顺序保存到 conflict-orders.json，并应用组内全部冲突文件

src-tauri/src/services/model_recognition.rs
  读取编译进应用的 references/mhwi-data/curated/model-index.json
  识别武器模型路径、防具模型 ID 与部位标记、发型路径和投射器/飞翔爪资源
  严格解析防具手臂 EVAM，并仅在匹配的飞翔爪模型同时存在时附加关联防具
  返回 ModelReplacement DTO；不修改 MOD 文件或部署路径

src-tauri/src/services/model_remap.rs
  读取同一份精简索引中的可选目标
  仅支持武器、防具、随从防具、投射器和玩家发型
  根据 manifest.modelRemaps 以类别化路径规则生成有效部署路径；防具规范 EPV 文件按三位套装号重命名，人物语音始终只读
```

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
  写 manifest 元数据与统一多分类；manifest 的 enabled/deployedFiles 和 conflict-orders.json 共同维护当前唯一的部署状态
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
4. 结果随 `ModInstallResult` 返回并写入当前 manifest schema 14。
5. `list_installed_mods` 读取 schema 1 至 12 的旧 manifest 时结合库内文件内容重算识别结果；武器和防具只在规范资源根目录识别，`vfx/mod` 中的同名模型只作为附属资源保留。
6. Vue 展示模型类型、子类型、模型 ID、游戏名称和可选关联防具摘要。

例如“修改模型替换目标”：

1. Vue 调用 `get_mod_remap_details`，只显示后端判定为可改绑的五类分组和同类型目标。
2. 用户选择目标并点击保存后，Vue 在内部调用 `preview_mod_remap`；Rust 校验原路径、有效路径、MRL3 修正、EVAM 绑定和碰撞，不写 manifest。前端只显示必要警告，不展示技术统计。
3. 校验通过后调用 `apply_mod_remap`；Rust 再次校验 MOD 未启用、目标类型和路径碰撞，然后把选择写入当前 schema 14 manifest。
4. `preview_enable_mod`、`enable_mod`、冲突检测和冲突顺序应用不再直接使用原始 `files[].deployRelativePath`，而是统一调用有效部署文件生成器。
5. 部署 `.mrl3` 时只重写精确命中的已移动贴图资源路径；飞翔爪改绑只修改已关联 `.evam` 的绑定字段；本地库原文件保持不变。

例如“Nexus 下载并进入导入预览”（Slice 15 计划）：

1. Vue 只提交 `monsterhunterworld` 的 Nexus mod/file ID 或后端解析过的页面 URL，不提交任意下载地址。
2. `src/api/nexus.ts` 调用 `commands/nexus.rs`；Rust 从系统凭据存储读取授权，并通过 Nexus 适配层获取元数据、文件列表或一次性下载链接。
3. Rust 创建 `staging/downloads/<task_id>/`，流式写入 `.part`，持续返回进度、剩余 API 配额和可取消状态。
4. 下载完成后校验文件大小和可用哈希，再原子改名为归档文件；失败或取消只清理该 task ID 下由 Acumod 创建的文件。
5. 用户点击“继续导入”后，把已完成归档交给现有 `install_mod_from_archive` 预览链路；多候选和游戏根目录 fallback 仍由现有 UI 确认。
6. 成功安装后 manifest 记录可选 `nexusSource`，临时下载链接和 API key 永不写入 manifest。

重新生成模型索引：

```powershell
.\scripts\build-mhwi-model-index.ps1
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

1. Vue 读取已安装 MOD 后调用 `get_mod_conflict_report`，主列表只显示普通序号。
2. Rust service 扫描所有 manifest 中的 `deployRelativePath`，将直接或间接相互冲突的 MOD 聚合为独立冲突组。
3. 用户打开冲突管理界面并选择一个 MOD 组；Vue 调用 `move_conflict_participant` 上移或下移组内 MOD。
4. 排序移动只更新 `conflict-orders.json` 中这个 MOD 组合的顺序，不立即写入 MHW 游戏目录。
5. 用户点击应用此组顺序后，Vue 先调用 `preview_apply_conflict_order`。
6. 用户确认后调用 `apply_conflict_order`，Rust service 遍历组内全部冲突文件，按组顺序选择各文件的最终提供者并更新部署记录。
7. 冲突报告只使用已启用 MOD；`enable_mod` 成功后把本次启用的 MOD 追加到相关组末尾，再应用组顺序。

## 验证标准

按修改类型选择验证：

- 只改 Markdown：阅读生成文件，确认链接、标题、术语一致。
- 改 Vue/TypeScript：运行 `npm.cmd run build`。
- 改 Tauri/Rust：运行 `cd src-tauri` 后的 `cargo check` 和 `cargo fmt`。
- 改前后端通信：同时运行前端 build 和 Rust check，并手动启动 `npm.cmd run tauri:dev` 验证窗口行为。

验证失败时，必须保留错误信息并先说明失败位置，再决定下一步。
