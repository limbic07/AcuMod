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

## 前端职责

前端负责：

- 展示游戏目录、MOD 列表、冲突排序、模型替换识别结果和操作结果。
- 调用 `src/api/*` 中的类型化函数，而不是在组件里直接散落 `invoke()`。
- 在危险操作前展示明确的确认信息。
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
- 执行复制式安装、启用、禁用、卸载、备份和回滚。
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

这个模型的好处是语义直观：安装表示“纳入管理”，启用才表示“写入游戏目录”。第一版只保留单 Profile，也就是只维护当前这一套启用状态和排序；多 Profile 后续可以扩展。

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
    mods/
      installed/           已导入并由 Acumod 管理的 MOD 副本
      staging/
        imports/           压缩包解包和导入预览的暂存目录
```

MOD 文件、导入暂存和后续备份可能很大，不放入 `AppData`。后续制作安装包时，需要确保软件目录对普通用户可写；如果安装到 `Program Files` 等受限目录，应提供 MOD 库位置设置或选择用户可写安装位置。

## MOD 导入目录识别

文件夹导入和压缩包导入应共用同一套目录识别规则。压缩包只负责先解包到暂存目录；解包后的目录树仍然走同一个识别入口。

第一版识别顺序：

1. 优先查找 `nativePC` 目录，并把其中的文件映射为 `nativePC/...`。
2. 如果没有 `nativePC`，但内容根下出现 `weapon`、`wp`、`pl`、`plugins`、`common`、`npc`、`em`、`stage`、`sound`、`ui` 等常见 nativePC 内部目录，则自动补成 `nativePC/...`。
3. 如果用户直接选择了 nativePC 内部目录本身，例如 `plugins/` 或 `weapon/`，则保留目录名并映射为 `nativePC/plugins/...` 或 `nativePC/weapon/...`。
4. 如果出现多个同级候选内容根，不自动选择，返回候选列表让用户决定。
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

MHW 的模型替换 MOD 通常可以通过文件路径和文件 ID 判断它替换的是哪一个游戏内模型。Acumod 需要内置维护一份 MHW 文件 ID 表，把低层文件 ID 映射成用户能理解的名称。MVP 先覆盖武器、防具、发型替换；同一个 MOD 可能识别出多个替换目标。第一版只做识别和展示，不提供替换目标选择或改绑。

建议识别链路：

```text
MOD 文件列表
  -> 解析 MHW 资源路径和文件 ID
  -> 查询 MHW 文件 ID 表
  -> 得到模型类型和游戏内名称
  -> 返回 ModelReplacement DTO
  -> UI 展示“该 MOD 替换了什么”
```

模型替换信息建议包含：

- 模型类型：武器、防具、发型。
- 具体类型：例如太刀、大剑、弓等。
- 原始目标 ID。
- 原始目标游戏内名称。

后续如果支持同类型模型替换目标选择，应作为独立功能设计。该功能不能修改 MOD 库中的原始文件，应在部署阶段生成新的部署计划。MVP 暂不做任何模型改绑，包括通过路径、文件名或文件内容进行改绑。

## ID 表后续用途

MHW 文件 ID 表不只用于展示模型替换目标。MVP 之后，它还可以成为 MOD 分类、筛选和排序的依据：

- 根据识别结果自动标记 MOD 类型，例如武器外观、防具外观、发型替换。
- 根据武器种类、装备部位、游戏内名称筛选 MOD。
- 在 MOD 列表中按替换目标聚合或排序。
- 在冲突排序中优先突出替换同一模型或同一路径的 MOD。

这些能力应建立在只读识别结果上，不要求修改 MOD 文件。

## MVP 存储策略

MVP 先使用 JSON 文件保存配置、MOD 元数据、启用状态、排序和部署记录。

选择 JSON 的原因：

- 当前项目已经使用 `serde` / `serde_json`。
- MVP 查询关系还不复杂。
- 文件结构更适合学习和调试。
- 暂时不需要引入数据库依赖。

后续如果出现大量 MOD、复杂搜索、历史记录或多 Profile，再评估 SQLite。

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

也就是说，AI 只能进入与传统 UI 相同的操作入口。传统按钮能做什么，AI 才能申请做什么；传统管理器尚未实现的能力，AI 也不应绕过实现。MVP 不包含 AI Agent，也不包含模型改绑；后续如果加入这些能力，仍必须生成计划并等待用户确认。

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
