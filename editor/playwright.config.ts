import os from 'node:os'
import path from 'node:path'
import { defineConfig } from '@playwright/test'

// Runs against the real riki binary (`cargo build -p riki-server` first; otto `e2e` does both).
// Each test starts its own riki on a free loopback port over a tempdir upstream and cache.
export default defineConfig({
  testDir: 'e2e',
  // Failure traces go to the temp dir, never into the repo.
  outputDir: path.join(os.tmpdir(), 'riki-playwright'),
  workers: 1,
  timeout: 60_000,
  reporter: [['list']],
  use: {
    browserName: 'chromium',
    headless: true,
    // Header identity, as the edge would inject it. Synthetic, never a real person.
    extraHTTPHeaders: { 'Remote-Email': 'e2e-editor@example.test', 'Remote-Name': 'E2E Editor' },
  },
})
