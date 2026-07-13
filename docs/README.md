# Acumod 文档入口

本目录记录 Acumen MOD manager 的产品范围、技术架构、开发方式和安全边界。文档优先服务两件事：

1. 让后续开发前能快速确认“现在项目到底怎么运转”。
2. 把尚未确定的技术细节显式列出来，避免在实现时临时猜测。

## 当前阶段

项目当前已完成传统 MOD 管理器 MVP，端到端链路为：

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
- 模型替换识别 service：`src-tauri/src/services/model_recognition.rs`

MVP 已具备游戏目录校验、文件夹与 `.zip/.7z/.rar` 导入、拖拽确认、多候选分支选择、同名去重、本地 MOD 库、启停、卸载、一键还原和独立冲突排序。已安装列表可展开完整文件清单并标记启用中的冲突；所有写入或删除操作都经过预览并基于 manifest 部署记录执行。

内置 `15.10.00` 索引可识别武器、防具、发型、随从武器、随从防具、猎虫、挂件、NPC、投射器和人物语音替换；同一 MOD 可显示多个目标。第一版只识别和展示，不修改 MOD 的替换目标。

MVP 后的 UI 架构重构已完成：应用具有固定侧边导航，以及在右侧内容区并列切换的 MOD 库、导入 MOD、冲突管理和设置页面；悬浮式 AI 助手作为独立窗口层存在，且没有改变既有导入、部署和冲突排序调用链。

传统管理器增强阶段已完成：MOD 库已拆出检索/Profile 工具栏和表格组件，支持持久化显示名称与备注、自动分类、用户分类、搜索、筛选、浏览排序、稳定 ID 类别扩展、同模型冲突提示和多 Profile。模型替换目标改绑、联网下载和 AI Agent 留在后续阶段。

MOD 库表格重构已完成：最左列使用启用状态按钮，名称和备注支持行内编辑，替换信息使用摘要加展开详情；自动识别分类保持只读，用户可创建、选择、重命名和删除全局分类。分类只更新元数据，不影响 MOD 文件、部署、Profile 或冲突排序。

## 文档阅读顺序

1. `docs/README.md`：文档入口、当前阶段、已确认技术事实。
2. `docs/architecture.md`：前端、Tauri command、Rust service、DTO 和 AI 边界。
3. `docs/ui-design.md`：页面信息架构、工作区职责和悬浮 AI 窗口边界。
4. `docs/features.md`：MVP 范围、传统 MOD 管理器、下载和 AI Agent 的功能拆分。
5. `docs/development.md`：开发命令、目录约定、端到端切片检查清单。
6. `docs/security.md`：文件操作安全、用户确认机制、AI 执行边界。

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
- 存储位置：`AppData` 只保存 `config.json` 等小配置；MOD 库和导入暂存放在软件目录旁的 `AcumodData/` 下。
- MOD 部署方式：只使用复制式部署。安装时在 Acumod 软件目录保存一份 MOD；启用时复制到 MHW 游戏目录；禁用时删除游戏目录中的已部署副本；卸载 MOD 时删除软件目录中的 MOD 副本。
- MOD 导入方式：第一版支持文件夹导入，以及 `rar`、`zip`、`7z` 压缩包导入；当前压缩包导入使用随 Acumod 打包的 7-Zip 解包组件，不要求用户另行安装 7-Zip。解包目录只作临时识别，成功后只长期保存所选分支的一份本地库副本。
- MOD 导入识别：优先识别 `nativePC`；缺少 `nativePC` 但出现常见 nativePC 内部目录时自动补成 `nativePC/...`；用户直接选择 `plugins`、`weapon` 等内部目录时保留目录名并映射到 `nativePC/<目录名>/...`；仍无法识别时允许用户确认按游戏根目录相对路径导入。
- MHW 替换识别：`references/mhwi-data/curated/model-index.json` 内置维护武器、防具、发型、随从装备、猎虫、挂件、NPC、投射器和人物语音索引；只把规范资源根目录中的文件显示为游戏内替换目标，`vfx/mod` 等附属特效资源仍部署但不冒充装备替换。防具同一模型的多部位会合并为套装结果。
- 多 Profile：每个 Profile 保存启用 MOD 集合和冲突组顺序；游戏目录同一时刻只部署当前激活的一个 Profile。首次读取会把当前启用状态迁移为“默认配置”；创建新 Profile 会复制当前配置，切换前必须确认部署预览。
- MVP 不包含下载、更新、AI Agent 和模型改绑；后续下载只考虑 Nexus Mods API 和 SSO 登录。
- 当前阶段能力：基于 MHW 文件 ID 表对 MOD 自动分类、搜索、筛选和浏览排序；稳定识别脸型、怪物、噗吱猪服装、家具及玩家/随从附件，并在冲突组中提示相同替换目标。
- 后续非 MVP 能力：模型替换目标改绑、Nexus Mods 下载和更新，以及 AI Agent 的术语感知翻译、联网搜索和受控安装。

## 实现前需要细化

这些问题不阻塞 MVP 范围，但实现前需要在具体任务里细化：

- 社区 Wiki 发型表及衍生索引的最终发布再分发许可。
- 后续模型改绑的路径重写或文件改写规则。
- 后续 MHW 游戏术语表的数据来源和翻译规则。
- 后续 Nexus Mods API、SSO 登录和 Agent 下载确认流程。

## 文档维护规则

- 当技术决策从“待确认”变成“已确认”时，同步更新相关文档。
- 文档不追求一次写全，优先记录当前阶段真实可执行的约定。
- 如果代码和文档冲突，以代码为当前事实，并及时修正文档。
