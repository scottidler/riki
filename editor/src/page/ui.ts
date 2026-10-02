// The reader's page behaviors: dark-mode toggle, the mobile sidebar, copy buttons on code
// blocks, and the "On this page" highlight. Served as /_riki/assets/riki.js under
// `script-src 'self'`; `main.ts` wires these to the document.

export const THEME_KEY = 'riki-theme'
export type Theme = 'light' | 'dark'

/** The subset of `Storage` the theme needs. */
export type ThemeStorage = Pick<Storage, 'getItem' | 'setItem'>

/** `window.localStorage`, or null where reading it throws (storage disabled, sandboxed frame). */
export function localStore(): ThemeStorage | null {
  try {
    return window.localStorage
  } catch {
    return null
  }
}

/** The theme the reader picked, or null for "follow the system". Storage errors count as null. */
export function storedTheme(storage: ThemeStorage | null): Theme | null {
  try {
    const value = storage?.getItem(THEME_KEY)
    return value === 'light' || value === 'dark' ? value : null
  } catch {
    return null
  }
}

/** Remember `theme`; false when storage refused it (the toggle still applies for this page). */
export function storeTheme(storage: ThemeStorage | null, theme: Theme): boolean {
  if (!storage) return false
  try {
    storage.setItem(THEME_KEY, theme)
    return true
  } catch {
    return false
  }
}

/** Pin `theme` on the root element, or clear the pin so CSS follows `prefers-color-scheme`. */
export function applyTheme(root: HTMLElement, theme: Theme | null): void {
  if (theme) root.dataset['theme'] = theme
  else delete root.dataset['theme']
}

/** The theme the page shows now: the pinned one, else the system's. */
export function effectiveTheme(root: HTMLElement, prefersDark: boolean): Theme {
  const pinned = root.dataset['theme']
  if (pinned === 'light' || pinned === 'dark') return pinned
  return prefersDark ? 'dark' : 'light'
}

/** Flip the shown theme, pin it, and remember it. Returns the new theme. */
export function toggleTheme(root: HTMLElement, storage: ThemeStorage | null, prefersDark: boolean): Theme {
  const next: Theme = effectiveTheme(root, prefersDark) === 'dark' ? 'light' : 'dark'
  applyTheme(root, next)
  storeTheme(storage, next)
  return next
}

/** Open or close the sidebar drawer (narrow screens). */
export function setNavOpen(doc: Document, open: boolean): void {
  doc.body.classList.toggle('riki-nav-open', open)
  for (const toggle of doc.querySelectorAll<HTMLElement>('.riki-nav-toggle')) {
    toggle.setAttribute('aria-expanded', String(open))
  }
}

const COPY_ICONS =
  '<svg class="riki-clip" viewBox="0 0 16 16" aria-hidden="true"><rect x="5.25" y="5.25" width="8" height="8" rx="1.75"/>' +
  '<path d="M10.75 5.25v-1a1.5 1.5 0 0 0-1.5-1.5h-4.5a1.5 1.5 0 0 0-1.5 1.5v4.5a1.5 1.5 0 0 0 1.5 1.5h1"/></svg>' +
  '<svg class="riki-check" viewBox="0 0 16 16" aria-hidden="true"><path d="m3.5 8.5 3 3 6-6.5"/></svg>'

/** The language a rendered code block declares (`<code class="language-x">`), if any. */
export function codeLanguage(pre: Element): string | null {
  const code = pre.querySelector('code')
  for (const name of code?.classList ?? []) {
    if (name.startsWith('language-') && name.length > 'language-'.length) return name.slice('language-'.length)
  }
  return null
}

/** Wrap every rendered `<pre>` under `root` in `.riki-code` with a copy button. Idempotent;
 *  returns how many blocks it decorated. The editor's own code blocks are not under the article. */
export function decorateCodeBlocks(root: ParentNode): number {
  let count = 0
  for (const pre of root.querySelectorAll<HTMLPreElement>('article.riki-prose pre')) {
    if (pre.parentElement?.classList.contains('riki-code')) continue
    const doc = pre.ownerDocument
    const wrap = doc.createElement('div')
    wrap.className = 'riki-code'
    const lang = codeLanguage(pre)
    if (lang) wrap.dataset['lang'] = lang
    const button = doc.createElement('button')
    button.type = 'button'
    button.className = 'riki-copy'
    button.setAttribute('aria-label', 'Copy code')
    button.title = 'Copy'
    button.innerHTML = COPY_ICONS
    pre.replaceWith(wrap)
    wrap.append(button, pre)
    count++
  }
  return count
}

/** Put `text` on the clipboard. False when no clipboard path worked. */
export async function copyText(text: string, clipboard: Pick<Clipboard, 'writeText'> | undefined): Promise<boolean> {
  if (clipboard) {
    try {
      await clipboard.writeText(text)
      return true
    } catch {
      // fall through to the selection path
    }
  }
  const doc = document
  const area = doc.createElement('textarea')
  area.value = text
  area.setAttribute('readonly', '')
  area.style.position = 'fixed'
  area.style.opacity = '0'
  doc.body.append(area)
  area.select()
  try {
    return doc.execCommand('copy')
  } catch {
    return false
  } finally {
    area.remove()
  }
}

/** Copy the code block the button sits on, and flag the button for a moment. */
export async function copyFrom(button: HTMLElement, clipboard: Pick<Clipboard, 'writeText'> | undefined): Promise<boolean> {
  const code = button.closest('.riki-code')?.querySelector('pre')
  if (!code) return false
  const ok = await copyText(code.textContent ?? '', clipboard)
  if (ok) {
    button.dataset['copied'] = ''
    button.setAttribute('aria-label', 'Copied')
    setTimeout(() => {
      delete button.dataset['copied']
      button.setAttribute('aria-label', 'Copy code')
    }, 1600)
  }
  return ok
}

/** The TOC link for the heading nearest above the top of the viewport (`offset` px down). */
export function activeTocId(headings: readonly HTMLElement[], offset: number): string | null {
  let active: string | null = headings[0]?.id ?? null
  for (const heading of headings) {
    if (heading.getBoundingClientRect().top - offset <= 0) active = heading.id
    else break
  }
  return active
}

/** Mark the TOC link whose heading the reader is in. */
export function markToc(doc: Document, id: string | null): void {
  for (const link of doc.querySelectorAll<HTMLAnchorElement>('.riki-toc a')) {
    const target = decodeURIComponent(link.hash.slice(1))
    link.classList.toggle('active', id !== null && target === id)
  }
}

/** The headings the TOC lists, in page order. */
export function tocHeadings(doc: Document): HTMLElement[] {
  const ids = new Set(
    [...doc.querySelectorAll<HTMLAnchorElement>('.riki-toc a')].map((link) => decodeURIComponent(link.hash.slice(1))),
  )
  return [...doc.querySelectorAll<HTMLElement>('article.riki-prose :is(h2, h3)[id]')].filter((h) => ids.has(h.id))
}
