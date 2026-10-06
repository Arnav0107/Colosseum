const fs = require("fs");
const path = require("path");

const sourcePath = path.resolve(__dirname, "../../target/idl/ch_core.json");
const destDir = path.resolve(__dirname, "../src/idl");
const destPath = path.resolve(destDir, "ch_core.json");

if (!fs.existsSync(sourcePath)) {
  console.error("Source IDL not found at", sourcePath);
  process.exit(1);
}

if (!fs.existsSync(destDir)) {
  fs.mkdirSync(destDir, { recursive: true });
}

fs.copyFileSync(sourcePath, destPath);
console.log("Successfully synced IDL to", destPath);
