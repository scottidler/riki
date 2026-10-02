// The page script's calls to riki's wiki-operation routes, and the pure helpers around them.
// Every call returns data, never throws, and touches no DOM; `actions.ts` owns the dialogs.

type Fetch = typeof fetch

export const TREE_API = '/_riki/api/tree'
export const NEW_PAGE_API = '/_riki/api/new-page'
export const MOVE_API = '/_riki/api/move'
export const DELETE_API = '/_riki/api/delete'
export const RESTORE_API = '/_riki/api/restore'

export interface Tree {
  folders: string[]
  pages: { path: string; url: string; title: string }[]
}

/** A route's answer: its JSON on a 200, else the words to show the person. */
export type Reply<T> = { ok: true; value: T } | { ok: false; message: string }

async function errorMessage(response: Response): Promise<string> {
  const json = (await response.json().catch(() => null)) as { error?: unknown } | null
  return typeof json?.error === 'string' ? json.error : `HTTP ${response.status}`
}

async function call<T>(fetchImpl: Fetch, url: string, init: RequestInit | undefined, what: string): Promise<Reply<T>> {
  let response: Response
  try {
    response = await fetchImpl(url, init)
  } catch (err) {
    return { ok: false, message: `${what} did not reach the server: ${String(err)}` }
  }
  if (!response.ok) return { ok: false, message: await errorMessage(response) }
  const value = (await response.json().catch(() => null)) as T | null
  if (value === null || typeof value !== 'object') return { ok: false, message: `${what} answered with a malformed response` }
  return { ok: true, value }
}

function post(body: unknown): RequestInit {
  return { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }
}

export function fetchTree(fetchImpl: Fetch = fetch): Promise<Reply<Tree>> {
  return call<Tree>(fetchImpl, TREE_API, { headers: { Accept: 'application/json' } }, 'loading the folders')
}

/** The path and URL the slug rule gives `title` in `folder`. */
export function fetchNewPage(folder: string, title: string, fetchImpl: Fetch = fetch): Promise<Reply<{ path: string; url: string }>> {
  const query = `folder=${encodeURIComponent(folder)}&title=${encodeURIComponent(title)}`
  return call(fetchImpl, `${NEW_PAGE_API}?${query}`, { headers: { Accept: 'application/json' } }, 'choosing the path')
}

export function movePage(from: string, baseOid: string, to: string, fetchImpl: Fetch = fetch): Promise<Reply<{ commit: string | null; url: string }>> {
  return call(fetchImpl, MOVE_API, post({ from, 'base-oid': baseOid, to }), 'the move')
}

export function deletePage(path: string, baseOid: string, fetchImpl: Fetch = fetch): Promise<Reply<{ commit: string | null }>> {
  return call(fetchImpl, DELETE_API, post({ path, 'base-oid': baseOid }), 'the delete')
}

export function restorePage(path: string, commit: string, fetchImpl: Fetch = fetch): Promise<Reply<{ commit: string | null; url: string }>> {
  return call(fetchImpl, RESTORE_API, post({ path, commit }), 'the restore')
}

/** `a/b/c.md` -> `a/b`; a top-level file -> `""`. */
export function folderOf(path: string): string {
  const slash = path.lastIndexOf('/')
  return slash < 0 ? '' : path.slice(0, slash)
}

/** `a/b/c.md` -> `c`. */
export function stemOf(path: string): string {
  const name = path.slice(path.lastIndexOf('/') + 1)
  return name.endsWith('.md') ? name.slice(0, -3) : name
}

/** A typed folder as a repo directory: slashes trimmed at both ends, `""` for the top level. */
export function normalizeFolder(typed: string): string {
  return typed.trim().replace(/^\/+|\/+$/g, '')
}

/** The folders matching what is being typed: case-insensitive substring, in tree order. */
export function filterFolders(folders: readonly string[], query: string): string[] {
  const needle = normalizeFolder(query).toLowerCase()
  return folders.filter((folder) => folder.toLowerCase().includes(needle))
}

/** The file a move targets, or the reason there is none. The name gets `.md` when it has none. */
export function movedPath(folder: string, fileName: string): { ok: true; path: string } | { ok: false; message: string } {
  const name = fileName.trim()
  if (name === '') return { ok: false, message: 'Give the page a file name.' }
  if (name.includes('/')) return { ok: false, message: 'The file name cannot contain a slash; put folders in the folder field.' }
  const file = name.endsWith('.md') ? name : `${name}.md`
  const dir = normalizeFolder(folder)
  return { ok: true, path: dir === '' ? file : `${dir}/${file}` }
}

/** The URL of the editor for a new page: the slug rule's URL with the typed title. */
export function newPageUrl(url: string, title: string): string {
  return `${url}?new=${encodeURIComponent(title)}`
}
