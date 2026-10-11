import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { nodePolyfills } from "vite-plugin-node-polyfills";

export default defineConfig({
  plugins: [
    react(),
    nodePolyfills({
      globals: {
        Buffer: true,
        global: true,
        process: true,
      },
    }),
  ],
  // Bind to loopback only: the page holds dev keys and talks to a localnet.
  server: {
    port: 5173,
    host: "127.0.0.1",
    strictPort: true,
  },
  preview: {
    host: "127.0.0.1",
  },
  // Unit and render tests only; the localnet smoke has its own config.
  test: {
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
