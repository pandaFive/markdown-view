const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
  testDir: './tests/e2e',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:4173',
    headless: true
  },
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' }
    }
  ],
  webServer: {
    command: 'cargo run -- tests/fixtures/e2e --port 4173 --no-open',
    url: 'http://localhost:4173',
    reuseExistingServer: true,
    timeout: 120000
  }
});
