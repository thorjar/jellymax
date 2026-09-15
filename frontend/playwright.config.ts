import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./e2e",
  workers: 1,
  timeout: 30_000,
  use: { baseURL: "http://127.0.0.1:15173", trace: "retain-on-failure" },
  webServer: [
    { command: "python3 e2e/server.py", port: 18097, timeout: 120_000, reuseExistingServer: false },
    { command: "npm run dev -- --host 127.0.0.1 --port 15173 --strictPort", port: 15173,
      env: { VITE_API_TARGET: "http://127.0.0.1:18097" }, reuseExistingServer: false },
  ],
});
