import { afterEach, describe, expect, it, vi } from 'vitest'
import { newPageBody, newPageBodyFromSearch } from '../src/newpage'
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
} from '../src/page/ops'
import { deleteDialog, moveDialog, newPageDialog } from '../src/page/actions'

const json = (status: number, body: unknown): Response =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

describe('new page body', () => {
  it('is the title as an H1 and a blank line', () => {
    expect(newPageBody('Foo')).toBe('# Foo\n\n')
  })
  it('collapses whitespace and escapes markdown punctuation', () => {
    expect(newPageBody('  A *b*\n c  ')).toBe('# A \\*b\\* c\n\n')
  })
  it('reads ?new= and ignores a missing or blank title', () => {
    expect(newPageBodyFromSearch('?new=Foo%20Bar')).toBe('# Foo Bar\n\n')
    expect(newPageBodyFromSearch('')).toBeNull()
    expect(newPageBodyFromSearch('?new=%20')).toBeNull()
  })
})

describe('path helpers', () => {
  it('splits a repo path', () => {
    expect(folderOf('a/b/c.md')).toBe('a/b')
    expect(folderOf('c.md')).toBe('')
    expect(stemOf('a/b/c.md')).toBe('c')
  })
  it('normalizes a typed folder', () => {
    expect(normalizeFolder(' /guide/ ')).toBe('guide')
    expect(normalizeFolder('/')).toBe('')
  })
  it('filters folders as you type, keeping the root for an empty query', () => {
    const folders = ['', 'guide', 'guide/api', 'notes']
    expect(filterFolders(folders, '')).toEqual(folders)
    expect(filterFolders(folders, 'API')).toEqual(['guide/api'])
    expect(filterFolders(folders, 'zzz')).toEqual([])
  })
  it('builds the moved path, adding .md and refusing slashes and empty names', () => {
    expect(movedPath('guide', 'intro')).toEqual({ ok: true, path: 'guide/intro.md' })
    expect(movedPath('', 'intro.md')).toEqual({ ok: true, path: 'intro.md' })
    expect(movedPath('/a/b/', 'x')).toEqual({ ok: true, path: 'a/b/x.md' })
    expect(movedPath('a', 'b/c')).toMatchObject({ ok: false })
    expect(movedPath('a', '  ')).toMatchObject({ ok: false })
  })
  it('adds the title to the new-page URL', () => {
    expect(newPageUrl('/guide/foo', 'Foo & Bar')).toBe('/guide/foo?new=Foo%20%26%20Bar')
  })
})

describe('routes', () => {
  it('fetchTree returns the folders', async () => {
    const f = vi.fn().mockResolvedValue(json(200, { folders: [''], pages: [] }))
    expect(await fetchTree(f)).toEqual({ ok: true, value: { folders: [''], pages: [] } })
  })
  it('fetchNewPage encodes folder and title', async () => {
    const f = vi.fn().mockResolvedValue(json(200, { path: 'a/x.md', url: '/a/x' }))
    await fetchNewPage('a b', 'X & Y', f)
    expect(f.mock.calls[0]?.[0]).toBe('/_riki/api/new-page?folder=a%20b&title=X%20%26%20Y')
  })
  it('an error status carries the server words, a network failure says so, junk is malformed', async () => {
    expect(await movePage('a.md', 'o', 'b.md', async () => json(409, { error: 'b.md exists' }))).toEqual({
      ok: false,
      message: 'b.md exists',
    })
    expect(await deletePage('a.md', 'o', async () => new Response('boom', { status: 500 }))).toEqual({
      ok: false,
      message: 'HTTP 500',
    })
    const down = await restorePage('a.md', 'c', async () => Promise.reject(new Error('offline')))
    expect(down).toMatchObject({ ok: false })
    expect(!down.ok && down.message).toContain('offline')
    const junk = await fetchTree(async () => new Response('null', { status: 200 }))
    expect(junk).toMatchObject({ ok: false })
  })
  it('posts the kebab-case bodies', async () => {
    const f = vi.fn().mockResolvedValue(json(200, { commit: 'c', url: '/b' }))
    await movePage('a.md', 'oid', 'b.md', f)
    expect(JSON.parse(f.mock.calls[0]?.[1].body)).toEqual({ from: 'a.md', 'base-oid': 'oid', to: 'b.md' })
    await deletePage('a.md', 'oid', f)
    expect(JSON.parse(f.mock.calls[1]?.[1].body)).toEqual({ path: 'a.md', 'base-oid': 'oid' })
    await restorePage('a.md', 'dd', f)
    expect(JSON.parse(f.mock.calls[2]?.[1].body)).toEqual({ path: 'a.md', commit: 'dd' })
  })
})

describe('dialogs', () => {
  afterEach(() => {
    document.body.innerHTML = ''
    vi.unstubAllGlobals()
  })
  const submit = (): void => {
    document.querySelector<HTMLFormElement>('.riki-dialog')?.dispatchEvent(new Event('submit', { cancelable: true }))
  }
  const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

  it('the new-page dialog asks the slug route, then navigates with ?new=', async () => {
    const fetchMock = vi.fn().mockResolvedValue(json(200, { path: 'g/foo.md', url: '/g/foo' }))
    vi.stubGlobal('fetch', fetchMock)
    const navigate = vi.fn()
    newPageDialog('g', navigate)
    const input = document.querySelector<HTMLInputElement>('input[name=title]')!
    input.value = 'Foo'
    submit()
    await settle()
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/_riki/api/new-page?folder=g&title=Foo')
    expect(navigate).toHaveBeenCalledWith('/g/foo?new=Foo')
    expect(document.querySelector('.riki-dialog')).toBeNull()
  })

  it('an empty title keeps the dialog open with the reason and calls nothing', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    newPageDialog('', vi.fn())
    submit()
    await settle()
    expect(fetchMock).not.toHaveBeenCalled()
    expect(document.querySelector('.riki-dialog-error')?.textContent).toBe('Give the page a title.')
  })

  it('Escape closes the dialog', () => {
    newPageDialog('', vi.fn())
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(document.querySelector('.riki-dialog')).toBeNull()
  })

  it('the move dialog lists folders, offers Create folder, and posts the moved path', async () => {
    document.body.innerHTML = '<main><article data-path="a/x.md" data-base-oid="oid1">x</article></main>'
    const fetchMock = vi.fn(async (url: string, _init?: RequestInit) =>
      url === '/_riki/api/tree' ? json(200, { folders: ['', 'a', 'b'], pages: [] }) : json(200, { commit: 'c', url: '/new/y' }),
    )
    vi.stubGlobal('fetch', fetchMock)
    const navigate = vi.fn()
    await moveDialog(navigate)
    const folder = document.querySelector<HTMLInputElement>('input[name=folder]')!
    expect(folder.value).toBe('a')
    folder.value = 'new'
    folder.dispatchEvent(new Event('input'))
    expect(document.querySelector('[data-create]')?.textContent).toBe('Create folder new')
    document.querySelector<HTMLInputElement>('input[name=name]')!.value = 'y'
    submit()
    await settle()
    const call = fetchMock.mock.calls.find(([url]) => url === '/_riki/api/move')!
    expect(JSON.parse(call[1]?.body as string)).toEqual({ from: 'a/x.md', 'base-oid': 'oid1', to: 'new/y.md' })
    expect(navigate).toHaveBeenCalledWith('/new/y')
  })

  it('moving a page onto its own path says so and calls no route', async () => {
    document.body.innerHTML = '<main><article data-path="a/x.md" data-base-oid="oid1">x</article></main>'
    const fetchMock = vi.fn(async () => json(200, { folders: ['a'], pages: [] }))
    vi.stubGlobal('fetch', fetchMock)
    await moveDialog(vi.fn())
    submit()
    await settle()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(document.querySelector('.riki-dialog-error')?.textContent).toBe('The page is already there.')
  })

  it('delete replaces the article with Deleted and Undo, and Undo restores then navigates', async () => {
    document.body.innerHTML =
      '<header><div class="actions"><button>Edit</button></div></header><nav class="riki-sidebar">old</nav><main><article data-path="a/x.md" data-base-oid="oid1">x</article></main>'
    const fetchMock = vi.fn(async (url: string, _init?: RequestInit) => {
      if (url === '/_riki/api/delete') return json(200, { commit: 'dd', 'content-present': false })
      if (url === '/_riki/api/restore') return json(200, { commit: 'rr', url: '/a/x' })
      return new Response('<nav class="riki-sidebar">new</nav>', { status: 404 })
    })
    vi.stubGlobal('fetch', fetchMock)
    const navigate = vi.fn()
    deleteDialog(navigate)
    submit()
    await settle()
    await settle()
    expect(document.querySelector('main article')?.textContent).toContain('Deleted a/x.md.')
    expect(document.querySelector('.actions')?.children.length).toBe(0)
    expect(document.querySelector('.riki-sidebar')?.textContent).toBe('new')
    document.querySelector<HTMLButtonElement>('[data-control=undo]')!.click()
    await settle()
    const restore = fetchMock.mock.calls.find(([url]) => url === '/_riki/api/restore')!
    expect(JSON.parse(restore[1]?.body as string)).toEqual({ path: 'a/x.md', commit: 'dd' })
    expect(navigate).toHaveBeenCalledWith('/a/x')
  })

  it('a delete that made no commit offers no Undo', async () => {
    document.body.innerHTML = '<main><article data-path="a/x.md" data-base-oid="oid1">x</article></main>'
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        url === '/_riki/api/delete' ? json(200, { commit: null, 'content-present': true }) : new Response('', { status: 404 }),
      ),
    )
    deleteDialog(vi.fn())
    submit()
    await settle()
    await settle()
    expect(document.querySelector('[data-control=undo]')).toBeNull()
    expect(document.querySelector('main article')?.textContent).toContain('Deleted')
  })

  it('a refused delete keeps the page and shows the server words', async () => {
    document.body.innerHTML = '<main><article data-path="a/x.md" data-base-oid="oid1">x</article></main>'
    vi.stubGlobal('fetch', vi.fn(async () => json(409, { error: 'a/x.md changed since it was loaded' })))
    deleteDialog(vi.fn())
    submit()
    await settle()
    expect(document.querySelector('.riki-dialog-error')?.textContent).toContain('changed since it was loaded')
    expect(document.querySelector('main article')?.textContent).toBe('x')
  })
})
