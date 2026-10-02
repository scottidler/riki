// The editor against the real riki binary. Authoring uses only toolbar clicks and plain-text
// typing: no Markdown syntax is ever typed.

import { expect, test } from './riki'
import type { Page } from '@playwright/test'

const editor = (page: Page) => page.locator('.riki-editor-root .ProseMirror')
const control = (page: Page, name: string) => page.locator(`[data-control="${name}"]`)
const status = (page: Page) => page.locator('.riki-status')

test('with the round-trip endpoint answering 500, Save stays disabled', async ({ page, riki }) => {
  await page.route('**/_riki/api/roundtrip', (route) => route.fulfill({ status: 500, body: 'guard exploded' }))
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(status(page)).toContainText('Editing is off for this page')
  await expect(status(page)).toContainText('HTTP 500')
  await expect(control(page, 'save')).toBeDisabled()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'false')
  await expect(control(page, 'bold')).toBeDisabled()
  // Still off after any late settling.
  await page.waitForTimeout(1000)
  await expect(control(page, 'save')).toBeDisabled()
})

test('with the round-trip endpoint never answering, Save stays disabled after the 5s deadline', async ({ page, riki }) => {
  await page.route('**/_riki/api/roundtrip', () => {
    // never fulfilled
  })
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(status(page)).toContainText('did not answer within 5s', { timeout: 10_000 })
  await expect(control(page, 'save')).toBeDisabled()
})

test('a page the editor would rewrite stays read-only (identical: false)', async ({ page, riki }) => {
  await page.goto(`${riki.url}/starred`)
  await page.locator('#riki-edit').click()
  await expect(status(page)).toContainText('cannot reproduce this page exactly')
  await expect(control(page, 'save')).toBeDisabled()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'false')
})

test('a canonical page passes the guard and becomes editable in place', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')
  await expect(control(page, 'save')).toBeEnabled()
  expect(page.url()).toBe(`${riki.url}/guide`)
})

test('a table, a WARNING alert, and a link from toolbar clicks and plain typing are saved as GFM', async ({ page, riki }) => {
  await page.goto(`${riki.url}/notes/toolbar`)
  await page.getByText('Create this page').click()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')
  await editor(page).click()
  await page.keyboard.type('Intro text')
  await page.keyboard.press('Enter')
  await page.keyboard.type('Mind the gap')
  await page.keyboard.press('Enter')
  await page.keyboard.type('riki docs')
  await page.keyboard.press('Enter')

  await control(page, 'table').click()
  for (const [i, cell] of ['Name', 'Value', 'Notes', 'alpha', 'one', 'first'].entries()) {
    if (i > 0) await page.keyboard.press('Tab')
    await page.keyboard.type(cell)
  }

  await editor(page).getByText('Mind the gap').click()
  await control(page, 'alert-type').selectOption('WARNING')
  await control(page, 'alert').click()

  await editor(page).getByText('riki docs').click({ clickCount: 3 })
  await control(page, 'link').click()
  const href = page.locator('.milkdown-link-edit input')
  await expect(href).toBeVisible()
  await href.fill('https://example.com/riki')
  await href.press('Enter')

  await control(page, 'save').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  await expect(page.locator('main article')).toContainText('Mind the gap')
  expect(page.url()).toBe(`${riki.url}/notes/toolbar`)

  const saved = riki.file('notes/toolbar.md')
  console.log(`saved notes/toolbar.md:\n${saved}`)
  expect(saved).toMatch(/^\| Name \| Value \| Notes \|\n\| --- \| --- \| --- \|\n\| alpha \| one \| first \|$/m)
  expect(saved).toContain('> [!WARNING]\n> Mind the gap\n')
  expect(saved).toContain('[riki docs](https://example.com/riki)')
  expect(riki.lastAuthor()).toBe('e2e-editor@example.test')
})

test('a 409 keeps the text and offers load latest, which discards after a confirm', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(control(page, 'save')).toBeEnabled()
  await editor(page).getByText('A canonical page.').click()
  await page.keyboard.press('End')
  await page.keyboard.type(' My local edit.')

  riki.push({ 'guide.md': '# Guide\n\nChanged on a laptop.\n' }, 'laptop edit')
  await control(page, 'save').click()
  await expect(status(page)).toContainText('changed since it was loaded')
  await expect(editor(page)).toContainText('My local edit.')

  page.once('dialog', (dialog) => void dialog.accept())
  await control(page, 'load-latest').click()
  await expect(editor(page)).toContainText('Changed on a laptop.')
  await expect(editor(page)).not.toContainText('My local edit.')
  await expect(control(page, 'save')).toBeEnabled()
})

test('dismissing the load-latest confirm keeps the local edits', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(control(page, 'save')).toBeEnabled()
  await editor(page).getByText('A canonical page.').click()
  await page.keyboard.press('End')
  await page.keyboard.type(' Keep me.')
  riki.push({ 'guide.md': '# Guide\n\nMoved upstream.\n' }, 'laptop edit')
  await control(page, 'save').click()
  await expect(status(page)).toContainText('changed since it was loaded')
  page.once('dialog', (dialog) => void dialog.dismiss())
  await control(page, 'load-latest').click()
  await expect(editor(page)).toContainText('Keep me.')
})
