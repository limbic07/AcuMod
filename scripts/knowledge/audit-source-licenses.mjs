import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const projectRoot = path.resolve(import.meta.dirname, "../..");
const catalogPath = path.join(projectRoot, "references/knowledge/sources/catalog.json");
const reportPath = path.join(projectRoot, "references/knowledge/audits/source-license-report.json");
const strict = process.argv.includes("--strict");

function requireText(value, field, sourceId) {
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`Source ${sourceId} is missing ${field}.`);
  }
}

function auditSource(source, approval) {
  const blockers = [];
  if (source.licenseStatus !== "projectOwned" && source.licenseStatus !== "GPL-3.0") {
    blockers.push(`licenseStatus is not an unambiguous redistribution license: ${source.licenseStatus}`);
  }
  if (source.redistribution === "requiresPermission" || source.redistribution.includes("releaseAuditRequired")) {
    blockers.push("redistribution requires a separate permission or release audit");
  }
  if (source.gameVersion === "unverified") {
    blockers.push("source game version is unverified");
  }
  const rawStatus = blockers.length === 0 ? "clear" : "reviewRequired";
  const approved = approval.approvedSourceIds.has(source.id);
  return {
    // 人工批准只覆盖本次明确列出的来源；新增来源会自动回到待审计状态。
    status: approved ? "approved" : rawStatus,
    rawStatus,
    blockers,
  };
}

const catalog = JSON.parse(await readFile(catalogPath, "utf8"));
if (catalog.schemaVersion !== 1 || !Array.isArray(catalog.sources)) {
  throw new Error("Source catalog schema is invalid.");
}
const approval = catalog.distributionApproval;
if (
  !approval ||
  approval.status !== "approvedByProjectMaintainer" ||
  !Array.isArray(approval.approvedSourceIds) ||
  approval.approvedSourceIds.length === 0
) {
  throw new Error("Source catalog is missing a valid project-maintainer distribution approval.");
}
for (const field of ["approvedAt", "scope", "decision"]) {
  requireText(approval[field], `distributionApproval.${field}`, "catalog");
}
approval.approvedSourceIds = new Set(approval.approvedSourceIds);

const ids = new Set();
const entries = catalog.sources.map((source) => {
  for (const field of ["id", "title", "kind", "gameVersion", "usage", "redistribution", "licenseStatus", "verificationStatus"]) {
    requireText(source[field], field, source.id ?? "unknown");
  }
  requireText(source.url ?? "local:project", "url", source.id);
  if (ids.has(source.id)) throw new Error(`Duplicate source ID: ${source.id}.`);
  ids.add(source.id);
  const audit = auditSource(source, approval);
  return {
    sourceId: source.id,
    title: source.title,
    licenseStatus: source.licenseStatus,
    redistribution: source.redistribution,
    usage: source.usage,
    verificationStatus: source.verificationStatus,
    status: audit.status,
    rawStatus: audit.rawStatus,
    blockers: audit.blockers,
  };
});
for (const sourceId of approval.approvedSourceIds) {
  if (!ids.has(sourceId)) {
    throw new Error(`Distribution approval references an unknown source: ${sourceId}.`);
  }
}

const report = {
  schemaVersion: 1,
  generatedAt: new Date().toISOString(),
  targetGameVersion: catalog.targetGameVersion,
  strict,
  distributionApproval: {
    status: approval.status,
    approvedAt: approval.approvedAt,
    scope: approval.scope,
    decision: approval.decision,
  },
  sourceCount: entries.length,
  approvedCount: entries.filter((entry) => entry.status === "approved").length,
  reviewRequiredCount: entries.filter((entry) => entry.status === "reviewRequired").length,
  entries,
};

await mkdir(path.dirname(reportPath), { recursive: true });
await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
console.log(`Source distribution audit: ${report.approvedCount} approved, ${report.reviewRequiredCount} awaiting approval, ${report.sourceCount} total.`);
console.log(`Report: ${path.relative(projectRoot, reportPath)}`);

if (strict && report.reviewRequiredCount > 0) {
  console.error("Release verification blocked: one or more sources are outside the current project-maintainer approval.");
  process.exitCode = 1;
}
