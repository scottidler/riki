// The page actions: "+" new page, and the ⋯ menu's Move and Delete (with Undo). Server-rendered
// controls carry the data (`data-riki-new`, `data-riki-action`, the article's `data-path` and
// `data-base-oid`); this module opens the dialogs and calls the routes. Nothing is written
// until a dialog is confirmed, and a new page is not written until the editor's Save.

import { field, openDialog } from './dialog'
import {
  deletePage,
  fetchNewPage,
  fetchTree,
  filterFolders,
  folderOf,
  movePage,
  movedPath,
  newPageUrl,
  normalizeFolder,
  restorePage,
  stemOf,
} from './ops'

export type Navigate = (url: string) => void

const MENU_OPEN = 'data-open'

function article(): HTMLElement | null {
  return document.querySelector<HTMLElement>('main article[data-path]')
}

/** "+" -> "Page title" dialog -> the new page's URL with `?new=`; the editor opens there. */
export function newPageDialog(folder: string, navigate: Navigate): void {
  const { row, input } = field('Page title', 'title')
  const body = document.createElement('div')
  body.append(row)
  openDialog({
    title: 'New page',
    body,
    confirmLabel: 'Create',
    onConfirm: async () => {
      const title = input.value.trim()
      if (title === '') return 'Give the page a title.'
      const reply = await fetchNewPage(folder, title)
      if (!reply.ok) return reply.message
      navigate(newPageUrl(reply.value.url, title))
      return null
    },
  })
}

/** The folder picker: a text field that filters `folders` as you type. A name that is not a
 *  folder yet is offered as "Create folder"; choosing it only means the new path contains it. */
function folderPicker(folders: readonly string[], initial: string): { element: HTMLElement; value: () => string } {
  const { row, input } = field('Folder', 'folder', initial)
  const list = document.createElement('ul')
  list.className = 'riki-folder-list'
  list.setAttribute('role', 'listbox')
  const render = (): void => {
    list.replaceChildren()
    const typed = normalizeFolder(input.value)
    const add = (label: string, value: string, create = false): void => {
      const item = document.createElement('li')
      const choose = document.createElement('button')
      choose.type = 'button'
      choose.setAttribute('role', 'option')
      choose.dataset['folder'] = value
      if (create) choose.dataset['create'] = ''
      choose.textContent = label
      choose.addEventListener('click', () => {
        input.value = value
        render()
        input.focus()
      })
      item.append(choose)
      list.append(item)
    }
    for (const folder of filterFolders(folders, typed)) add(folder === '' ? '/ (top level)' : folder, folder)
    if (typed !== '' && !folders.includes(typed)) add(`Create folder ${typed}`, typed, true)
  }
  input.addEventListener('input', render)
  render()
  const element = document.createElement('div')
  element.append(row, list)
  return { element, value: () => normalizeFolder(input.value) }
}

/** ⋯ -> Move… -> folder picker + file name -> `POST move` -> the page at its new URL. */
export async function moveDialog(navigate: Navigate): Promise<void> {
  const page = article()
  const from = page?.dataset['path']
  const baseOid = page?.dataset['baseOid']
  if (!from || !baseOid) return
  const tree = await fetchTree()
  const folders = tree.ok ? tree.value.folders : []
  const picker = folderPicker(folders, folderOf(from))
  const name = field('File name', 'name', stemOf(from))
  const body = document.createElement('div')
  body.append(picker.element, name.row)
  const note = document.createElement('p')
  note.className = 'riki-dialog-note'
  note.textContent = tree.ok
    ? 'The old URL redirects to the new one. Links inside the page are not rewritten.'
    : `Could not list the folders (${tree.message}); type one.`
  body.append(note)
  openDialog({
    title: `Move ${from}`,
    body,
    confirmLabel: 'Move',
    onConfirm: async () => {
      const target = movedPath(picker.value(), name.input.value)
      if (!target.ok) return target.message
      if (target.path === from) return 'The page is already there.'
      const reply = await movePage(from, baseOid, target.path)
      if (!reply.ok) return reply.message
      navigate(reply.value.url)
      return null
    },
  })
}

/** ⋯ -> Delete -> confirm -> `POST delete` -> the article becomes "Deleted <path>" with Undo. */
export function deleteDialog(navigate: Navigate): void {
  const page = article()
  const path = page?.dataset['path']
  const baseOid = page?.dataset['baseOid']
  if (!page || !path || !baseOid) return
  const body = document.createElement('p')
  body.textContent = `Delete ${path}? It stays in git history, and you can undo right after.`
  openDialog({
    title: 'Delete page',
    body,
    confirmLabel: 'Delete',
    danger: true,
    onConfirm: async () => {
      const reply = await deletePage(path, baseOid)
      if (!reply.ok) return reply.message
      await showDeleted(page, path, reply.value.commit, navigate)
      return null
    },
  })
}

/** Replace the article in place; drop the page's actions; re-render the sidebar without the
 *  page. Undo exists only when the delete made a commit (`commit: null` is a retried delete). */
async function showDeleted(page: HTMLElement, path: string, commit: string | null, navigate: Navigate): Promise<void> {
  const notice = document.createElement('div')
  notice.className = 'riki-deleted'
  notice.setAttribute('role', 'status')
  const text = document.createElement('p')
  text.append('Deleted ')
  const code = document.createElement('code')
  code.textContent = path
  text.append(code, '.')
  notice.append(text)
  if (commit !== null) {
    const undo = document.createElement('button')
    undo.type = 'button'
    undo.className = 'riki-button'
    undo.dataset['control'] = 'undo'
    undo.textContent = 'Undo'
    undo.addEventListener('click', () => {
      undo.disabled = true
      void restorePage(path, commit).then((reply) => {
        if (reply.ok) {
          navigate(reply.value.url)
          return
        }
        undo.disabled = false
        text.textContent = `Could not undo: ${reply.message}`
      })
    })
    notice.append(undo)
  }
  page.replaceChildren(notice)
  page.removeAttribute('data-path')
  page.removeAttribute('data-base-oid')
  document.querySelector('header .actions')?.replaceChildren()
  document.querySelector('.riki-pager')?.remove()
  await refreshSidebar()
}

async function refreshSidebar(): Promise<void> {
  try {
    const response = await fetch(window.location.pathname, { headers: { Accept: 'text/html' } })
    const fresh = new DOMParser().parseFromString(await response.text(), 'text/html')
    const next = fresh.querySelector('.riki-sidebar')
    if (next) document.querySelector('.riki-sidebar')?.replaceWith(document.adoptNode(next))
  } catch {
    // The page is deleted either way; a stale sidebar fixes itself on the next load.
  }
}

/** Show or hide the ⋯ menu next to Edit. */
export function setMenuOpen(open: boolean): void {
  const menu = document.querySelector<HTMLElement>('.riki-menu')
  const toggle = document.getElementById('riki-more')
  if (!menu || !toggle) return
  menu.hidden = !open
  menu.toggleAttribute(MENU_OPEN, open)
  toggle.setAttribute('aria-expanded', String(open))
}

export function menuIsOpen(): boolean {
  return document.querySelector('.riki-menu')?.hasAttribute(MENU_OPEN) ?? false
}
