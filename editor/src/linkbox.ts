// The editor's Ctrl+K / Cmd+K link box. It lists the wiki's pages (GET /_riki/api/tree, filtered
// as you type, plus GET /_riki/api/search for body matches) and inserts a link whose href is the
// target's path relative to the file being edited, because riki resolves links against the source
// file. Text that parses as an absolute URL is inserted as it is. Everything shown is set through
// textContent; nothing here touches innerHTML.

import { linkSchema } from '@milkdown/kit/preset/commonmark'
import type { MarkType } from '@milkdown/kit/prose/model'
import { Plugin, PluginKey } from '@milkdown/kit/prose/state'
import type { EditorView } from '@milkdown/kit/prose/view'
import { $prose } from '@milkdown/kit/utils'
import { fetchTree } from './page/ops'
import type { Tree } from './page/ops'
import { DEBOUNCE_MS, fetchSearch } from './page/search'

type Fetch = typeof fetch

/** Most rows the box shows for a query. */
const MAX_ROWS = 20

/** One path segment percent-encoded the way the server's `encode_path` does (and so the
 *  sidebar's hrefs): every byte but unreserved characters, so `#` and `?` stay in the path. */
function encodeSegment(segment: string): string {
  return encodeURIComponent(segment).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`)
}

/** `target` (a repo path) as a link written in `sourceFile`: `../c/d.md` from `a/b.md` to `c/d.md`.
 *  Each segment is percent-encoded, so `docs/a#b.md` links as `docs/a%23b.md`. */
export function relativeHref(sourceFile: string, target: string): string {
  const from = sourceFile.split('/').slice(0, -1)
  const to = target.split('/')
  let shared = 0
  while (shared < from.length && shared < to.length - 1 && from[shared] === to[shared]) shared++
  return [...from.slice(shared).map(() => '..'), ...to.slice(shared).map(encodeSegment)].join('/')
}

/** True for input that is a URL with a scheme (`https://x`, `mailto:a@b`): inserted as typed. */
export function isAbsoluteUrl(input: string): boolean {
  if (!/^[a-z][a-z0-9+.-]*:\S+$/i.test(input)) return false
  return URL.canParse(input)
}

/** One row of the box: what Enter would insert. */
export interface LinkChoice {
  href: string
  /** The link text when nothing is selected. */
  text: string
  title: string
  detail: string
}

/** Rows for pages of `tree` whose title or path contains every word of `query`, in tree order. */
export function matchPages(tree: Tree, query: string, sourceFile: string): LinkChoice[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  return tree.pages
    .filter((page) => {
      const haystack = `${page.title} ${page.path}`.toLowerCase()
      return words.every((word) => haystack.includes(word))
    })
    .map((page) => ({ href: relativeHref(sourceFile, page.path), text: page.title, title: page.title, detail: page.path }))
}

/** The row for a typed absolute URL, or null when `query` is not one. */
export function urlChoice(query: string): LinkChoice | null {
  const url = query.trim()
  return isAbsoluteUrl(url) ? { href: url, text: url, title: url, detail: 'External link' } : null
}

/** Link the editor's selection (or insert `choice.text` as the link) at the positions kept when
 *  the box opened. */
export function applyLink(view: EditorView, link: MarkType, range: { from: number; to: number }, choice: LinkChoice): void {
  const mark = link.create({ href: choice.href })
  const tr = view.state.tr
  if (range.from === range.to) {
    tr.insert(range.from, view.state.schema.text(choice.text, [mark]))
  } else {
    tr.addMark(range.from, range.to, mark)
  }
  view.dispatch(tr.scrollIntoView())
}

let open: { close(): void } | null = null

export interface LinkBoxEnv {
  view: EditorView
  link: MarkType
  sourceFile: string
  fetchImpl?: Fetch
}

/** Open the link box over the editor. A second call while open only refocuses it. */
export function openLinkBox({ view, link, sourceFile, fetchImpl = fetch }: LinkBoxEnv): void {
  if (open) {
    document.querySelector<HTMLInputElement>('.riki-linkbox input')?.focus()
    return
  }
  const { from, to } = view.state.selection
  const range = { from, to }

  const backdrop = document.createElement('div')
  backdrop.className = 'riki-dialog-backdrop riki-search-backdrop'
  const box = document.createElement('div')
  box.className = 'riki-dialog riki-search riki-linkbox'
  box.setAttribute('role', 'dialog')
  box.setAttribute('aria-modal', 'true')
  box.setAttribute('aria-label', 'Link to page')
  const input = document.createElement('input')
  input.type = 'search'
  input.name = 'link'
  input.placeholder = 'Link to a page, or paste a URL'
  input.autocomplete = 'off'
  input.spellcheck = false
  input.setAttribute('aria-label', 'Link to a page or URL')
  const status = document.createElement('p')
  status.className = 'riki-dialog-note riki-search-status'
  status.setAttribute('role', 'status')
  status.hidden = true
  const list = document.createElement('ul')
  list.className = 'riki-search-results'
  list.setAttribute('role', 'listbox')
  box.append(input, status, list)
  backdrop.append(box)

  let tree: Tree | null = null
  let treeProblem: string | null = null
  let choices: LinkChoice[] = []
  let selected = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  let latest = 0

  const paint = (note: string | null): void => {
    list.replaceChildren()
    choices.forEach((choice, i) => {
      const item = document.createElement('li')
      item.className = 'riki-search-hit'
      item.setAttribute('role', 'option')
      item.setAttribute('aria-selected', String(i === selected))
      item.dataset['index'] = String(i)
      const head = document.createElement('span')
      head.className = 'riki-search-head'
      const title = document.createElement('span')
      title.className = 'riki-search-title'
      title.textContent = choice.title
      const detail = document.createElement('span')
      detail.className = 'riki-search-path'
      detail.textContent = choice.detail
      head.append(title, detail)
      item.append(head)
      list.append(item)
    })
    const message = note ?? treeProblem
    status.hidden = message === null
    status.textContent = message ?? ''
    list.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: 'nearest' })
  }

  const refresh = async (): Promise<void> => {
    const query = input.value.trim()
    const ticket = ++latest
    const typedUrl = urlChoice(query)
    const local = tree ? matchPages(tree, query, sourceFile) : []
    choices = [...(typedUrl ? [typedUrl] : []), ...local].slice(0, MAX_ROWS)
    selected = 0
    paint(null)
    if (query === '' || typedUrl) return
    const reply = await fetchSearch(query, fetchImpl)
    if (ticket !== latest || !reply.ok) return
    const seen = new Set(choices.map((choice) => choice.detail))
    for (const hit of reply.value.hits) {
      if (seen.has(hit.path) || choices.length >= MAX_ROWS) continue
      seen.add(hit.path)
      choices.push({ href: relativeHref(sourceFile, hit.path), text: hit.title, title: hit.title, detail: hit.path })
    }
    paint(choices.length === 0 ? 'No pages match. Paste a full URL to link outside the wiki.' : null)
  }

  const close = (): void => {
    clearTimeout(timer)
    latest++
    document.removeEventListener('keydown', onKey, true)
    backdrop.remove()
    open = null
    view.focus()
  }
  const choose = (choice: LinkChoice | undefined): void => {
    if (!choice) return
    close()
    applyLink(view, link, range, choice)
  }
  const onKey = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      close()
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault()
      event.stopPropagation()
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      if (choices.length > 0) selected = (selected + (event.key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length
      paint(null)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      choose(choices[selected])
    }
  }
  input.addEventListener('input', () => {
    clearTimeout(timer)
    timer = setTimeout(() => void refresh(), DEBOUNCE_MS)
  })
  list.addEventListener('click', (event) => {
    const row = event.target instanceof Element ? event.target.closest<HTMLElement>('.riki-search-hit') : null
    choose(choices[Number(row?.dataset['index'])])
  })
  backdrop.addEventListener('mousedown', (event) => {
    if (event.target === backdrop) close()
  })
  document.addEventListener('keydown', onKey, true)
  document.body.append(backdrop)
  input.focus()
  open = { close }

  void fetchTree(fetchImpl).then((reply) => {
    if (open?.close !== close) return
    if (reply.ok) tree = reply.value
    else treeProblem = `Pages did not load: ${reply.message}`
    void refresh()
  })
}

export function linkBoxIsOpen(): boolean {
  return open !== null
}

/** Mod-k opens the link box. Not offered where links cannot go (a code block). */
export const linkBoxPlugin = (sourceFile: string) =>
  $prose((ctx) => {
    const link = linkSchema.type(ctx)
    return new Plugin({
      key: new PluginKey('rikiLinkBox'),
      props: {
        handleKeyDown(view, event) {
          if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.key.toLowerCase() !== 'k') return false
          if (!view.editable) return false
          if (!view.state.selection.$from.parent.type.allowsMarkType(link)) return false
          openLinkBox({ view, link, sourceFile })
          return true
        },
      },
    })
  })
