import { describe, expect, it } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import { TextSelection } from '@milkdown/kit/prose/state'
import { getMarkdown } from '@milkdown/kit/utils'
import { TURN_INTO, shouldShowSelectionToolbar } from '../src/selectiontoolbar'
import { withEditor } from './serialize'

function select(editor: Editor, text: string, whole = true): void {
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

const markdown = (editor: Editor) => editor.action(getMarkdown())
const bar = (root: HTMLElement) => root.querySelector<HTMLElement>('.riki-selection-toolbar')

function click(root: HTMLElement, control: string): void {
  const el = root.querySelector<HTMLButtonElement>(`.riki-selection-toolbar [data-control="selection-${control}"]`)
  if (!el) throw new Error(`no control ${control}`)
  el.click()
}

describe('selection toolbar', () => {
  it('mounts inside the editor root, shown only by the provider, with the listed controls', async () => {
    await withEditor('hello\n', (editor, root) => {
      select(editor, 'hello', false) // the first editor update mounts the bar
      const el = bar(root)
      expect(el).not.toBeNull()
      expect(root.contains(el)).toBe(true)
      const controls = [...el!.querySelectorAll('[data-control]')].map((c) => (c as HTMLElement).dataset['control'])
      expect(controls.slice(0, 1)).toEqual(['selection-turn-into'])
      expect(controls).toEqual(expect.arrayContaining(['selection-bold', 'selection-italic', 'selection-strike', 'selection-code', 'selection-link']))
      expect(controls).not.toContain('selection-underline')
    })
  })

  it('lists Turn into: Text, H1-H3, bullet, numbered, task, quote', () => {
    expect(TURN_INTO.map((t) => t.label)).toEqual([
      'Text', 'Heading 1', 'Heading 2', 'Heading 3', 'Bulleted list', 'Numbered list', 'Task list', 'Quote',
    ])
  })

  it('Turn into H2 on a paragraph serializes ## ', async () => {
    const out = await withEditor('pick this\n', (editor, root) => {
      select(editor, 'pick', true)
      click(root, 'turn-h2')
      return markdown(editor)
    })
    expect(out).toBe('## pick this\n')
  })

  it('Turn into bullet, task, quote and back to text', async () => {
    const run = (id: string, md = 'item\n') =>
      withEditor(md, (editor, root) => {
        select(editor, 'item')
        click(root, id)
        return markdown(editor)
      })
    expect(await run('turn-bullet-list')).toBe('- item\n')
    expect(await run('turn-task-list')).toBe('- [ ] item\n')
    expect(await run('turn-quote')).toBe('> item\n')
    expect(await run('turn-text', '# item\n')).toBe('item\n')
  })

  it('bold, italic, strike, and code wrap the selection', async () => {
    const run = (id: string) =>
      withEditor('say word now\n', (editor, root) => {
        select(editor, 'word')
        click(root, id)
        return markdown(editor)
      })
    expect(await run('bold')).toBe('say **word** now\n')
    expect(await run('italic')).toBe('say *word* now\n')
    expect(await run('strike')).toBe('say ~~word~~ now\n')
    expect(await run('code')).toBe('say `word` now\n')
  })

  it('the Turn into button opens and closes its menu', async () => {
    await withEditor('x\n', (editor, root) => {
      select(editor, 'x', false)
      const menu = root.querySelector<HTMLElement>('.riki-selection-menu')!
      expect(menu.hidden).toBe(true)
      click(root, 'turn-into')
      expect(menu.hidden).toBe(false)
      click(root, 'turn-into')
      expect(menu.hidden).toBe(true)
    })
  })

  it('link opens the link box over the selection', async () => {
    await withEditor('see here\n', (editor, root) => {
      select(editor, 'here')
      click(root, 'link')
      expect(document.querySelector('.riki-linkbox')).not.toBeNull()
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    })
  })

  it('shows for a text range, not for a cursor or inside a code block', async () => {
    await withEditor('plain words\n\n```\ncode here\n```\n', (editor) => {
      select(editor, 'plain')
      const view = editor.ctx.get(editorViewCtx)
      expect(shouldShowSelectionToolbar(view.state)).toBe(true)
      select(editor, 'plain', false)
      expect(shouldShowSelectionToolbar(view.state)).toBe(false)
      select(editor, 'code')
      expect(shouldShowSelectionToolbar(view.state)).toBe(false)
    })
  })
})
