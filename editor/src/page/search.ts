// The Ctrl+K search palette. Results come from GET /_riki/api/search; every title, heading, path
// and snippet reaches the page as a DOM text node, and `marks` (UTF-16 offsets into the snippet)
// become <mark> elements around slices of it. Nothing here touches innerHTML.

import type { Reply } from './ops'

type Fetch = typeof fetch
type Navigate = (url: string) => void

export const SEARCH_API = '/_riki/api/search'
export const DEBOUNCE_MS = 150

export interface Hit {
  path: string
  url: string
  title: string
  heading: string | null
  anchor: string | null
  snippet: string
  marks: [number, number][]
}

export async function fetchSearch(query: string, fetchImpl: Fetch = fetch): Promise<Reply<{ hits: Hit[] }>> {
  let response: Response
  try {
    response = await fetchImpl(`${SEARCH_API}?q=${encodeURIComponent(query)}`, { headers: { Accept: 'application/json' } })
  } catch (err) {
    return { ok: false, message: `search did not reach the server: ${String(err)}` }
  }
  const json = (await response.json().catch(() => null)) as { hits?: unknown; error?: unknown } | null
  if (!response.ok) return { ok: false, message: typeof json?.error === 'string' ? json.error : `HTTP ${response.status}` }
  if (!json || !Array.isArray(json.hits)) return { ok: false, message: 'search answered with a malformed response' }
  return { ok: true, value: { hits: json.hits as Hit[] } }
}

/** Where Enter on a hit goes: its URL, plus `#anchor` when the hit is under a heading. */
export function hitUrl(hit: Pick<Hit, 'url' | 'anchor'>): string {
  return hit.anchor ? `${hit.url}#${hit.anchor}` : hit.url
}

/** `text` as text nodes with each mark range wrapped in <mark>. Ranges are UTF-16 code units, which
 *  is what JS string indexing counts; ranges that are out of order, overlap, or fall outside the
 *  text are skipped rather than trusted. */
export function markedNodes(text: string, marks: readonly [number, number][]): Node[] {
  const nodes: Node[] = []
  let at = 0
  for (const [start, end] of marks) {
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < at || end <= start || end > text.length) continue
    if (start > at) nodes.push(document.createTextNode(text.slice(at, start)))
    const mark = document.createElement('mark')
    mark.textContent = text.slice(start, end)
    nodes.push(mark)
    at = end
  }
  if (at < text.length) nodes.push(document.createTextNode(text.slice(at)))
  return nodes
}

function line(className: string, text: string): HTMLElement {
  const element = document.createElement('span')
  element.className = className
  element.textContent = text
  return element
}

function renderHit(hit: Hit, index: number): HTMLElement {
  const item = document.createElement('li')
  item.id = `riki-search-hit-${index}`
  item.setAttribute('role', 'option')
  item.className = 'riki-search-hit'
  item.dataset['url'] = hitUrl(hit)
  const head = document.createElement('span')
  head.className = 'riki-search-head'
  head.append(line('riki-search-title', hit.title))
  if (hit.heading) head.append(line('riki-search-heading', hit.heading))
  head.append(line('riki-search-path', hit.path))
  item.append(head)
  if (hit.snippet !== '') {
    const snippet = document.createElement('span')
    snippet.className = 'riki-search-snippet'
    snippet.append(...markedNodes(hit.snippet, hit.marks))
    item.append(snippet)
  }
  return item
}

let current: { close(): void } | null = null

export function paletteIsOpen(): boolean {
  return current !== null
}

/** Open the palette (or focus the open one). ↑/↓ move, Enter opens the selected hit, Esc closes. */
export function openPalette(navigate: Navigate, fetchImpl: Fetch = fetch): void {
  if (current) {
    document.querySelector<HTMLInputElement>('.riki-search input')?.focus()
    return
  }
  const backdrop = document.createElement('div')
  backdrop.className = 'riki-dialog-backdrop riki-search-backdrop'
  const box = document.createElement('div')
  box.className = 'riki-dialog riki-search'
  box.setAttribute('role', 'dialog')
  box.setAttribute('aria-modal', 'true')
  box.setAttribute('aria-label', 'Search')
  const input = document.createElement('input')
  input.type = 'search'
  input.name = 'q'
  input.placeholder = 'Search pages'
  input.autocomplete = 'off'
  input.spellcheck = false
  input.setAttribute('role', 'combobox')
  input.setAttribute('aria-expanded', 'true')
  input.setAttribute('aria-controls', 'riki-search-results')
  input.setAttribute('aria-label', 'Search pages')
  const status = document.createElement('p')
  status.className = 'riki-dialog-note riki-search-status'
  status.setAttribute('role', 'status')
  status.hidden = true
  const list = document.createElement('ul')
  list.id = 'riki-search-results'
  list.className = 'riki-search-results'
  list.setAttribute('role', 'listbox')
  box.append(input, status, list)
  backdrop.append(box)

  let selected = -1
  let timer: ReturnType<typeof setTimeout> | undefined
  let latest = 0

  const items = (): HTMLElement[] => [...list.querySelectorAll<HTMLElement>('.riki-search-hit')]
  const select = (index: number): void => {
    const all = items()
    if (all.length === 0) return
    selected = (index + all.length) % all.length
    all.forEach((item, i) => item.setAttribute('aria-selected', String(i === selected)))
    input.setAttribute('aria-activedescendant', all[selected]?.id ?? '')
    all[selected]?.scrollIntoView?.({ block: 'nearest' })
  }
  const show = (message: string | null): void => {
    status.hidden = message === null
    status.textContent = message ?? ''
  }
  const search = async (): Promise<void> => {
    const query = input.value.trim()
    const ticket = ++latest
    if (query === '') {
      list.replaceChildren()
      selected = -1
      show(null)
      return
    }
    const reply = await fetchSearch(query, fetchImpl)
    if (ticket !== latest) return
    list.replaceChildren()
    selected = -1
    if (!reply.ok) {
      show(reply.message)
      return
    }
    show(reply.value.hits.length === 0 ? 'No pages match.' : null)
    reply.value.hits.forEach((hit, i) => list.append(renderHit(hit, i)))
    select(0)
  }

  const close = (): void => {
    clearTimeout(timer)
    latest++
    document.removeEventListener('keydown', onKey, true)
    backdrop.remove()
    current = null
  }
  const onKey = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      close()
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      select(selected + (event.key === 'ArrowDown' ? 1 : -1))
    } else if (event.key === 'Enter') {
      event.preventDefault()
      const url = items()[selected]?.dataset['url']
      if (url) {
        close()
        navigate(url)
      }
    }
  }
  input.addEventListener('input', () => {
    clearTimeout(timer)
    timer = setTimeout(() => void search(), DEBOUNCE_MS)
  })
  list.addEventListener('click', (event) => {
    const url = (event.target instanceof Element ? event.target.closest<HTMLElement>('.riki-search-hit') : null)?.dataset['url']
    if (url) {
      close()
      navigate(url)
    }
  })
  backdrop.addEventListener('mousedown', (event) => {
    if (event.target === backdrop) close()
  })
  document.addEventListener('keydown', onKey, true)
  document.body.append(backdrop)
  input.focus()
  current = { close }
}

/** True for the palette's shortcut, Ctrl+K or Cmd+K, when the key was not typed in the editor
 *  (which owns Mod-k for its link box, and so does the box once open). */
export function isPaletteShortcut(event: KeyboardEvent): boolean {
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.key.toLowerCase() !== 'k') return false
  const target = event.target instanceof Element ? event.target : null
  return !target?.closest('.riki-editor-root, .ProseMirror, .riki-linkbox')
}
