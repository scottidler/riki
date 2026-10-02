// Page script entry, /_riki/assets/riki.js. Loaded without `defer` in <head> so a pinned theme
// applies before first paint; everything that needs the DOM waits for DOMContentLoaded. Clicks
// are delegated from the document, so the editor's re-render (which swaps the sidebar and main)
// needs only the `riki:rendered` event to re-decorate.

import {
  applyTheme,
  activeTocId,
  copyFrom,
  decorateCodeBlocks,
  localStore,
  markToc,
  setNavOpen,
  storedTheme,
  toggleTheme,
  tocHeadings,
} from './ui'

const root = document.documentElement
applyTheme(root, storedTheme(localStore()))

const prefersDark = () => window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false

function headerOffset(): number {
  const header = document.querySelector('.riki-header')
  return (header?.getBoundingClientRect().height ?? 0) + 24
}

let spyQueued = false
function spy(): void {
  if (spyQueued) return
  spyQueued = true
  requestAnimationFrame(() => {
    spyQueued = false
    markToc(document, activeTocId(tocHeadings(document), headerOffset()))
  })
}

function decorate(): void {
  decorateCodeBlocks(document)
  spy()
}

document.addEventListener('click', (event) => {
  const target = event.target instanceof Element ? event.target : null
  const toggle = target?.closest<HTMLElement>('[data-riki-toggle]')
  if (toggle?.dataset['rikiToggle'] === 'theme') {
    toggleTheme(root, localStore(), prefersDark())
    return
  }
  if (toggle?.dataset['rikiToggle'] === 'nav') {
    setNavOpen(document, !document.body.classList.contains('riki-nav-open'))
    return
  }
  const copy = target?.closest<HTMLElement>('.riki-copy')
  if (copy) {
    void copyFrom(copy, navigator.clipboard)
    return
  }
  if (target?.closest('.riki-sidebar a')) setNavOpen(document, false)
})

document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && document.body.classList.contains('riki-nav-open')) setNavOpen(document, false)
})

window.addEventListener('scroll', spy, { passive: true })
document.addEventListener('riki:rendered', decorate)
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', decorate)
else decorate()
