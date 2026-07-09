# Acumod 文档入口

本目录记录 Acumen MOD manager 的产品范围、技术架构、开发方式和安全边界。文档优先服务两件事：

1. 让后续开发前能快速确认“现在项目到底怎么运转”。
2. 把尚未确定的技术细节显式列出来，避免在实现时临时猜测。

## 当前阶段

项目当前处于 Tauri 桌面应用的 MVP 起步阶段，已经具备多条端到端链路：

`Vue UI -> src/api typed invoke wrapper -> Tauri command -> Rust DTO -> Vue UI`

当前已验证的示例包括：

- 前端入口：`src/App.vue`
- 前端调用封装：`src/api/app.ts`
- Rust 命令：`src-tauri/src/commands/app.rs`
- 命令注册：`src-tauri/src/lib.rs`
- 游戏目录调用封装：`src/api/game.ts`
- 游戏目录命令：`src-tauri/src/commands/game.rs`
- 游戏目录 service：`src-tauri/src/services/game.rs`
- JSON 配置存储：`src-tauri/src/storage/config.rs`
- MOD 库调用封装：`src/api/modLibrary.ts`
- MOD 库、导入预览和本地安装命令：`src-tauri/src/commands/mod_library.rs`
- MOD 库、导入预览和本地安装 service：`src-tauri/src/services/mod_library.rs`

当前第三个切片已经支持把文件夹 MOD 导入到 Acumod 本地 MOD 库。该切片复用导入路径预览规则，把文件复制到 `AcumodData/mods/installed/<mod_id>/content/`，并写入 `manifest.json`；暂时仍不写入 MHW 游戏目录。

## 文档阅读顺序

1. `docs/README.md`：文档入口、当前阶段、已确认技术事实。
2. `docs/architecture.md`：前端、Tauri command、Rust service、DTO 和 AI 边界。
3. `docs/features.md`：MVP 范围、传统 MOD 管理器、下载和 AI Agent 的功能拆分。
4. `docs/development.md`：开发命令、目录约定、端到端切片检查清单。
5. `docs/security.md`：文件操作安全、用户确认机制、AI 执行边界。

## 已确认技术事实

- 应用名称：Acumod。
- 项目包名：`acumod`。
- 当前版本：`0.1.0`。
- 桌面框架：Tauri 2。
- 前端：Vue 3 + TypeScript + Vite。
- 脚本和包管理：npm。
- 后端：Rust 2021 edition。
- 前后端通信：`@tauri-apps/api/core` 的 `invoke()` 调用 Rust `#[tauri::command]`。
- 序列化：Rust 使用 `serde` / `serde_json` 返回结构化 DTO。
- 当前插件：`tauri-plugin-opener` / `@tauri-apps/plugin-opener`。
- Tauri identifier：`com.acumod.app`。
- 开发端口：Vite 固定使用 `1420`，Tauri devUrl 为 `http://localhost:1420`。
- MVP 存储策略：先使用 JSON 文件保存配置、MOD 元数据、启用状态、排序和部署记录；后续需要复杂查询时再考虑 SQLite。
- 存储位置：`AppData` 只保存 `config.json` 等小配置；MOD 库、导入暂存和后续备份放在软件目录旁的 `AcumodData/` 下。
- MOD 部署方式：只使用复制式部署。安装时在 Acumod 软件目录保存一份 MOD；启用时复制到 MHW 游戏目录；禁用时删除游戏目录中的已部署副本；卸载 MOD 时删除软件目录中的 MOD 副本。
- MOD 导入方式：第一版必须支持文件夹导入，以及 `rar`、`zip`、`7z` 压缩包导入。
- MOD 导入识别：优先识别 `nativePC`；缺少 `nativePC` 但出现常见 nativePC 内部目录时自动补成 `nativePC/...`；用户直接选择 `plugins`、`weapon` 等内部目录时保留目录名并映射到 `nativePC/<目录名>/...`；仍无法识别时允许用户确认按游戏根目录相对路径导入。
- MHW 模型替换识别：内置维护 MHW 文件 ID 表，用于识别模型替换 MOD 对应的游戏装备或模型名称；第一版覆盖武器、防具、发型替换；第一版只识别，不提供替换目标选择或改绑。
- 第一版只保留单 Profile：首期只维护当前这一套启用状态和排序；多 Profile 后续可以做。
- MVP 不包含下载、更新、AI Agent 和模型改绑；后续下载只考虑 Nexus Mods API 和 SSO 登录。
- 后续非 MVP 能力：基于 MHW 文件 ID 表对 MOD 分类、筛选和排序；AI Agent 支持 MHW 术语感知翻译、联网搜索 MOD、提供下载链接或通过 Nexus Mods API 安装。

## 实现前需要细化

这些问题不阻塞 MVP 范围，但实现前需要在具体任务里细化：

- Steam 默认路径检测规则。
- MOD 元数据、启用状态、排序和部署记录的 JSON 文件拆分方式。
- `rar`、`zip`、`7z` 解包实现方案和依赖选择。
- MHW 文件 ID 表的数据格式和维护位置。
- 冲突排序的重新部署策略。
- 后续多 Profile 的数据结构。
- 后续模型改绑的路径重写或文件改写规则。
- 后续 MHW 游戏术语表的数据来源和翻译规则。
- 后续 Nexus Mods API、SSO 登录和 Agent 下载确认流程。

## 文档维护规则

- 当技术决策从“待确认”变成“已确认”时，同步更新相关文档。
- 文档不追求一次写全，优先记录当前阶段真实可执行的约定。
- 如果代码和文档冲突，以代码为当前事实，并及时修正文档。
