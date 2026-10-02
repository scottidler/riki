import { afterEach, describe, expect, it, vi } from 'vitest'
import { DEBOUNCE_MS, fetchSearch, hitUrl, isPaletteShortcut, markedNodes, openPalette, paletteIsOpen } from '../src/page/search'

const json = (status: number, body: unknown): Response =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

const hit = (over: Record<string, unknown> = {}) => ({
  path: 'a.md',
  url: '/a',
  title: 'A',
  heading: null,
  anchor: null,
  snippet: '',
  marks: [],
  ...over,
})

const settle = () => new Promise((resolve) => setTimeout(resolve, 0))
const press = (key: string): void => {
  document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
}
const type = async (text: string): Promise<void> => {
  const input = document.querySelector<HTMLInputElement>('.riki-search input')!
  input.value = text
  input.dispatchEvent(new Event('input'))
  await new Promise((resolve) => setTimeout(resolve, DEBOUNCE_MS + 30))
}

afterEach(() => {
  if (paletteIsOpen()) press('Escape')
  document.body.innerHTML = ''
})

describe('hitUrl', () => {
  it('appends the anchor when there is one', () => {
    expect(hitUrl({ url: '/ref/tables', anchor: 'syntax' })).toBe('/ref/tables#syntax')
    expect(hitUrl({ url: '/ref/tables', anchor: null })).toBe('/ref/tables')
  })
})

describe('markedNodes', () => {
  const render = (text: string, marks: [number, number][]): string => {
    const div = document.createElement('div')
    div.append(...markedNodes(text, marks))
    return div.innerHTML
  }
  it('wraps each range in <mark> and keeps the rest as text', () => {
    expect(render('a table here', [[2, 7]])).toBe('a <mark>table</mark> here')
  })
  it('counts UTF-16 code units, so an emoji before the range shifts it by two', () => {
    expect(render('😀 table', [[3, 8]])).toBe('😀 <mark>table</mark>')
  })
  it('never makes HTML from the text', () => {
    const div = document.createElement('div')
    div.append(...markedNodes('<img src=x onerror=alert(1)> tabl', [[29, 33]]))
    expect(div.querySelector('img')).toBeNull()
    expect(div.querySelector('mark')?.textContent).toBe('tabl')
    expect(div.textContent).toBe('<img src=x onerror=alert(1)> tabl')
  })
  it('skips overlapping, reversed, and out-of-range marks instead of trusting them', () => {
    expect(render('abcdef', [[1, 3], [2, 4], [5, 99], [4, 4]])).toBe('a<mark>bc</mark>def')
  })
})

describe('isPaletteShortcut', () => {
  const key = (init: KeyboardEventInit, target: Element = document.body): KeyboardEvent => {
    const event = new KeyboardEvent('keydown', { bubbles: true, ...init })
    Object.defineProperty(event, 'target', { value: target })
    return event
  }
  it('accepts Ctrl+K and Cmd+K outside the editor', () => {
    expect(isPaletteShortcut(key({ key: 'k', ctrlKey: true }))).toBe(true)
    expect(isPaletteShortcut(key({ key: 'K', metaKey: true }))).toBe(true)
  })
  it('ignores a bare k, other modifiers, and any key typed inside the editor', () => {
    expect(isPaletteShortcut(key({ key: 'k' }))).toBe(false)
    expect(isPaletteShortcut(key({ key: 'k', ctrlKey: true, shiftKey: true }))).toBe(false)
    document.body.innerHTML = '<div class="riki-editor-root"><div class="ProseMirror"><p id="p">x</p></div></div>'
    expect(isPaletteShortcut(key({ key: 'k', ctrlKey: true }, document.getElementById('p')!))).toBe(false)
  })
})

describe('fetchSearch', () => {
  it('encodes the query and returns the hits', async () => {
    const f = vi.fn().mockResolvedValue(json(200, { hits: [hit()] }))
    const reply = await fetchSearch('a b&c', f)
    expect(f.mock.calls[0]?.[0]).toBe('/_riki/api/search?q=a%20b%26c')
    expect(reply).toMatchObject({ ok: true })
  })
  it('turns an error status, a network failure, and junk into a message', async () => {
    expect(await fetchSearch('x', async () => json(503, { error: 'not ready' }))).toEqual({ ok: false, message: 'not ready' })
    expect(await fetchSearch('x', async () => Promise.reject(new Error('offline')))).toMatchObject({ ok: false })
    expect(await fetchSearch('x', async () => json(200, { nope: 1 }))).toMatchObject({ ok: false })
  })
})

describe('the palette', () => {
  const hits = [
    hit({ path: 'a.md', url: '/a', title: 'Alpha', snippet: 'the table of alpha', marks: [[4, 9]] }),
    hit({ path: 'ref/b.md', url: '/ref/b', title: 'Beta', heading: 'Syntax', anchor: 'syntax-1', snippet: 'tables' , marks: [[0, 4]] }),
  ]

  it('Down then Enter navigates to the second hit url#anchor and closes', async () => {
    const f = vi.fn().mockResolvedValue(json(200, { hits }))
    const navigate = vi.fn()
    openPalette(navigate, f)
    await type('tabl')
    expect(document.querySelectorAll('.riki-search-hit')).toHaveLength(2)
    press('ArrowDown')
    press('Enter')
    expect(navigate).toHaveBeenCalledWith('/ref/b#syntax-1')
    expect(paletteIsOpen()).toBe(false)
    expect(document.querySelector('.riki-search')).toBeNull()
  })

  it('Enter with no move opens the first hit; Up from the first wraps to the last', async () => {
    const f = vi.fn().mockResolvedValue(json(200, { hits }))
    const navigate = vi.fn()
    openPalette(navigate, f)
    await type('tabl')
    press('ArrowUp')
    expect(document.querySelectorAll('.riki-search-hit')[1]?.getAttribute('aria-selected')).toBe('true')
    press('ArrowDown')
    press('Enter')
    expect(navigate).toHaveBeenCalledWith('/a')
  })

  it('Escape closes without navigating', async () => {
    const navigate = vi.fn()
    openPalette(navigate, vi.fn().mockResolvedValue(json(200, { hits })))
    expect(document.querySelector('.riki-search')).not.toBeNull()
    press('Escape')
    expect(document.querySelector('.riki-search')).toBeNull()
    expect(navigate).not.toHaveBeenCalled()
  })

  it('shows title, heading, path, and the snippet with <mark> built from text nodes', async () => {
    openPalette(vi.fn(), vi.fn().mockResolvedValue(json(200, { hits })))
    await type('tabl')
    const second = document.querySelectorAll('.riki-search-hit')[1]!
    expect(second.querySelector('.riki-search-title')?.textContent).toBe('Beta')
    expect(second.querySelector('.riki-search-heading')?.textContent).toBe('Syntax')
    expect(second.querySelector('.riki-search-path')?.textContent).toBe('ref/b.md')
    expect(second.querySelector('mark')?.textContent).toBe('tabl')
  })

  it('a hit with HTML in its fields renders it as text', async () => {
    const evil = hit({ title: '<b>x</b>', snippet: '<img src=x>', marks: [] })
    openPalette(vi.fn(), vi.fn().mockResolvedValue(json(200, { hits: [evil] })))
    await type('x')
    expect(document.querySelector('.riki-search img, .riki-search b')).toBeNull()
    expect(document.querySelector('.riki-search-title')?.textContent).toBe('<b>x</b>')
  })

  it('debounces: typing fast sends one request for the last text; a blank query sends none', async () => {
    const f = vi.fn().mockResolvedValue(json(200, { hits: [] }))
    openPalette(vi.fn(), f)
    const input = document.querySelector<HTMLInputElement>('.riki-search input')!
    for (const text of ['t', 'ta', 'tab']) {
      input.value = text
      input.dispatchEvent(new Event('input'))
    }
    await new Promise((resolve) => setTimeout(resolve, DEBOUNCE_MS + 30))
    expect(f).toHaveBeenCalledTimes(1)
    expect(f.mock.calls[0]?.[0]).toBe('/_riki/api/search?q=tab')
    expect(document.querySelector('.riki-search-status')?.textContent).toBe('No pages match.')
    await type('  ')
    expect(f).toHaveBeenCalledTimes(1)
  })

  it('shows the server error and Enter does nothing with no hits', async () => {
    const navigate = vi.fn()
    openPalette(navigate, vi.fn().mockResolvedValue(json(503, { error: 'nothing published yet' })))
    await type('x')
    await settle()
    expect(document.querySelector('.riki-search-status')?.textContent).toBe('nothing published yet')
    press('Enter')
    expect(navigate).not.toHaveBeenCalled()
    expect(paletteIsOpen()).toBe(true)
  })

  it('opening twice keeps one palette; a click on a hit navigates', async () => {
    const navigate = vi.fn()
    openPalette(navigate, vi.fn().mockResolvedValue(json(200, { hits })))
    openPalette(navigate)
    expect(document.querySelectorAll('.riki-search')).toHaveLength(1)
    await type('tabl')
    ;(document.querySelectorAll('.riki-search-title')[1] as HTMLElement).click()
    expect(navigate).toHaveBeenCalledWith('/ref/b#syntax-1')
  })
})
