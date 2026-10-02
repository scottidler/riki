// The Ctrl+K palette against the real riki binary: the shortcut, the header button, a hit's
// URL and anchor, and the editor's Ctrl+K link box taking the key.

import { expect, test } from './riki'

const FILES = {
  'README.md': '# Home\n\nWelcome.\n',
  'reference/tables.md': '# Tables\n\nIntro words.\n\n## Table syntax\n\nPipes make a table.\n',
}

test('Ctrl+K, type, Enter lands on the hit URL and heading anchor', async ({ page, riki }) => {
  await riki.pushAndServe(FILES, '/reference/tables')
  await page.goto(`${riki.url}/`)
  await page.keyboard.press('Control+k')
  const input = page.locator('.riki-search input')
  await expect(input).toBeFocused()
  await input.fill('pipe')
  const hit = page.locator('.riki-search-hit').first()
  await expect(hit).toBeVisible()
  await expect(hit.locator('mark').first()).toHaveText(/^pipe/i)
  await page.keyboard.press('Enter')
  await page.waitForURL(`${riki.url}/reference/tables#table-syntax`)
  await expect(page.locator('.riki-search')).toHaveCount(0)
})

test('the header button opens the palette and Escape closes it', async ({ page, riki }) => {
  await riki.pushAndServe(FILES, '/reference/tables')
  await page.goto(`${riki.url}/`)
  await page.locator('[data-riki-search]').click()
  await expect(page.locator('.riki-search input')).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(page.locator('.riki-search')).toHaveCount(0)
})

test('inside the editor Ctrl+K opens the link box, not the palette, and links relative to the file', async ({ page, riki }) => {
  await riki.pushAndServe(FILES, '/reference/tables')
  await page.goto(`${riki.url}/reference/tables`)
  await page.locator('#riki-edit').click()
  const editor = page.locator('.riki-editor-root .ProseMirror')
  await expect(editor).toHaveAttribute('contenteditable', 'true')
  await editor.click()
  await page.keyboard.press('Control+k')
  await expect(page.locator('.riki-linkbox input')).toBeFocused()
  await expect(page.locator('[role="dialog"][aria-label="Search"]')).toHaveCount(0)
  await page.locator('.riki-linkbox input').fill('home')
  await expect(page.locator('.riki-linkbox .riki-search-hit').first()).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(page.locator('.riki-linkbox')).toHaveCount(0)
  await page.locator('[data-control="save"]').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  expect(riki.file('reference/tables.md')).toContain('[Home](../README.md)')
})
