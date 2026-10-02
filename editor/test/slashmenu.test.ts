import { describe, expect, it } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import { Selection, TextSelection } from '@milkdown/kit/prose/state'
import { getMarkdown } from '@milkdown/kit/utils'
import { SLASH_ITEMS, filterSlashItems, openSlashMenu } from '../src/slashmenu'
import { withEditor } from './serialize'

const markdown = (editor: Editor) => editor.action(getMarkdown())
const viewOf = (editor: Editor) => editor.ctx.get(editorViewCtx)
const frame = () => new Promise((resolve) => setTimeout(resolve, 40))
const menuOf = (root: HTMLElement) => root.querySelector<HTMLElement>('.riki-slash-menu')!
const shown = (root: HTMLElement) => root.querySelector<HTMLElement>('.riki-slash-menu')?.dataset['show'] === 'true'
const labels = (root: HTMLElement) =>
  [...menuOf(root).querySelectorAll<HTMLElement>('button')].filter((b) => !b.hidden).map((b) => b.textContent)

/** Put the cursor at the end of the last paragraph and type `text` there. */
function type(editor: Editor, text: string): void {
  const view = viewOf(editor)
  view.dispatch(view.state.tr.setSelection(Selection.atEnd(view.state.doc)).insertText(text))
}

function emptyParagraphAtEnd(editor: Editor): void {
  const view = viewOf(editor)
  const paragraph = view.state.schema.nodes['paragraph']!
  const end = view.state.doc.content.size
  const tr = view.state.tr.insert(end, paragraph.create())
  view.dispatch(tr.setSelection(TextSelection.create(tr.doc, end + 1)))
}

describe('slash menu', () => {
  it('lists exactly the 16 items', async () => {
    const expected = [
      'Text', 'Heading 1', 'Heading 2', 'Heading 3', 'Bulleted list', 'Numbered list', 'Task list', 'Quote',
      'Code block', 'Table', 'Divider', 'Note', 'Tip', 'Important', 'Warning', 'Caution',
    ]
    expect(SLASH_ITEMS.map((i) => i.label)).toEqual(expected)
    await withEditor('before\n', async (editor, root) => {
      emptyParagraphAtEnd(editor)
      type(editor, '/')
      await frame()
      expect(shown(root)).toBe(true)
      expect(labels(root)).toEqual(expected)
    })
  })

  it('is hidden until a `/` starts an empty paragraph, and not for a `/` mid-text', async () => {
    await withEditor('before\n', async (editor, root) => {
      await frame()
      expect(shown(root)).toBe(false)
      type(editor, ' and/') // text before the slash
      await frame()
      expect(shown(root)).toBe(false)
    })
  })

  it('filters as you type and hides when nothing matches', async () => {
    await withEditor('before\n', async (editor, root) => {
      emptyParagraphAtEnd(editor)
      type(editor, '/head')
      await frame()
      expect(labels(root)).toEqual(['Heading 1', 'Heading 2', 'Heading 3'])
      type(editor, 'zzz')
      await frame()
      expect(shown(root)).toBe(false)
    })
    expect(filterSlashItems('WARN').map((i) => i.label)).toEqual(['Warning'])
    expect(filterSlashItems('')).toHaveLength(16)
  })

  it('Warning on an empty paragraph serializes > [!WARNING]', async () => {
    const out = await withEditor('before\n', async (editor, root) => {
      emptyParagraphAtEnd(editor)
      type(editor, '/')
      await frame()
      root.querySelector<HTMLButtonElement>('[data-control="slash-warning"]')!.click()
      expect(shown(root)).toBe(false)
      expect(markdown(editor)).toMatch(/^before\n\n> \[!WARNING\]\n/) // marker line; the empty body is Milkdown's `<br />`
      type(editor, 'careful')
      return markdown(editor)
    })
    expect(out).toBe('before\n\n> [!WARNING]\n> careful\n')
  })

  it('removes the typed filter and applies Heading 2', async () => {
    const out = await withEditor('before\n', async (editor, root) => {
      emptyParagraphAtEnd(editor)
      type(editor, '/head')
      await frame()
      root.querySelector<HTMLButtonElement>('[data-control="slash-h2"]')!.click()
      type(editor, 'Title')
      return markdown(editor)
    })
    expect(out).toBe('before\n\n## Title\n')
  })

  it('inserts a table, a divider, a code block and a task list from the menu', async () => {
    const cases: Array<[string, RegExp]> = [
      ['table', /^before\n\n\| \| \| \|\n\| --- \| --- \| --- \|\n/],
      ['divider', /^before\n\n---\n/],
      ['code-block', /^before\n\n```\n```\n$/],
      ['task-list', /^before\n\n- \[ \] /],
    ]
    for (const [id, expected] of cases) {
      const out = await withEditor('before\n', async (editor, root) => {
        emptyParagraphAtEnd(editor)
        type(editor, '/')
        await frame()
        root.querySelector<HTMLButtonElement>(`[data-control="slash-${id}"]`)!.click()
        return markdown(editor)
      })
      expect(out, id).toMatch(expected)
    }
  })

  it('keys: arrows move, Enter picks, Escape closes', async () => {
    const out = await withEditor('before\n', async (editor, root) => {
      emptyParagraphAtEnd(editor)
      type(editor, '/')
      await frame()
      const view = viewOf(editor)
      const press = (key: string) => view.dom.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
      press('ArrowDown')
      expect(menuOf(root).querySelector('.is-selected')!.textContent).toBe('Heading 1')
      press('ArrowUp')
      expect(menuOf(root).querySelector('.is-selected')!.textContent).toBe('Text')
      press('Escape')
      await frame()
      expect(shown(root)).toBe(false)
      return markdown(editor)
    })
    expect(out).toBe('before\n\n/\n')
  })

  it('Enter chooses the highlighted entry', async () => {
    const out = await withEditor('before\n', async (editor, root) => {
      emptyParagraphAtEnd(editor)
      type(editor, '/quote')
      await frame()
      viewOf(editor).dom.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
      type(editor, 'said')
      expect(shown(root)).toBe(false)
      return markdown(editor)
    })
    expect(out).toBe('before\n\n> said\n')
  })

  it('openSlashMenu shows the menu on an empty paragraph and refuses elsewhere', async () => {
    await withEditor('before\n', async (editor, root) => {
      const view = viewOf(editor)
      view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 2)))
      expect(openSlashMenu(view)).toBe(false)
      emptyParagraphAtEnd(editor)
      expect(openSlashMenu(view)).toBe(true)
      expect(shown(root)).toBe(true)
      expect(labels(root)).toHaveLength(16)
    })
  })
})
