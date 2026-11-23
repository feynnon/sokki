import base from "./playwright.config";
import { defineConfig } from "@playwright/test";

// The main-window views on their own port, so a dev server another
// checkout runs on 1420 is never reused.
export default defineConfig({
  ...base,
  reporter: "list",
  use: { ...base.use, baseURL: "http://localhost:1431" },
  webServer: {
    command: "bunx vite dev --port 1431 --strictPort",
    url: "http://localhost:1431",
    reuseExistingServer: false,
    timeout: 60000,
  },
});
