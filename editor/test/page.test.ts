import { describe, expect, it, vi } from 'vitest'
import {
  THEME_KEY,
  activeTocId,
  applyTheme,
  codeLanguage,
  copyFrom,
  copyText,
  decorateCodeBlocks,
  effectiveTheme,
  markToc,
  setNavOpen,
  storeTheme,
  storedTheme,
  toggleTheme,
  tocHeadings,
} from '../src/page/ui'
import type { ThemeStorage } from '../src/page/ui'

function memory(initial: Record<string, string> = {}): ThemeStorage & { data: Record<string, string> } {
  const data = { ...initial }
  return {
    data,
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => {
      data[key] = value
    },
  }
}

const throwing: ThemeStorage = {
  getItem: () => {
    throw new DOMException('denied', 'SecurityError')
  },
  setItem: () => {
    throw new DOMException('full', 'QuotaExceededError')
  },
}

describe('theme', () => {
  it('reads only light or dark from storage', () => {
    expect(storedTheme(memory({ [THEME_KEY]: 'dark' }))).toBe('dark')
    expect(storedTheme(memory({ [THEME_KEY]: 'light' }))).toBe('light')
    expect(storedTheme(memory({ [THEME_KEY]: 'purple' }))).toBeNull()
    expect(storedTheme(memory())).toBeNull()
    expect(storedTheme(null)).toBeNull()
  })

  it('treats storage that throws as no preference, and reports a failed write', () => {
    expect(storedTheme(throwing)).toBeNull()
    expect(storeTheme(throwing, 'dark')).toBe(false)
    expect(storeTheme(null, 'dark')).toBe(false)
    const store = memory()
    expect(storeTheme(store, 'dark')).toBe(true)
    expect(store.data[THEME_KEY]).toBe('dark')
  })

  it('pins and clears the theme on the root element', () => {
    const root = document.createElement('html')
    applyTheme(root, 'dark')
    expect(root.dataset['theme']).toBe('dark')
    applyTheme(root, null)
    expect(root.hasAttribute('data-theme')).toBe(false)
  })

  it('follows the system until pinned', () => {
    const root = document.createElement('html')
    expect(effectiveTheme(root, true)).toBe('dark')
    expect(effectiveTheme(root, false)).toBe('light')
    applyTheme(root, 'light')
    expect(effectiveTheme(root, true)).toBe('light')
  })

  it('toggles away from what is shown and remembers it', () => {
    const root = document.createElement('html')
    const store = memory()
    expect(toggleTheme(root, store, true)).toBe('light')
    expect(root.dataset['theme']).toBe('light')
    expect(store.data[THEME_KEY]).toBe('light')
    expect(toggleTheme(root, store, true)).toBe('dark')
    expect(store.data[THEME_KEY]).toBe('dark')
  })

  it('still toggles the page when storage throws', () => {
    const root = document.createElement('html')
    expect(toggleTheme(root, throwing, false)).toBe('dark')
    expect(root.dataset['theme']).toBe('dark')
  })
})

describe('sidebar drawer', () => {
  it('opens and closes, keeping aria-expanded in step', () => {
    document.body.innerHTML = '<button class="riki-nav-toggle" aria-expanded="false"></button>'
    setNavOpen(document, true)
    expect(document.body.classList.contains('riki-nav-open')).toBe(true)
    expect(document.querySelector('.riki-nav-toggle')?.getAttribute('aria-expanded')).toBe('true')
    setNavOpen(document, false)
    expect(document.body.classList.contains('riki-nav-open')).toBe(false)
    expect(document.querySelector('.riki-nav-toggle')?.getAttribute('aria-expanded')).toBe('false')
  })
})

describe('code blocks', () => {
  const page = () => {
    document.body.innerHTML =
      '<article class="riki-prose"><pre class="syntax-highlighting"><code class="language-bash">echo hi\n</code></pre>' +
      '<pre><code>plain</code></pre></article>' +
      '<div class="riki-editor"><pre><code>in the editor</code></pre></div>'
  }

  it('names the language a block declares', () => {
    page()
    const [bash, plain] = document.querySelectorAll('pre')
    expect(codeLanguage(bash!)).toBe('bash')
    expect(codeLanguage(plain!)).toBeNull()
  })

  it('wraps article code blocks once, leaving the editor alone', () => {
    page()
    expect(decorateCodeBlocks(document)).toBe(2)
    expect(decorateCodeBlocks(document)).toBe(0)
    const wraps = document.querySelectorAll('.riki-code')
    expect(wraps).toHaveLength(2)
    expect((wraps[0] as HTMLElement).dataset['lang']).toBe('bash')
    expect((wraps[1] as HTMLElement).dataset['lang']).toBeUndefined()
    expect(wraps[0]?.querySelector('button.riki-copy')?.getAttribute('aria-label')).toBe('Copy code')
    expect(document.querySelector('.riki-editor .riki-code')).toBeNull()
  })

  it('copies the code text, not the button', async () => {
    page()
    decorateCodeBlocks(document)
    const writeText = vi.fn(async () => {})
    const button = document.querySelector<HTMLElement>('.riki-copy')!
    expect(await copyFrom(button, { writeText })).toBe(true)
    expect(writeText).toHaveBeenCalledWith('echo hi\n')
    expect(button.hasAttribute('data-copied')).toBe(true)
  })

  it('reports a copy that no clipboard path took', async () => {
    document.execCommand = () => false
    const writeText = vi.fn(async () => {
      throw new Error('denied')
    })
    expect(await copyText('x', { writeText })).toBe(false)
    expect(await copyText('x', undefined)).toBe(false)
    expect(document.querySelector('textarea')).toBeNull()
  })

  it('a button outside a code block copies nothing', async () => {
    document.body.innerHTML = '<button class="riki-copy"></button>'
    expect(await copyFrom(document.querySelector<HTMLElement>('button')!, { writeText: async () => {} })).toBe(false)
  })
})

describe('on this page', () => {
  const at = (el: Element, top: number) => {
    el.getBoundingClientRect = () => ({ top }) as DOMRect
  }

  it('picks the last heading above the offset, else the first', () => {
    document.body.innerHTML = '<h2 id="a"></h2><h2 id="b"></h2><h3 id="c"></h3>'
    const [a, b, c] = [...document.querySelectorAll<HTMLElement>('h2, h3')]
    at(a!, 400)
    at(b!, 800)
    at(c!, 1200)
    expect(activeTocId([a!, b!, c!], 100)).toBe('a')
    at(a!, -300)
    at(b!, 50)
    expect(activeTocId([a!, b!, c!], 100)).toBe('b')
    expect(activeTocId([], 100)).toBeNull()
  })

  it('marks the link for the active heading and finds only listed headings', () => {
    document.body.innerHTML =
      '<article class="riki-prose"><h2 id="install">Install</h2><h2 id="ünï">U</h2><h2 id="unlisted">X</h2></article>' +
      '<aside class="riki-toc"><a href="#install">Install</a><a href="#%C3%BCn%C3%AF">U</a></aside>'
    expect(tocHeadings(document).map((h) => h.id)).toEqual(['install', 'ünï'])
    markToc(document, 'ünï')
    const links = [...document.querySelectorAll('.riki-toc a')]
    expect(links.map((l) => l.classList.contains('active'))).toEqual([false, true])
    markToc(document, null)
    expect(links.some((l) => l.classList.contains('active'))).toBe(false)
  })
})
