#!/usr/bin/env node
// Fails if any program's declare_id! disagrees with Anchor.toml, or if the
// committed testbench IDL / Anchor.toml genesis entry point at another address.
// Usage: node scripts/ci/check-program-ids.cjs   (from clearinghouse/)
const fs = require("fs");
const path = require("path");

const root = path.resolve(__dirname, "../..");
const anchorToml = fs.readFileSync(path.join(root, "Anchor.toml"), "utf8");
let failed = false;
const fail = (msg) => {
  console.error("FAIL:", msg);
  failed = true;
};

// [programs.localnet] name = "address"
const section = /\[programs\.localnet\]([\s\S]*?)(\n\[|$)/.exec(anchorToml);
if (!section) {
  fail("Anchor.toml has no [programs.localnet] section");
  process.exit(1);
}
const tomlIds = {};
for (const m of section[1].matchAll(/^\s*([a-z0-9_]+)\s*=\s*"([1-9A-HJ-NP-Za-km-z]{32,44})"/gm)) {
  tomlIds[m[1]] = m[2];
}

for (const [name, addr] of Object.entries(tomlIds)) {
  const lib = path.join(root, "programs", name, "src", "lib.rs");
  if (!fs.existsSync(lib)) {
    fail(`Anchor.toml lists ${name} but ${lib} does not exist`);
    continue;
  }
  const m = /declare_id!\("([^"]+)"\)/.exec(fs.readFileSync(lib, "utf8"));
  if (!m) fail(`${name}: no declare_id! in ${lib}`);
  else if (m[1] !== addr) fail(`${name}: declare_id! ${m[1]} != Anchor.toml ${addr}`);
  else console.log(`ok  ${name.padEnd(14)} ${addr}`);
}

// Genesis entries must point at a workspace program's declared address.
for (const m of anchorToml.matchAll(/\[\[test\.genesis\]\]\s*\naddress\s*=\s*"([^"]+)"\s*\nprogram\s*=\s*"([^"]+)"/g)) {
  const name = path.basename(m[2], ".so");
  if (tomlIds[name] !== m[1]) fail(`genesis entry for ${name} uses ${m[1]}, Anchor.toml says ${tomlIds[name]}`);
  else console.log(`ok  genesis ${name} -> ${m[1]}`);
}

const idlPath = path.join(root, "testbench", "src", "idl", "ch_core.json");
const idl = JSON.parse(fs.readFileSync(idlPath, "utf8"));
if (idl.address !== tomlIds.ch_core) fail(`testbench IDL address ${idl.address} != ${tomlIds.ch_core}`);
else console.log(`ok  testbench IDL address`);

if (failed) process.exit(1);
