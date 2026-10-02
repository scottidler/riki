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

test('the three-way theme switch pins dark across a reload, then returns to the system', async ({ page, riki }) => {
  await page.emulateMedia({ colorScheme: 'light' })
  await page.goto(`${riki.url}/`)
  const html = page.locator('html')
  const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  await expect(html).toHaveAttribute('data-theme-preference', 'system')
  await expect(page.locator('[data-riki-theme="system"]')).toHaveAttribute('aria-pressed', 'true')
  const light = await background()
  await page.locator('[data-riki-theme="dark"]').click()
  await expect(html).toHaveClass('dark')
  await expect(html).toHaveAttribute('data-theme-preference', 'dark')
  const dark = await background()
  expect(dark).not.toBe(light)
  await page.reload()
  await expect(html).toHaveClass('dark')
  await expect(page.locator('[data-riki-theme="dark"]')).toHaveAttribute('aria-pressed', 'true')
  expect(await background()).toBe(dark)
  await page.locator('[data-riki-theme="system"]').click()
  await expect(html).toHaveClass('light')
  await page.reload()
  await expect(html).toHaveAttribute('data-theme-preference', 'system')
  expect(await background()).toBe(light)
})

test('with nothing stored the page follows the system, live', async ({ page, riki }) => {
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.goto(`${riki.url}/`)
  const html = page.locator('html')
  await expect(html).toHaveAttribute('data-theme-preference', 'system')
  await expect(html).toHaveClass('dark')
  const dark = await page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  await page.emulateMedia({ colorScheme: 'light' })
  await expect(html).toHaveClass('light')
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

test('a nested sidebar group collapses and expands from its chevron', async ({ page, riki }) => {
  riki.push({ 'guide/deploy/rollback.md': '# Rolling back\n', 'guide/setup.md': '# Setup\n' })
  await expect.poll(async () => (await fetch(`${riki.url}/guide/setup`)).status, { timeout: 10_000 }).toBe(200)
  await page.goto(`${riki.url}/guide/setup`)
  const chevron = page.locator('.riki-sidebar [data-riki-toggle="group"]')
  const link = page.locator('.riki-sidebar a[href="/guide/deploy/rollback"]')
  await expect(chevron).toHaveAttribute('aria-expanded', 'false')
  await expect(link).toBeHidden()
  await chevron.click()
  await expect(chevron).toHaveAttribute('aria-expanded', 'true')
  await expect(link).toBeVisible()
  await chevron.click()
  await expect(link).toBeHidden()
})

/** The boxes of every visible code glyph in `block` that the copy button's box intersects, and
 *  whether the fade overlay is shown and covers the button. */
async function glyphsUnderTheButton(page: Page, block: number) {
  return page.evaluate((index) => {
    const wrap = document.querySelectorAll<HTMLElement>('article .riki-code')[index]
    if (!wrap) throw new Error(`no code block ${index}`)
    const pre = wrap.querySelector('pre')!
    const button = wrap.querySelector<HTMLElement>('.riki-copy')!.getBoundingClientRect()
    const box = pre.getBoundingClientRect()
    const hits: string[] = []
    const walker = document.createTreeWalker(pre, NodeFilter.SHOW_TEXT)
    const range = document.createRange()
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.textContent ?? ''
      for (let i = 0; i < text.length; i++) {
        if (/\s/.test(text[i]!)) continue
        range.setStart(node, i)
        range.setEnd(node, i + 1)
        for (const r of range.getClientRects()) {
          const visible = r.right > box.left && r.left < box.right && r.bottom > box.top && r.top < box.bottom
          const under = r.right > button.left && r.left < button.right && r.bottom > button.top && r.top < button.bottom
          if (visible && under) hits.push(text[i]!)
        }
      }
    }
    const fade = wrap.querySelector<HTMLElement>('.riki-code-fade')
    const shown = !!fade && getComputedStyle(fade).display !== 'none'
    const f = fade?.getBoundingClientRect()
    const covers =
      shown && !!f && f.left <= button.left && f.right >= button.right && f.top <= button.top && f.bottom >= button.bottom
    return { hits, fade: shown, covers, overflow: wrap.hasAttribute('data-overflow'), scrollLeft: pre.scrollLeft }
  }, block)
}

test('on a phone the copy button never sits on visible code', async ({ page, riki }) => {
  riki.push({
    'guide/long.md': [
      '# Long lines',
      '',
      '```bash',
      'curl -fsSL https://example.test/bootstrap.sh | bash -s -- --profile platform --with-editor --region us-east-1',
      '```',
      '',
      '```bash',
      'make test',
      '```',
      '',
    ].join('\n'),
  })
  await expect.poll(async () => (await fetch(`${riki.url}/guide/long`)).status, { timeout: 10_000 }).toBe(200)
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto(`${riki.url}/guide/long`)
  await expect(page.locator('.riki-copy')).toHaveCount(2)
  expect(await page.evaluate(() => document.documentElement.scrollWidth), 'the page never scrolls sideways').toBe(390)

  const long = await glyphsUnderTheButton(page, 0)
  expect(long.scrollLeft).toBe(0)
  expect(long.overflow, 'the long line scrolls inside its block').toBe(true)
  expect(long.fade && long.covers, `glyphs under the button (${long.hits.join('')}) sit under the fade`).toBe(true)

  const short = await glyphsUnderTheButton(page, 1)
  expect(short.overflow).toBe(false)
  expect(short.fade).toBe(false)
  expect(short.hits, 'no glyph of a short block is under the button').toEqual([])
  const chip = await page
    .locator('.riki-copy')
    .nth(1)
    .evaluate((el) => getComputedStyle(el).backgroundColor)
  expect(chip, 'a short block puts a solid chip behind the button').not.toBe('rgba(0, 0, 0, 0)')

  // Scrolled to the end, the long line's last characters clear the button.
  await page.locator('article .riki-code pre').first().evaluate((pre) => (pre.scrollLeft = pre.scrollWidth))
  const end = await page.evaluate(() => {
    const wrap = document.querySelector('article .riki-code')!
    const pre = wrap.querySelector('pre')!
    // Text nodes only: a range over the whole <pre> would also report the <code> element's box.
    const range = document.createRange()
    const rights: number[] = []
    const walker = document.createTreeWalker(pre, NodeFilter.SHOW_TEXT)
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      range.selectNodeContents(node)
      for (const r of range.getClientRects()) if (r.width > 0) rights.push(r.right)
    }
    const last = Math.max(...rights)
    return { last, button: wrap.querySelector('.riki-copy')!.getBoundingClientRect().left }
  })
  expect(end.last).toBeLessThanOrEqual(end.button)
})

test('a titled fence puts its title and the copy button in a header bar, and no language label anywhere', async ({ page, riki }) => {
  riki.push({ 'guide/titled.md': '# Titled\n\n```yaml title="app.yml"\nkey: value\n```\n' })
  await expect.poll(async () => (await fetch(`${riki.url}/guide/titled`)).status, { timeout: 10_000 }).toBe(200)
  await page.goto(`${riki.url}/guide/titled`)
  const head = page.locator('.riki-code-head')
  await expect(head.locator('.riki-code-title')).toHaveText('app.yml')
  await expect(head.locator('.riki-copy')).toBeVisible()
  const headBox = (await head.boundingBox())!
  const preBox = (await page.locator('.riki-code-titled pre').boundingBox())!
  expect(preBox.y).toBeGreaterThanOrEqual(headBox.y + headBox.height - 1)
  await expect(page.locator('article')).not.toContainText('yaml')
})
