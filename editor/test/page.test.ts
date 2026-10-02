import { describe, expect, it, vi } from 'vitest'
import {
  THEME_KEY,
  activeTocId,
  applyPreference,
  choosePreference,
  copyFrom,
  copyText,
  currentPreference,
  decorateCodeBlocks,
  markOverflow,
  markToc,
  resolveTheme,
  setNavOpen,
  storePreference,
  storedPreference,
  switchChoice,
  toggleGroup,
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
  const switchHtml =
    '<button data-riki-theme="system"></button><button data-riki-theme="light"></button><button data-riki-theme="dark"></button>'

  it('reads system, light, or dark from storage, defaulting to system', () => {
    expect(storedPreference(memory({ [THEME_KEY]: 'dark' }))).toBe('dark')
    expect(storedPreference(memory({ [THEME_KEY]: 'light' }))).toBe('light')
    expect(storedPreference(memory({ [THEME_KEY]: 'system' }))).toBe('system')
    expect(storedPreference(memory({ [THEME_KEY]: 'purple' }))).toBe('system')
    expect(storedPreference(memory())).toBe('system')
    expect(storedPreference(null)).toBe('system')
  })

  it('treats storage that throws as system, and reports a failed write', () => {
    expect(storedPreference(throwing)).toBe('system')
    expect(storePreference(throwing, 'dark')).toBe(false)
    expect(storePreference(null, 'dark')).toBe(false)
    const store = memory()
    expect(storePreference(store, 'system')).toBe(true)
    expect(store.data[THEME_KEY]).toBe('system')
  })

  it('resolves system against the OS and pins light or dark', () => {
    expect(resolveTheme('system', true)).toBe('dark')
    expect(resolveTheme('system', false)).toBe('light')
    expect(resolveTheme('light', true)).toBe('light')
    expect(resolveTheme('dark', false)).toBe('dark')
  })

  it('sets the html class and preference, and presses the matching switch button', () => {
    document.body.innerHTML = switchHtml
    const root = document.documentElement
    expect(applyPreference(root, 'system', true)).toBe('dark')
    expect(root.classList.contains('dark')).toBe(true)
    expect(root.classList.contains('light')).toBe(false)
    expect(root.dataset['themePreference']).toBe('system')
    expect(currentPreference(root)).toBe('system')
    const pressed = () =>
      [...document.querySelectorAll('[data-riki-theme]')].map((b) => b.getAttribute('aria-pressed'))
    expect(pressed()).toEqual(['true', 'false', 'false'])
    applyPreference(root, 'light', true)
    expect(root.classList.contains('light')).toBe(true)
    expect(root.classList.contains('dark')).toBe(false)
    expect(pressed()).toEqual(['false', 'true', 'false'])
  })

  it('choosing from the switch applies and remembers the choice', () => {
    document.body.innerHTML = switchHtml
    const root = document.documentElement
    const store = memory()
    expect(choosePreference(root, store, 'dark', false)).toBe('dark')
    expect(store.data[THEME_KEY]).toBe('dark')
    expect(currentPreference(root)).toBe('dark')
  })

  it('still switches the page when storage throws', () => {
    const root = document.documentElement
    expect(choosePreference(root, throwing, 'dark', false)).toBe('dark')
    expect(root.classList.contains('dark')).toBe(true)
  })

  it('knows which switch button was clicked', () => {
    document.body.innerHTML = '<button data-riki-theme="light"><svg><path></path></svg></button><button data-riki-theme="x"></button>'
    expect(switchChoice(document.querySelector('path'))).toBe('light')
    expect(switchChoice(document.querySelector('[data-riki-theme="x"]'))).toBeNull()
    expect(switchChoice(null)).toBeNull()
  })
})

describe('sidebar groups', () => {
  it('a chevron expands and collapses the list it controls', () => {
    document.body.innerHTML =
      '<button data-riki-toggle="group" aria-expanded="false" aria-controls="g"></button><ul id="g" hidden></ul>'
    const button = document.querySelector<HTMLElement>('button')!
    const list = document.getElementById('g')!
    expect(toggleGroup(button)).toBe(true)
    expect(button.getAttribute('aria-expanded')).toBe('true')
    expect(list.hidden).toBe(false)
    expect(toggleGroup(button)).toBe(false)
    expect(list.hidden).toBe(true)
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
      '<article class="riki-prose"><div class="riki-code"><pre class="syntax-highlighting"><code class="language-bash">echo hi\n</code></pre></div>' +
      '<div class="riki-code riki-code-titled"><div class="riki-code-head"><span class="riki-code-title">x.sh</span></div><pre><code>plain</code></pre></div></article>' +
      '<div class="riki-editor"><div class="riki-code"><pre><code>in the editor</code></pre></div></div>'
  }

  it('gives article code frames one copy button each, leaving the editor alone', () => {
    page()
    expect(decorateCodeBlocks(document)).toBe(2)
    expect(decorateCodeBlocks(document)).toBe(0)
    const [plain, titled] = document.querySelectorAll('article .riki-code')
    expect(plain?.querySelectorAll('.riki-copy')).toHaveLength(1)
    expect(plain?.querySelector(':scope > .riki-copy')?.getAttribute('aria-label')).toBe('Copy code')
    expect(titled?.querySelector('.riki-code-head > .riki-copy')).not.toBeNull()
    expect(document.querySelector('.riki-editor .riki-copy')).toBeNull()
  })

  it('an untitled block gets the fade, a titled one does not, and no language label', () => {
    page()
    decorateCodeBlocks(document)
    const [plain, titled] = document.querySelectorAll<HTMLElement>('article .riki-code')
    expect(plain?.querySelector(':scope > .riki-code-fade')?.getAttribute('aria-hidden')).toBe('true')
    expect(titled?.querySelector('.riki-code-fade')).toBeNull()
    expect(plain?.dataset['lang']).toBeUndefined()
    expect(document.body.textContent).not.toContain('bash')
  })

  it('flags a block whose code is wider than its box', () => {
    page()
    const wrap = document.querySelector<HTMLElement>('article .riki-code')!
    const pre = wrap.querySelector('pre')!
    Object.defineProperty(pre, 'clientWidth', { value: 300, configurable: true })
    Object.defineProperty(pre, 'scrollWidth', { value: 800, configurable: true })
    expect(markOverflow(wrap)).toBe(true)
    expect(wrap.hasAttribute('data-overflow')).toBe(true)
    Object.defineProperty(pre, 'scrollWidth', { value: 300, configurable: true })
    expect(markOverflow(wrap)).toBe(false)
    expect(wrap.hasAttribute('data-overflow')).toBe(false)
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

  it('a titled block copies its code, not its title', async () => {
    page()
    decorateCodeBlocks(document)
    const writeText = vi.fn(async () => {})
    const button = document.querySelector<HTMLElement>('.riki-code-head .riki-copy')!
    expect(await copyFrom(button, { writeText })).toBe(true)
    expect(writeText).toHaveBeenCalledWith('plain')
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
