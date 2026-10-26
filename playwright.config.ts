// ページを通すテスト。scripts/serve.ts で配るサイトを、Chromium・Firefox・WebKit で開く
import { defineConfig, devices } from "@playwright/test";

const port = 8710;
const url = `http://127.0.0.1:${port}/`;

export default defineConfig({
  testDir: "tests/e2e",
  forbidOnly: true,
  reporter: "list",
  use: { baseURL: url },
  webServer: {
    command: `node scripts/serve.ts ${port} --local-wasm`,
    url,
  },
  projects: [
    { name: "chromium", use: devices["Desktop Chrome"] },
    { name: "firefox", use: devices["Desktop Firefox"] },
    { name: "webkit", use: devices["Desktop Safari"] },
  ],
});
