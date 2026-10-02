// The page actions against the real riki binary: "+" new page, the ⋯ menu's Move and Delete,
// and Undo.

import { expect, test } from './riki'

const editor = (page: import('@playwright/test').Page) => page.locator('.riki-editor-root .ProseMirror')
const confirm = (page: import('@playwright/test').Page) => page.locator('[data-control="dialog-confirm"]')

test('after foo moves to bar, /foo?new=Foo opens the editor prefilled with # Foo and does not redirect', async ({ page, riki }) => {
  await riki.pushAndServe({ 'foo.md': '# Foo\n\nOld page.\n' }, '/foo')
  await page.goto(`${riki.url}/foo`)
  await page.locator('#riki-more').click()
  await page.locator('[data-riki-action="move"]').click()
  await page.locator('input[name="name"]').fill('bar')
  await confirm(page).click()
  await page.waitForURL(`${riki.url}/bar`)
  await expect(page.locator('main article')).toContainText('Old page.')

  await page.goto(`${riki.url}/foo?new=Foo`)
  expect(page.url()).toBe(`${riki.url}/foo?new=Foo`)
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')
  await expect(editor(page).locator('h1')).toHaveText('Foo')
  expect(riki.commitCount()).toBe(3)
})

test('+ then a title writes nothing until Save, then exactly one commit', async ({ page, riki }) => {
  await riki.pushAndServe({ 'notes/prose.md': '# Prose\n\nWords.\n' }, '/notes/prose')
  const before = riki.commitCount()
  await page.goto(`${riki.url}/`)
  await page.locator('[data-riki-new="notes"]').click()
  await page.locator('input[name="title"]').fill('AC4 probe')
  await confirm(page).click()
  await page.waitForURL(`${riki.url}/notes/ac4-probe?new=AC4%20probe`)
  await expect(editor(page).locator('h1')).toHaveText('AC4 probe')
  await expect(page.locator('[data-control="save"]')).toBeEnabled()
  expect(riki.commitCount()).toBe(before)

  await page.keyboard.press('Enter')
  await page.keyboard.type('First words.')
  await page.locator('[data-control="save"]').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  expect(page.url()).toBe(`${riki.url}/notes/ac4-probe`)
  expect(riki.commitCount()).toBe(before + 1)
  expect(riki.file('notes/ac4-probe.md')).toBe('# AC4 probe\n\nFirst words.\n')
})

test('the root + creates a top-level page and leaving the editor writes nothing', async ({ page, riki }) => {
  const before = riki.commitCount()
  await page.goto(`${riki.url}/`)
  await page.locator('[data-riki-new=""]').click()
  await page.locator('input[name="title"]').fill('Status')
  await confirm(page).click()
  await page.waitForURL(`${riki.url}/status-2?new=Status`)
  await expect(editor(page).locator('h1')).toHaveText('Status')
  await page.goto(`${riki.url}/guide`)
  expect(riki.commitCount()).toBe(before)
})

test('delete then Undo renders the page at its URL again, in two commits', async ({ page, riki }) => {
  const before = riki.commitCount()
  const original = riki.file('guide.md')
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-more').click()
  await page.locator('[data-riki-action="delete"]').click()
  await confirm(page).click()
  await expect(page.locator('main article')).toContainText('Deleted guide.md.')
  await expect(page.locator('.riki-sidebar')).not.toContainText('Guide')
  expect(riki.commitCount()).toBe(before + 1)

  await page.locator('[data-control="undo"]').click()
  await page.waitForURL(`${riki.url}/guide`)
  await expect(page.locator('main article')).toContainText('A canonical page.')
  expect(riki.commitCount()).toBe(before + 2)
  expect(riki.file('guide.md')).toBe(original)
})
