// The page theme against the real riki binary: editing looks the way reading does, and the page
// script's toggles and copy buttons work under the page CSP.

import { expect, test } from './riki'
import type { Page } from '@playwright/test'

const PAGE = [
  '# Setup guide',
  '',
  'Install the toolchain, then clone the repo.',
  '',
  '```bash',
  'cargo build --release',
  '```',
  '',
  '> [!WARNING]',
  '> Never commit credentials.',
  '',
  '| Tool | Version |',
  '| --- | --- |',
  '| rust | 1.98 |',
  '',
].join('\n')

/** What "looks the same" means here: type, color, and box, not layout-dependent sizes. */
const PROPS = [
  'font-family',
  'font-size',
  'font-weight',
  'font-style',
  'line-height',
  'letter-spacing',
  'color',
  'background-color',
  'text-transform',
  'text-align',
  'margin-top',
  'margin-bottom',
  'padding-top',
  'padding-right',
  'padding-bottom',
  'padding-left',
  'border-top-width',
  'border-top-style',
  'border-top-color',
  'border-bottom-width',
  'border-bottom-style',
  'border-bottom-color',
  'border-left-width',
  'border-left-color',
  'border-top-left-radius',
] as const

/** The first element matching each selector under `root`, as computed style + text. */
async function look(page: Page, root: string, selectors: Record<string, string>) {
  return page.evaluate(
    ({ root, selectors, props }) => {
      const base = document.querySelector(root)
      if (!base) throw new Error(`no ${root}`)
      const out: Record<string, { style: Record<string, string>; text: string; icon: string | null }> = {}
      for (const [name, selector] of Object.entries(selectors)) {
        const el = base.querySelector(selector)
        if (!el) throw new Error(`no ${selector} under ${root}`)
        const computed = getComputedStyle(el)
        out[name] = {
          style: Object.fromEntries(props.map((prop) => [prop, computed.getPropertyValue(prop)])),
          text: (el.textContent ?? '').trim(),
          icon: el.querySelector('svg path')?.getAttribute('d') ?? null,
        }
      }
      return out
    },
    { root, selectors, props: [...PROPS] },
  )
}

/** Push the setup page and wait for the poller to publish it. */
async function seedSetup(riki: { push: (files: Record<string, string>) => void; url: string }): Promise<void> {
  riki.push({ 'guide/setup.md': PAGE })
  await expect.poll(async () => (await fetch(`${riki.url}/guide/setup`)).status, { timeout: 10_000 }).toBe(200)
}

const TARGETS = {
  'table cell': 'td',
  'alert title': '.markdown-alert-title',
  h1: 'h1',
  p: ':scope > p',
  'code block': 'pre',
}

test('the editor shows a page with the styles the reader sees', async ({ page, riki }) => {
  await seedSetup(riki)
  await page.goto(`${riki.url}/guide/setup`)
  const read = await look(page, 'main article.riki-prose', TARGETS)

  await page.locator('#riki-edit').click()
  await expect(page.locator('.riki-editor-root .ProseMirror')).toHaveAttribute('contenteditable', 'true')
  const edit = await look(page, '.riki-editor-root .ProseMirror', TARGETS)

  for (const name of Object.keys(TARGETS)) {
    expect.soft(edit[name]?.style, `${name} style`).toEqual(read[name]?.style)
  }
  expect(edit['alert title']?.text).toBe('Warning')
  expect(read['alert title']?.text).toBe('Warning')
  expect(edit['alert title']?.icon).toBeTruthy()
  expect(edit['alert title']?.icon).toBe(read['alert title']?.icon)
  // The rendered page and the editor are the same content box, at the same left edge.
  const left = (selector: string) => page.locator(selector).first().evaluate((el) => el.getBoundingClientRect().left)
  await page.locator('[data-control="cancel"]').click()
  const articleLeft = await left('main article h1')
  await page.locator('#riki-edit').click()
  await expect(page.locator('.riki-editor-root .ProseMirror')).toHaveAttribute('contenteditable', 'true')
  expect(await left('.riki-editor-root h1')).toBe(articleLeft)
})

test('Save and Cancel take the Edit slot while editing, and Cancel gives it back', async ({ page, riki }) => {
  await page.goto(`${riki.url}/guide`)
  await page.locator('#riki-edit').click()
  await expect(page.locator('header .actions [data-control="save"]')).toBeVisible()
  await expect(page.locator('#riki-edit')).toBeHidden()
  await page.locator('[data-control="cancel"]').click()
  await expect(page.locator('#riki-edit')).toBeVisible()
  await expect(page.locator('[data-control="save"]')).toHaveCount(0)
})

test('the theme toggle flips the page and survives a reload', async ({ page, riki }) => {
  await page.emulateMedia({ colorScheme: 'light' })
  await page.goto(`${riki.url}/`)
  const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  const light = await background()
  await page.locator('[data-riki-toggle="theme"]').click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  const dark = await background()
  expect(dark).not.toBe(light)
  await page.reload()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  expect(await background()).toBe(dark)
})

test('the system dark preference applies with nothing stored', async ({ page, riki }) => {
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.goto(`${riki.url}/`)
  await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.*/)
  const dark = await page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  await page.emulateMedia({ colorScheme: 'light' })
  expect(await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).not.toBe(dark)
})

test('a code block copies its text, with no CSP violations on the page', async ({ page, context, riki }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const violations: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') violations.push(message.text())
  })
  await seedSetup(riki)
  await page.goto(`${riki.url}/guide/setup`)
  await expect(page.locator('pre.syntax-highlighting code.language-bash span[class^="hl-"]').first()).toBeVisible()
  await page.locator('.riki-code').first().hover()
  await page.locator('.riki-copy').first().click()
  await expect(page.locator('.riki-copy').first()).toHaveAttribute('data-copied', '')
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('cargo build --release\n')
  expect(violations).toEqual([])
})

test('on a phone the sidebar is a drawer behind the menu button', async ({ page, riki }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto(`${riki.url}/guide`)
  const sidebar = page.locator('.riki-sidebar')
  await expect(sidebar).toBeHidden()
  await page.locator('.riki-nav-toggle').click()
  await expect(sidebar).toBeVisible()
  await expect(page.locator('.riki-nav-toggle')).toHaveAttribute('aria-expanded', 'true')
  await page.keyboard.press('Escape')
  await expect(sidebar).toBeHidden()
})
