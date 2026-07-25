# AcuAI 知识库来源许可审计

`references/knowledge/sources/catalog.json` 是知识库来源、版本、用途和再分发边界的唯一登记表。

运行：

```powershell
npm.cmd run knowledge:audit-licenses
npm.cmd run knowledge:verify-release
```

`knowledge:audit-licenses` 会生成被 Git 忽略的 `references/knowledge/audits/source-license-report.json`，逐条记录来源的许可状态、用途、再分发方式和阻塞原因。

`knowledge:verify-release` 是严格发布门槛。以下情况会阻止正式知识包发布：

- `licenseStatus` 仍为 `notAudited`；
- 来源要求单独授权或发布前审计；
- 来源游戏版本仍为 `unverified`。

当前开发审计快照：共登记 43 个来源，其中 4 个已通过初步审计，39 个仍然阻塞正式发布。已通过的来源是：

- `wwiseutil`：目录中记录了 GPL-3.0 信息，知识包只保留格式核验摘要。
- `mhw-curated-special-quest-name-map`：项目维护的人工桥接表。
- `acumod-validated-modding`：项目维护的回归验证记录。
- `acumod-help-documents`：项目维护的 Acumod 使用说明。

其余来源即使只进入开发包的派生字段，也不能仅凭“公开可访问”推断允许再分发。只有在来源许可证、派生字段范围、简繁文本桥、攻略摘要和 MOD 技术摘要分别完成复核后，才能把对应条目的 `licenseStatus` 从 `notAudited` 改为明确状态，并重新运行严格发布检查。

特别注意：`mhworlddata-armor-name-map` 已记录为 `MIT-code-Capcom-data-unverified`。上游的 MIT 许可只覆盖构建代码，不自动覆盖游戏数据和图片，因此该条目仍然阻塞正式再分发。

本轮已复核的 MOD 技术来源：`wwiseutil` 官方仓库明确标注 GPL-3.0；`CTC-MHW-Editor`、`MHW-Editor` 和 `MonsterHunterWorldModding` 的官方仓库页面未显示明确许可证声明，因此仍按未审计处理，不能仅因为仓库公开就解除阻塞。

开发包可以在这些状态下继续构建和测试，但只能用于本地开发验收，不能直接作为公开下载附件。只有完成原始数据、派生字段、简繁文本桥、攻略摘要和 MOD 技术摘要的逐来源复核后，才允许通过严格发布门槛。
