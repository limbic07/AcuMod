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

function auditSource(source) {
  const blockers = [];
  if (source.licenseStatus === "projectOwned") {
    return { status: "clear", blockers };
  }
  if (source.licenseStatus !== "GPL-3.0") {
    blockers.push(`licenseStatus is not an unambiguous redistribution license: ${source.licenseStatus}`);
  }
  if (source.redistribution === "requiresPermission" || source.redistribution.includes("releaseAuditRequired")) {
    blockers.push("redistribution requires a separate permission or release audit");
  }
  if (source.gameVersion === "unverified") {
    blockers.push("source game version is unverified");
  }
  return {
    status: blockers.length === 0 ? "clear" : "blocked",
    blockers,
  };
}

const catalog = JSON.parse(await readFile(catalogPath, "utf8"));
if (catalog.schemaVersion !== 1 || !Array.isArray(catalog.sources)) {
  throw new Error("Source catalog schema is invalid.");
}

const ids = new Set();
const entries = catalog.sources.map((source) => {
  for (const field of ["id", "title", "kind", "gameVersion", "usage", "redistribution", "licenseStatus", "verificationStatus"]) {
    requireText(source[field], field, source.id ?? "unknown");
  }
  requireText(source.url ?? "local:project", "url", source.id);
  if (ids.has(source.id)) throw new Error(`Duplicate source ID: ${source.id}.`);
  ids.add(source.id);
  const audit = auditSource(source);
  return {
    sourceId: source.id,
    title: source.title,
    licenseStatus: source.licenseStatus,
    redistribution: source.redistribution,
    usage: source.usage,
    verificationStatus: source.verificationStatus,
    status: audit.status,
    blockers: audit.blockers,
  };
});

const report = {
  schemaVersion: 1,
  generatedAt: new Date().toISOString(),
  targetGameVersion: catalog.targetGameVersion,
  strict,
  sourceCount: entries.length,
  clearCount: entries.filter((entry) => entry.status === "clear").length,
  blockedCount: entries.filter((entry) => entry.status === "blocked").length,
  entries,
};

await mkdir(path.dirname(reportPath), { recursive: true });
await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
console.log(`Source license audit: ${report.clearCount} clear, ${report.blockedCount} blocked, ${report.sourceCount} total.`);
console.log(`Report: ${path.relative(projectRoot, reportPath)}`);

if (strict && report.blockedCount > 0) {
  console.error("Release verification blocked: one or more sources still require license or redistribution review.");
  process.exitCode = 1;
}
