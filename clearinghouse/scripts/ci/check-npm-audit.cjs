#!/usr/bin/env node
// Run `npm audit --omit=dev` in a package directory and fail on any advisory
// that is not in npm-audit-accepted.json (each entry carries a reachability
// justification and a review date). Fails as well once the review date has
// passed, so accepted risk cannot go stale silently.
// Usage: node scripts/ci/check-npm-audit.cjs <package dir>
const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const dir = path.resolve(process.argv[2] ?? ".");
const register = JSON.parse(fs.readFileSync(path.join(__dirname, "npm-audit-accepted.json"), "utf8"));
const today = new Date().toISOString().slice(0, 10);
if (today > register.review_by) {
  console.error(`FAIL: accepted npm advisories were due for re-review on ${register.review_by}.`);
  process.exit(1);
}

let out;
try {
  // Fixed command string, no interpolated input.
  out = execSync("npm audit --omit=dev --json --package-lock-only --no-save", { cwd: dir, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
} catch (e) {
  out = e.stdout; // npm audit exits non-zero when it finds anything
}
const report = JSON.parse(out);
const found = new Map();
for (const v of Object.values(report.vulnerabilities ?? {})) {
  for (const via of v.via) {
    if (typeof via === "string") continue;
    const id = (via.url ?? "").split("/").pop();
    found.set(id, `${via.name} ${via.severity}: ${via.title}`);
  }
}
const unaccepted = [...found].filter(([id]) => !register.accepted[id]);
for (const [id, what] of found) {
  console.log(`${register.accepted[id] ? "accepted" : "NEW     "}  ${id}  ${what}`);
}
if (unaccepted.length > 0) {
  console.error(`FAIL: ${unaccepted.length} production advisory(ies) in ${dir} not reviewed. Fix, or review and add to scripts/ci/npm-audit-accepted.json.`);
  process.exit(1);
}
console.log(`ok  ${found.size} production advisories in ${path.basename(dir)}, all reviewed and accepted`);
