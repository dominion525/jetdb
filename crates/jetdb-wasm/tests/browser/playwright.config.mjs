// Runs examples.spec.mjs on Chromium, Firefox and WebKit, against the
// developer page served by scripts/serve-wasm-example.mjs.

import { defineConfig, devices } from "@playwright/test";

const port = 8181;
const url = `http://127.0.0.1:${port}/examples/`;

export default defineConfig({
  testDir: ".",
  forbidOnly: true,
  reporter: "list",
  use: { baseURL: url },
  webServer: {
    command: `node ../../../../scripts/serve-wasm-example.mjs ${port}`,
    url,
  },
  projects: [
    { name: "chromium", use: devices["Desktop Chrome"] },
    { name: "firefox", use: devices["Desktop Firefox"] },
    { name: "webkit", use: devices["Desktop Safari"] },
  ],
});
