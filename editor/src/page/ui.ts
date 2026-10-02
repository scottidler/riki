// The reader's page behaviors: the three-way theme switch, the mobile sidebar and nested
// sidebar groups, copy buttons on code blocks, and the "On this page" highlight. Served as /_riki/assets/riki.js under
// `script-src 'self'`; `main.ts` wires these to the document.

export const THEME_KEY = 'riki-theme'
/** What the reader picked in the three-way switch. `system` follows `prefers-color-scheme`. */
export type Preference = 'system' | 'light' | 'dark'
/** What the page shows. */
export type Theme = 'light' | 'dark'

const PREFERENCES: readonly Preference[] = ['system', 'light', 'dark']

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

function isPreference(value: unknown): value is Preference {
  return PREFERENCES.includes(value as Preference)
}

/** The stored preference; `system` when nothing (or nothing valid) is stored or storage throws. */
export function storedPreference(storage: ThemeStorage | null): Preference {
  try {
    const value = storage?.getItem(THEME_KEY)
    return isPreference(value) ? value : 'system'
  } catch {
    return 'system'
  }
}

/** Remember `preference`; false when storage refused it (the switch still applies for this page). */
export function storePreference(storage: ThemeStorage | null, preference: Preference): boolean {
  if (!storage) return false
  try {
    storage.setItem(THEME_KEY, preference)
    return true
  } catch {
    return false
  }
}

/** The theme a preference shows, given the system's. */
export function resolveTheme(preference: Preference, prefersDark: boolean): Theme {
  if (preference === 'system') return prefersDark ? 'dark' : 'light'
  return preference
}

/** Show `preference` on the root element: `class="light|dark"` (what the stylesheet keys on) and
 *  `data-theme-preference`, and mark the matching switch button pressed. Returns the theme. */
export function applyPreference(root: HTMLElement, preference: Preference, prefersDark: boolean): Theme {
  const theme = resolveTheme(preference, prefersDark)
  root.classList.toggle('dark', theme === 'dark')
  root.classList.toggle('light', theme === 'light')
  root.dataset['themePreference'] = preference
  for (const button of root.ownerDocument.querySelectorAll<HTMLElement>('[data-riki-theme]')) {
    button.setAttribute('aria-pressed', String(button.dataset['rikiTheme'] === preference))
  }
  return theme
}

/** The preference the root element shows now. */
export function currentPreference(root: HTMLElement): Preference {
  const value = root.dataset['themePreference']
  return isPreference(value) ? value : 'system'
}

/** Pick `preference` from the switch: show it and remember it. Returns the theme shown. */
export function choosePreference(
  root: HTMLElement,
  storage: ThemeStorage | null,
  preference: Preference,
  prefersDark: boolean,
): Theme {
  storePreference(storage, preference)
  return applyPreference(root, preference, prefersDark)
}

/** The preference a switch button stands for, or null when `el` is not one. */
export function switchChoice(el: Element | null): Preference | null {
  const value = el?.closest<HTMLElement>('[data-riki-theme]')?.dataset['rikiTheme']
  return isPreference(value) ? value : null
}

/** Expand or collapse the nested sidebar group a chevron button controls. Returns the new state. */
export function toggleGroup(button: HTMLElement): boolean {
  const open = button.getAttribute('aria-expanded') !== 'true'
  button.setAttribute('aria-expanded', String(open))
  const id = button.getAttribute('aria-controls')
  const list = id ? button.ownerDocument.getElementById(id) : null
  if (list) list.hidden = !open
  return open
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

/** Give every server-rendered code frame (`.riki-code`) under the article a copy button, in the
 *  header bar when the fence has a title, else floating top-right over a fade (shown only when
 *  the block scrolls sideways). Idempotent; returns how many frames it decorated. The editor's
 *  own code blocks are not under the article. */
export function decorateCodeBlocks(root: ParentNode): number {
  let count = 0
  for (const wrap of root.querySelectorAll<HTMLElement>('article.riki-prose .riki-code')) {
    if (wrap.querySelector(':scope > .riki-copy, :scope > .riki-code-head > .riki-copy')) continue
    const doc = wrap.ownerDocument
    const button = doc.createElement('button')
    button.type = 'button'
    button.className = 'riki-copy'
    button.setAttribute('aria-label', 'Copy code')
    button.title = 'Copy'
    button.innerHTML = COPY_ICONS
    const head = wrap.querySelector(':scope > .riki-code-head')
    if (head) {
      head.append(button)
    } else {
      const fade = doc.createElement('span')
      fade.className = 'riki-code-fade'
      fade.setAttribute('aria-hidden', 'true')
      wrap.prepend(fade, button)
    }
    markOverflow(wrap)
    count++
  }
  return count
}

/** Flag a code frame whose code is wider than its box (`data-overflow`): the stylesheet then
 *  fades the code under the copy button instead of giving the button a solid chip. */
export function markOverflow(wrap: HTMLElement): boolean {
  const pre = wrap.querySelector('pre')
  const overflows = !!pre && pre.scrollWidth > pre.clientWidth + 1
  wrap.toggleAttribute('data-overflow', overflows)
  return overflows
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
