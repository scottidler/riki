import fs from 'node:fs'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import type { EditorView } from '@milkdown/kit/prose/view'
import { getMarkdown } from '@milkdown/kit/utils'
import { BLOCK_MENU, addBlockBelow, canTurnInto, deleteBlock, duplicateBlock } from '../src/blockhandle'
import type { Block } from '../src/blockhandle'
import { withEditor } from './serialize'

const CANONICAL = path.join(import.meta.dirname, '..', 'fixtures', 'canonical')
const fixture = (name: string) => fs.readFileSync(path.join(CANONICAL, `${name}.md`), 'utf8')

const markdown = (editor: Editor) => editor.action(getMarkdown())
const viewOf = (editor: Editor): EditorView => editor.ctx.get(editorViewCtx)
const frame = () => new Promise((resolve) => setTimeout(resolve, 30))

/** The doc's `index`-th top-level block. */
function topBlock(editor: Editor, index: number): Block {
  const { doc } = viewOf(editor).state
  let pos = 0
  for (let i = 0; i < index; i++) pos += doc.child(i).nodeSize
  return { pos, node: doc.child(index) }
}

const realElementFromPoint = document.elementFromPoint
afterEach(() => {
  document.elementFromPoint = realElementFromPoint
})

/** Hover the `index`-th top-level block the way a pointer does: jsdom has no layout, so the
 *  coordinate lookups the block plugin makes answer with that block. */
async function hover(editor: Editor, index: number): Promise<void> {
  const view = viewOf(editor)
  const target = topBlock(editor, index)
  await frame() // the provider binds on the first animation frame
  document.elementFromPoint = () => view.dom
  view.posAtCoords = () => ({ pos: target.pos + 1, inside: target.pos })
  view.dom.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 1, clientY: 1 }))
  await frame()
}

function click(root: HTMLElement, control: string): void {
  const el = root.querySelector<HTMLButtonElement>(`[data-control="${control}"]`)
  if (!el) throw new Error(`no control ${control}`)
  el.click()
}

/** `source` with its `index`-th blank-line-separated block written twice. */
function withRepeated(source: string, index: number): string {
  const blocks = source.replace(/\n$/, '').split('\n\n')
  blocks.splice(index, 0, blocks[index]!)
  return `${blocks.join('\n\n')}\n`
}

describe('block handle', () => {
  it('mounts the gutter + and the drag handle inside the editor root, hidden until hover', async () => {
    await withEditor('one\n\ntwo\n', async (editor, root) => {
      await frame()
      const handle = root.querySelector<HTMLElement>('.riki-block-handle')
      expect(handle).not.toBeNull()
      expect(handle!.dataset['show']).toBe('false')
      expect(handle!.draggable).toBe(true)
      expect([...handle!.querySelectorAll('[data-control]')].map((c) => (c as HTMLElement).dataset['control'])).toEqual([
        'block-add',
        'block-handle',
      ])
      await hover(editor, 1)
      expect(handle!.dataset['show']).toBe('true')
    })
  })

  it('Duplicate on a canonical fixture\'s second block repeats exactly that block', async () => {
    for (const name of ['other--mixed-page', 'blank-line-inserted--blocks-separated']) {
      const source = fixture(name)
      const out = await withEditor(source, async (editor, root) => {
        expect(markdown(editor)).toBe(source)
        await hover(editor, 1)
        click(root, 'block-handle')
        expect(root.querySelector<HTMLElement>('.riki-block-menu')!.hidden).toBe(false)
        click(root, 'block-duplicate')
        expect(root.querySelector<HTMLElement>('.riki-block-menu')!.hidden).toBe(true)
        return markdown(editor)
      })
      expect(out).toBe(withRepeated(source, 1))
    }
  })

  it('Delete from the menu removes the hovered block', async () => {
    const out = await withEditor('# Guide\n\nA canonical page.\n\n- one\n- two\n', async (editor, root) => {
      await hover(editor, 1)
      click(root, 'block-handle')
      click(root, 'block-delete')
      return markdown(editor)
    })
    expect(out).toBe('# Guide\n\n- one\n- two\n')
  })

  it('Turn into retypes the whole hovered block', async () => {
    const out = await withEditor('first\n\nsecond line\n', async (editor, root) => {
      await hover(editor, 1)
      click(root, 'block-handle')
      click(root, 'block-turn-into')
      expect(root.querySelector<HTMLElement>('.riki-block-submenu')!.hidden).toBe(false)
      click(root, 'block-turn-h2')
      expect(viewOf(editor).state.selection.empty).toBe(true)
      return markdown(editor)
    })
    expect(out).toBe('first\n\n## second line\n')
  })

  it('Turn into is off for a block with no text (divider, table)', async () => {
    await withEditor('one\n\n---\n\n| a |\n| --- |\n| 1 |\n', (editor) => {
      const { state } = viewOf(editor)
      expect(canTurnInto(state, topBlock(editor, 0))).toBe(true)
      expect(canTurnInto(state, topBlock(editor, 1))).toBe(false)
      expect(canTurnInto(state, topBlock(editor, 2))).toBe(false)
    })
  })

  it('the menu lists Turn into, Duplicate, Delete and closes on Escape', async () => {
    expect(BLOCK_MENU).toEqual(['turn-into', 'duplicate', 'delete'])
    await withEditor('one\n\ntwo\n', async (editor, root) => {
      await hover(editor, 0)
      click(root, 'block-handle')
      const menu = root.querySelector<HTMLElement>('.riki-block-menu')!
      const labels = [...menu.querySelectorAll(':scope > button')].map((b) => b.textContent)
      expect(labels).toEqual(['Turn into', 'Duplicate', 'Delete'])
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
      expect(menu.hidden).toBe(true)
    })
  })

  it('+ adds an empty paragraph below the block with the cursor in it', async () => {
    const out = await withEditor('one\n\ntwo\n', async (editor, root) => {
      await hover(editor, 0)
      click(root, 'block-add')
      const view = viewOf(editor)
      expect(view.state.selection.$from.parent.type.name).toBe('paragraph')
      expect(view.state.selection.$from.parent.content.size).toBe(0)
      view.dispatch(view.state.tr.insertText('new'))
      return markdown(editor)
    })
    expect(out).toBe('one\n\nnew\n\ntwo\n')
  })

  it('an action on a stale block does nothing', async () => {
    await withEditor('one\n\ntwo\n', (editor) => {
      const view = viewOf(editor)
      const stale = topBlock(editor, 1)
      view.dispatch(view.state.tr.insertText('x', 1))
      expect(duplicateBlock(view, stale)).toBe(false)
      expect(deleteBlock(view, stale)).toBe(false)
      expect(addBlockBelow(view, stale)).toBe(false)
      expect(markdown(editor)).toBe('xone\n\ntwo\n')
    })
  })

  it('deleting the only item of a list removes the list', async () => {
    await withEditor('one\n\n- solo\n\ntwo\n', (editor) => {
      const view = viewOf(editor)
      const list = topBlock(editor, 1)
      const item = { pos: list.pos + 1, node: list.node.child(0) }
      expect(deleteBlock(view, item)).toBe(true)
      expect(markdown(editor)).toBe('one\n\ntwo\n')
    })
  })

  it('a read-only editor never shows the handle', async () => {
    let editable = true
    const root = document.createElement('div')
    document.body.append(root)
    const { makeEditor } = await import('../src/setup')
    const editor = await makeEditor({ root, markdown: 'one\n', sourceFile: 'README.md', editable: () => editable }).create()
    try {
      await hover(editor, 0)
      const handle = root.querySelector<HTMLElement>('.riki-block-handle')!
      expect(handle.dataset['show']).toBe('true')
      editable = false
      const view = viewOf(editor)
      view.updateState(view.state)
      expect(handle.dataset['show']).toBe('false')
    } finally {
      await editor.destroy()
      root.remove()
    }
  })
})
