import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import { linkSchema } from '@milkdown/kit/preset/commonmark'
import { TextSelection } from '@milkdown/kit/prose/state'
import { getMarkdown } from '@milkdown/kit/utils'
import { DEBOUNCE_MS, isPaletteShortcut } from '../src/page/search'
import { isAbsoluteUrl, linkBoxIsOpen, matchPages, openLinkBox, relativeHref, urlChoice } from '../src/linkbox'
import { roundTrip, withEditor } from './serialize'

const tree = {
  folders: ['', 'a', 'c'],
  pages: [
    { path: 'README.md', url: '/', title: 'Home' },
    { path: 'a/b.md', url: '/a/b', title: 'B page' },
    { path: 'c/d.md', url: '/c/d', title: 'Dee' },
    { path: 'c/e/f.md', url: '/c/e/f', title: 'Deep' },
  ],
}

function stubFetch(searchHits: { path: string; title: string }[] = []): typeof fetch {
  return (async (input: RequestInfo | URL) => {
    const url = String(input)
    const body = url.startsWith('/_riki/api/tree') ? tree : { hits: searchHits.map((h) => ({ ...h, url: '/', heading: null, anchor: null, snippet: '', marks: [] })) }
    return new Response(JSON.stringify(body), { status: 200, headers: { 'Content-Type': 'application/json' } })
  }) as typeof fetch
}

const flush = async () => {
  for (let i = 0; i < 20; i++) await vi.advanceTimersByTimeAsync(0)
}

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(DEBOUNCE_MS + 5)
  await flush()
}

function type(text: string): void {
  const input = document.querySelector<HTMLInputElement>('.riki-linkbox input')!
  input.value = text
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

function key(k: string, init: KeyboardEventInit = {}): void {
  document.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }))
}

function select(editor: Editor, text: string, whole: boolean): void {
  editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    let at = -1
    view.state.doc.descendants((node, pos) => {
      if (at === -1 && node.isText && node.text?.includes(text)) at = pos + node.text.indexOf(text)
    })
    if (at === -1) throw new Error(`no text ${text}`)
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at, whole ? at + text.length : at)))
  })
}

function openBox(editor: Editor, sourceFile: string, fetchImpl = stubFetch()): void {
  editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    openLinkBox({ view, link: linkSchema.type(ctx), sourceFile, fetchImpl })
  })
}

afterEach(() => {
  vi.useRealTimers()
  key('Escape')
  document.body.replaceChildren()
})

describe('relativeHref', () => {
  it('climbs out of the source directory', () => {
    expect(relativeHref('a/b.md', 'c/d.md')).toBe('../c/d.md')
  })
  it('stays put in the same directory', () => {
    expect(relativeHref('a/b.md', 'a/x.md')).toBe('x.md')
    expect(relativeHref('b.md', 'x.md')).toBe('x.md')
  })
  it('descends from the root and from a parent', () => {
    expect(relativeHref('README.md', 'c/d.md')).toBe('c/d.md')
    expect(relativeHref('a/b.md', 'a/c/d.md')).toBe('c/d.md')
  })
  it('shares only the common prefix', () => {
    expect(relativeHref('a/b/c.md', 'a/x/y.md')).toBe('../x/y.md')
    expect(relativeHref('a/b/c.md', 'x.md')).toBe('../../x.md')
  })
  it('links a file to itself by name', () => {
    expect(relativeHref('a/b.md', 'a/b.md')).toBe('b.md')
  })
})

describe('urls and matching', () => {
  it('recognises absolute urls only', () => {
    expect(isAbsoluteUrl('https://example.com/x')).toBe(true)
    expect(isAbsoluteUrl('mailto:a@b.co')).toBe(true)
    expect(isAbsoluteUrl('guide/setup')).toBe(false)
    expect(isAbsoluteUrl('https://')).toBe(false)
    expect(isAbsoluteUrl('https://a b')).toBe(false)
    expect(urlChoice('  https://x.dev  ')?.href).toBe('https://x.dev')
    expect(urlChoice('dee')).toBeNull()
  })
  it('filters pages by every word of title or path and links relative to the source', () => {
    expect(matchPages(tree, 'c/d', 'a/b.md').map((c) => c.href)).toEqual(['../c/d.md'])
    expect(matchPages(tree, 'c deep', 'a/b.md').map((c) => c.href)).toEqual(['../c/e/f.md'])
    expect(matchPages(tree, 'DEE', 'a/b.md')).toHaveLength(2)
    expect(matchPages(tree, '', 'a/b.md')).toHaveLength(4)
    expect(matchPages(tree, 'nothing', 'a/b.md')).toHaveLength(0)
  })
})

describe('link box', () => {
  it('editing a/b.md, choosing c/d.md over a selection inserts [..](../c/d.md) and round-trips', async () => {
    vi.useFakeTimers()
    const out = await withEditor(
      'see here now\n',
      async (editor) => {
        select(editor, 'here', true)
        openBox(editor, 'a/b.md')
        expect(linkBoxIsOpen()).toBe(true)
        await flush()
        type('dee')
        await settle()
        key('Enter')
        expect(linkBoxIsOpen()).toBe(false)
        return editor.action(getMarkdown())
      },
      'a/b.md',
    )
    expect(out).toBe('see [here](../c/d.md) now\n')
    vi.useRealTimers()
    expect(await roundTrip(out)).toBe(out)
  })

  it('inserts the target title when nothing is selected', async () => {
    vi.useFakeTimers()
    const out = await withEditor('para\n', async (editor) => {
      select(editor, 'para', false)
      openBox(editor, 'a/b.md')
      await flush()
      type('dee')
      await settle()
      key('Enter')
      return editor.action(getMarkdown())
    }, 'a/b.md')
    expect(out).toBe('[Dee](../c/d.md)para\n')
  })

  it('passes an absolute url through unchanged', async () => {
    vi.useFakeTimers()
    const out = await withEditor('see here\n', async (editor) => {
      select(editor, 'here', true)
      openBox(editor, 'a/b.md')
      await flush()
      type('https://example.com/x')
      await settle()
      key('Enter')
      return editor.action(getMarkdown())
    }, 'a/b.md')
    expect(out).toBe('see [here](https://example.com/x)\n')
  })

  it('moves with the arrows and adds search hits the tree filter missed', async () => {
    vi.useFakeTimers()
    const out = await withEditor('x here\n', async (editor) => {
      select(editor, 'here', true)
      openBox(editor, 'a/b.md', stubFetch([{ path: 'c/e/f.md', title: 'Deep' }, { path: 'zzz/body.md', title: 'Body hit' }]))
      await flush()
      type('de')
      await settle()
      const rows = [...document.querySelectorAll('.riki-linkbox .riki-search-hit')].map((r) => r.querySelector('.riki-search-path')?.textContent)
      expect(rows).toEqual(['c/d.md', 'c/e/f.md', 'zzz/body.md'])
      key('ArrowDown')
      key('ArrowDown')
      key('Enter')
      return editor.action(getMarkdown())
    }, 'a/b.md')
    expect(out).toBe('x [here](../zzz/body.md)\n')
  })

  it('Escape closes without changing the document; Ctrl+K inside it opens no palette', async () => {
    vi.useFakeTimers()
    const out = await withEditor('keep\n', async (editor) => {
      select(editor, 'keep', true)
      openBox(editor, 'a/b.md')
      await flush()
      const input = document.querySelector<HTMLInputElement>('.riki-linkbox input')!
      const event = new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true })
      input.dispatchEvent(event)
      expect(event.defaultPrevented).toBe(true)
      expect(isPaletteShortcut(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true }))).toBe(true)
      expect(linkBoxIsOpen()).toBe(true)
      key('Escape')
      expect(linkBoxIsOpen()).toBe(false)
      return editor.action(getMarkdown())
    })
    expect(out).toBe('keep\n')
  })

  it('Ctrl+K in the editor opens the box; in a code block it does not', async () => {
    const fetchStub = vi.fn(stubFetch())
    vi.stubGlobal('fetch', fetchStub)
    await withEditor('para\n\n```\ncode\n```\n', async (editor) => {
      select(editor, 'para', false)
      const view = editor.ctx.get(editorViewCtx)
      view.dom.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }))
      expect(linkBoxIsOpen()).toBe(true)
      key('Escape')
      select(editor, 'code', false)
      view.dom.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }))
      expect(linkBoxIsOpen()).toBe(false)
    })
    vi.unstubAllGlobals()
  })

  it('shows a tree failure but still takes a url', async () => {
    vi.useFakeTimers()
    const failing = (async () => new Response('{"error":"boom"}', { status: 503 })) as unknown as typeof fetch
    await withEditor('a here\n', async (editor) => {
      select(editor, 'here', true)
      openBox(editor, 'a/b.md', failing)
      await flush()
      expect(document.querySelector('.riki-linkbox .riki-search-status')?.textContent).toContain('boom')
      type('https://x.dev')
      await settle()
      key('Enter')
      expect(await editor.action(getMarkdown())).toBe('a [here](https://x.dev)\n')
    })
  })
})
