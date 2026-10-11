// Copy the generated IDL and its TypeScript type into the testbench.
// Generate them first (from clearinghouse/):
//   anchor idl build -p ch_core -o target/idl/ch_core.json -t target/types/ch_core.ts \
//     -- --features mock-oracle,dev-venue-signer
const fs = require("fs");
const path = require("path");

const root = path.resolve(__dirname, "../..");
const destDir = path.resolve(__dirname, "../src/idl");
const files = [
  ["target/idl/ch_core.json", "ch_core.json"],
  ["target/types/ch_core.ts", "ch_core.ts"],
];

fs.mkdirSync(destDir, { recursive: true });
for (const [src, dest] of files) {
  const from = path.join(root, src);
  if (!fs.existsSync(from)) {
    console.error("Not found:", from, "(build the IDL first)");
    process.exit(1);
  }
  fs.copyFileSync(from, path.join(destDir, dest));
  console.log("synced", src, "->", path.join("testbench/src/idl", dest));
}
