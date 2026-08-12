# AcuAI 知识库来源许可审计

`references/knowledge/sources/catalog.json` 是知识库来源、版本、用途和再分发边界的唯一登记表。

运行：

```powershell
npm.cmd run knowledge:audit-licenses
npm.cmd run knowledge:verify-release
```

`knowledge:audit-licenses` 会生成被 Git 忽略的 `references/knowledge/audits/source-license-report.json`，逐条记录来源的许可状态、用途、再分发方式和阻塞原因。

`knowledge:verify-release` 以来源目录中的 `distributionApproval` 为发布门槛。当前知识包已经由项目维护者人工审核并标记为可分发；严格校验会确认以下情况不存在：

- 新增来源未列入本次人工批准清单；
- 人工批准字段缺失、范围不完整或无明确批准日期；
- 来源用途超出当前知识包登记范围。

## 本地开发使用范围

当前知识包的 18 个登记来源已由项目维护者于 2026-08-13 人工审核，并标记为可作为独立知识包分发。该结论的精确范围记录在 `catalog.json` 的 `distributionApproval.approvedSourceIds`：它只覆盖当前来源、当前派生摘要和当前用途；新增来源、重新导入原始网页/文件、或扩大用途时必须重新审核并显式加入新批准清单。

自动审计仍会保留每个来源的原始许可证信号、版本标记和历史待核对项，便于维护者追溯；这些提示不再自动否决已经人工批准的当前发布范围。

## 已移除的第三方攻略内容

此前登记的第三方攻略页面摘要和任务解锁页面派生条件已从 `game-guide-documents.json`、来源目录、构建脚本和抓取命令中移除；`mhw-game-guides` 当前生成空包，以保持安装格式兼容而不再分发这些内容。

## 人工批准后的维护边界

`mhworlddata-armor-name-map` 的上游仍区分 MIT 构建代码和 Capcom 游戏数据/图片；MOD 技术来源中也仍有未在机器可读许可字段中表达的条目。这些事实作为审计记录保留，而当前发布状态以维护者的人工审核决定为准。该文档不构成第三方授权证明或法律意见；后续若来源条款、内容范围或发布模式变化，应重新进行人工审核。
