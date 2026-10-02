// Browser entry: wires the page's Edit button and "Create this page" link to an editing
// session. Served from /_riki/assets/editor.js under `script-src 'self'`.

import { Session } from './session'
import type { PageTarget } from './session'

/** The pieces of a rendered riki page the editor replaces after a save. */
const SWAPPED = ['header .actions', '.riki-trail', '.riki-sidebar', 'main']

/** On `<body>` while a session is open (the stylesheet hides the "On this page" list). */
const EDITING_CLASS = 'riki-editing'

/** Fired on `document` after a re-render, so the page script re-decorates the new content. */
export const RENDERED_EVENT = 'riki:rendered'

let active: Session | null = null

function target(el: HTMLElement): PageTarget | null {
  const path = el.dataset['path']
  if (!path) return null
  return { path, source: el.dataset['source'] ?? null }
}

/** Fetch the page as riki now serves it (the good tip, which every 200 save published) and
 *  swap it in. The URL does not change. */
async function rerender(): Promise<void> {
  const response = await fetch(window.location.pathname, { headers: { Accept: 'text/html' } })
  if (!response.ok) throw new Error(`GET ${window.location.pathname}: HTTP ${response.status}`)
  const html = await response.text()
  const fresh = new DOMParser().parseFromString(html, 'text/html')
  await active?.close()
  for (const selector of SWAPPED) {
    const current = document.querySelector(selector)
    const next = fresh.querySelector(selector)
    if (current && next) current.replaceWith(document.adoptNode(next))
  }
  for (const banner of document.querySelectorAll('.banner')) banner.remove()
  const header = document.querySelector('header')
  for (const banner of [...fresh.querySelectorAll('.banner')].reverse()) header?.after(document.adoptNode(banner))
  document.title = fresh.title
  bind()
  document.dispatchEvent(new CustomEvent(RENDERED_EVENT))
}

async function start(el: HTMLElement): Promise<void> {
  const page = target(el)
  const article = document.querySelector<HTMLElement>('main article')
  if (!page || !article || active) return
  document.body.classList.add(EDITING_CLASS)
  active = new Session(page, article, {
    rerender,
    closed: () => {
      active = null
      document.body.classList.remove(EDITING_CLASS)
    },
  })
  await active.open()
}

export function bind(): void {
  for (const id of ['riki-edit', 'riki-create']) {
    const el = document.getElementById(id)
    if (!el) continue
    el.addEventListener('click', (event) => {
      event.preventDefault()
      void start(el)
    })
  }
}

if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', bind)
else bind()
