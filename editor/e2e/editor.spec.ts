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

test('selecting text floats the selection toolbar; Turn into Heading 2 and bold are saved, the fixed toolbar stays', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')
  const bar = page.locator('.riki-selection-toolbar')
  await expect(control(page, 'bold').first()).toBeVisible()
  await editor(page).getByText('A canonical page.').click()
  await expect(bar).toBeHidden()

  await editor(page).getByText('A canonical page.').click({ clickCount: 3 })
  await expect(bar).toBeVisible()
  await bar.locator('[data-control="selection-turn-into"]').click()
  await bar.locator('[data-control="selection-turn-h2"]').click()
  await expect(editor(page).locator('h2')).toHaveText('A canonical page.')

  await editor(page).locator('h2').click({ clickCount: 3 })
  await expect(bar).toBeVisible()
  await bar.locator('[data-control="selection-bold"]').click()
  await control(page, 'save').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  expect(riki.file('guide.md')).toContain('## **A canonical page.**')
})

test('hovering a paragraph shows the block handle in the left gutter; Delete from its menu removes the block from the saved file', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')
  const handle = page.locator('.riki-block-handle')
  const paragraph = editor(page).getByText('A canonical page.')
  await paragraph.hover()
  await expect(handle).toHaveAttribute('data-show', 'true')
  const grip = handle.locator('[data-control="block-handle"]')
  await expect(grip).toBeVisible()

  const root = (await page.locator('.riki-editor-root').boundingBox())!
  const text = (await editor(page).boundingBox())!
  const gripBox = (await grip.boundingBox())!
  const para = (await paragraph.boundingBox())!
  // Inside the editor root, left of the text column, level with the hovered paragraph.
  expect(gripBox.x).toBeGreaterThanOrEqual(root.x)
  expect(gripBox.x + gripBox.width).toBeLessThanOrEqual(text.x)
  expect(gripBox.y).toBeLessThan(para.y + para.height)
  expect(gripBox.y + gripBox.height).toBeGreaterThan(para.y)

  await grip.click()
  await page.locator('.riki-block-menu [data-control="block-delete"]').click()
  await expect(editor(page)).not.toContainText('A canonical page.')
  await control(page, 'save').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  expect(riki.file('guide.md')).toBe('# Guide\n\n- one\n- two\n')
})

test('dragging the block handle moves the block (native drag)', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')
  await editor(page).getByText('A canonical page.').hover()
  const grip = page.locator('.riki-block-handle [data-control="block-handle"]')
  await expect(grip).toBeVisible()
  await grip.dragTo(editor(page).locator('h1'), { targetPosition: { x: 4, y: 2 } })
  await expect(editor(page).locator('> *').first()).toHaveText('A canonical page.')
  await control(page, 'save').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  expect(riki.file('guide.md')).toBe('A canonical page.\n\n# Guide\n\n- one\n- two\n')
})

test('typing / in an empty paragraph opens the slash menu; Heading 2 is saved, and the gutter + opens it too', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')
  const menu = page.locator('.riki-slash-menu')
  const paragraph = editor(page).getByText('A canonical page.')

  // The gutter + below the paragraph opens the menu on the new empty paragraph.
  await paragraph.hover()
  await page.locator('.riki-block-handle [data-control="block-add"]').click()
  await expect(menu).toHaveAttribute('data-show', 'true')
  await expect(menu.locator('button:visible')).toHaveCount(16)
  await page.keyboard.press('Escape')
  await expect(menu).toHaveAttribute('data-show', 'false')

  // Typing / in that paragraph opens it, filtered.
  await page.keyboard.type('/head')
  await expect(menu).toHaveAttribute('data-show', 'true')
  await expect(menu.locator('button:visible')).toHaveCount(3)
  await menu.locator('[data-control="slash-h2"]').click()
  await page.keyboard.type('Next')
  await control(page, 'save').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  expect(riki.file('guide.md')).toBe('# Guide\n\nA canonical page.\n\n## Next\n\n- one\n- two\n')
})

test("clicking a table's column handle and Add column after saves the table with one more column", async ({ page, riki }) => {
  await riki.pushAndServe({ 'table.md': '# Table\n\n| Name | Value |\n| --- | --- |\n| alpha | 1 |\n' }, '/table')
  await page.goto(`${riki.url}/table`)
  await page.locator('#riki-edit').click()
  await expect(editor(page)).toHaveAttribute('contenteditable', 'true')

  await editor(page).locator('th').filter({ hasText: 'Name' }).hover()
  const handle = page.locator('.milkdown-table-block [data-role="col-drag-handle"]')
  await expect(handle).toBeVisible()
  const header = (await editor(page).locator('th').first().boundingBox())!
  const handleBox = (await handle.boundingBox())!
  // Over the hovered column, on its top edge.
  expect(handleBox.x).toBeGreaterThanOrEqual(header.x)
  expect(handleBox.x + handleBox.width).toBeLessThanOrEqual(header.x + header.width)
  expect(handleBox.y).toBeLessThan(header.y + 4)

  await handle.click()
  const add = handle.locator('[data-control="table-add-col-after"]')
  await expect(add).toBeVisible()
  await add.click()
  await expect(editor(page).locator('th')).toHaveCount(3)
  await editor(page).locator('th').nth(1).click()
  await page.keyboard.type('Added')

  await control(page, 'save').click()
  await expect(page.locator('.riki-editor')).toHaveCount(0)
  expect(riki.file('table.md')).toBe('# Table\n\n| Name | Added | Value |\n| --- | --- | --- |\n| alpha | | 1 |\n')
})
