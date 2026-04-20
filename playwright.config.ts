import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  timeout: 30000,
  // E2E は共通 fixture (tests/fixtures/e2e/{README,notes}.md) を読み書きするため
  // ワーカー並列実行で別 spec が同 fixture を上書きし test pollution を起こす。
  // 直列化で決定論性を確保する。
  workers: 1,
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
