import { defineConfig, mergeConfig } from "vitest/config";
import base from "./vite.config";

// `npm run smoke`: the end-to-end check against a running localnet.
export default mergeConfig(
  base,
  defineConfig({
    test: {
      include: ["scripts/smoke.e2e.ts"],
      testTimeout: 180_000,
    },
  })
);
