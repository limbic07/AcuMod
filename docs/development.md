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

  previewModImport(path, allowGameRoot)
  -> invoke<ModImportPreview>("preview_mod_import", { path, allowGameRoot })

  installModFromFolder(path, allowGameRoot)
  -> invoke<ModInstallResult>("install_mod_from_folder", { path, allowGameRoot })

src-tauri/src/commands/mod_library.rs
  #[tauri::command]
  get_mod_library_status(app) -> Result<ModLibraryStatus, String>
  preview_mod_import(path, allow_game_root) -> Result<ModImportPreview, String>
  install_mod_from_folder(app, path, allow_game_root) -> Result<ModInstallResult, String>

src-tauri/src/services/mod_library.rs
  创建 MOD 库目录
  MOD 库位于软件目录旁的 AcumodData/，不放入 AppData
  识别 nativePC、nativePC 内部目录、多候选目录和游戏根目录确认 fallback
  将 ready 状态的文件夹 MOD 复制到 AcumodData/mods/installed/<mod_id>/content/
  写入 manifest.json，记录来源、识别方式、部署相对路径和启用状态
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

## 验证标准

按修改类型选择验证：

- 只改 Markdown：阅读生成文件，确认链接、标题、术语一致。
- 改 Vue/TypeScript：运行 `npm.cmd run build`。
- 改 Tauri/Rust：运行 `cd src-tauri` 后的 `cargo check` 和 `cargo fmt`。
- 改前后端通信：同时运行前端 build 和 Rust check，并手动启动 `npm.cmd run tauri:dev` 验证窗口行为。

验证失败时，必须保留错误信息并先说明失败位置，再决定下一步。
