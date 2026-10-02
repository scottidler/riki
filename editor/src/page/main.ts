// Page script entry, /_riki/assets/riki.js. Loaded without `defer` in <head> so the stored theme
// applies before first paint; everything that needs the DOM waits for DOMContentLoaded. Clicks
// are delegated from the document, so the editor's re-render (which swaps the sidebar and main)
// needs only the `riki:rendered` event to re-decorate.

import {
  activeTocId,
  applyPreference,
  choosePreference,
  copyFrom,
  currentPreference,
  decorateCodeBlocks,
  localStore,
  markOverflow,
  markToc,
  setNavOpen,
  storedPreference,
  switchChoice,
  toggleGroup,
  tocHeadings,
} from './ui'
import { isPaletteShortcut, openPalette } from './search'
import { deleteDialog, menuIsOpen, moveDialog, newPageDialog, setMenuOpen } from './actions'

const navigate = (url: string): void => window.location.assign(url)

const root = document.documentElement
const darkQuery = window.matchMedia?.('(prefers-color-scheme: dark)')
const prefersDark = () => darkQuery?.matches ?? false
applyPreference(root, storedPreference(localStore()), prefersDark())
darkQuery?.addEventListener('change', () => applyPreference(root, currentPreference(root), prefersDark()))

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

const resized = typeof ResizeObserver === 'function'
  ? new ResizeObserver((entries) => {
      for (const entry of entries) {
        const wrap = entry.target.closest<HTMLElement>('.riki-code')
        if (wrap) markOverflow(wrap)
      }
    })
  : null

function decorate(): void {
  applyPreference(root, currentPreference(root), prefersDark())
  decorateCodeBlocks(document)
  for (const pre of document.querySelectorAll('article.riki-prose .riki-code > pre')) resized?.observe(pre)
  spy()
}

document.addEventListener('click', (event) => {
  const target = event.target instanceof Element ? event.target : null
  const choice = switchChoice(target)
  if (choice) {
    choosePreference(root, localStore(), choice, prefersDark())
    return
  }
  const toggle = target?.closest<HTMLElement>('[data-riki-toggle]')
  if (toggle?.dataset['rikiToggle'] === 'nav') {
    setNavOpen(document, !document.body.classList.contains('riki-nav-open'))
    return
  }
  if (toggle?.dataset['rikiToggle'] === 'group') {
    toggleGroup(toggle)
    return
  }
  if (target?.closest('[data-riki-search]')) {
    openPalette(navigate)
    return
  }
  const add = target?.closest<HTMLElement>('[data-riki-new]')
  if (add) {
    newPageDialog(add.dataset['rikiNew'] ?? '', navigate)
    return
  }
  if (target?.closest('#riki-more')) {
    setMenuOpen(!menuIsOpen())
    return
  }
  const action = target?.closest<HTMLElement>('[data-riki-action]')
  if (action) {
    setMenuOpen(false)
    if (action.dataset['rikiAction'] === 'move') void moveDialog(navigate)
    else if (action.dataset['rikiAction'] === 'delete') deleteDialog(navigate)
    return
  }
  if (menuIsOpen()) setMenuOpen(false)
  const copy = target?.closest<HTMLElement>('.riki-copy')
  if (copy) {
    void copyFrom(copy, navigator.clipboard)
    return
  }
  if (target?.closest('.riki-sidebar a')) setNavOpen(document, false)
})

document.addEventListener('keydown', (event) => {
  if (isPaletteShortcut(event)) {
    event.preventDefault()
    openPalette(navigate)
    return
  }
  if (event.key !== 'Escape') return
  if (menuIsOpen()) setMenuOpen(false)
  else if (document.body.classList.contains('riki-nav-open')) setNavOpen(document, false)
})

window.addEventListener('scroll', spy, { passive: true })
if (!resized) {
  window.addEventListener('resize', () => {
    for (const wrap of document.querySelectorAll<HTMLElement>('article.riki-prose .riki-code')) markOverflow(wrap)
  })
}
document.addEventListener('riki:rendered', decorate)
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', decorate)
else decorate()
