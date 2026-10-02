import { describe, expect, it } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import { TextSelection } from '@milkdown/kit/prose/state'
import { getMarkdown } from '@milkdown/kit/utils'
import { BUTTONS, buildToolbar, setAlert, setBlockLevel } from '../src/toolbar'
import { withEditor } from './serialize'

/** Put the cursor (or a range) inside the first text that contains `text`. */
function select(editor: Editor, text: string, whole = false): void {
  editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    let at = -1
    view.state.doc.descendants((node, pos) => {
      if (at === -1 && node.isText && node.text?.includes(text)) at = pos + (node.text.indexOf(text) ?? 0)
    })
    if (at === -1) throw new Error(`no text ${text}`)
    const selection = TextSelection.create(view.state.doc, at, whole ? at + text.length : at)
    view.dispatch(view.state.tr.setSelection(selection))
  })
}

/** A toolbar as the session shows it once the guard passed. */
function enabledToolbar(editor: Editor): HTMLElement {
  const toolbar = buildToolbar(editor)
  toolbar.setEnabled(true)
  return toolbar.element
}

function click(toolbar: HTMLElement, control: string): void {
  const el = toolbar.querySelector<HTMLButtonElement>(`[data-control="${control}"]`)
  if (!el) throw new Error(`no control ${control}`)
  el.click()
}

const markdown = (editor: Editor) => editor.action(getMarkdown())

describe('toolbar', () => {
  it('does nothing while disabled', async () => {
    const out = await withEditor('item\n', (editor) => {
      select(editor, 'item')
      click(buildToolbar(editor).element, 'bullet-list')
      return markdown(editor)
    })
    expect(out).toBe('item\n')
  })

  it('starts disabled and enables on request', async () => {
    await withEditor('x\n', (editor) => {
      const toolbar = buildToolbar(editor)
      const controls = [...toolbar.element.querySelectorAll<HTMLButtonElement | HTMLSelectElement>('button, select')]
      expect(controls.length).toBe(BUTTONS.length + 3)
      expect(controls.every((c) => c.disabled)).toBe(true)
      toolbar.setEnabled(true)
      expect(controls.every((c) => !c.disabled)).toBe(true)
    })
  })

  it('offers every control the design doc lists', async () => {
    await withEditor('x\n', (editor) => {
      const ids = [...buildToolbar(editor).element.querySelectorAll<HTMLElement>('[data-control]')].map((el) => el.dataset['control'])
      expect(ids).toEqual([
        'block-level', 'bold', 'italic', 'strike', 'code', 'bullet-list', 'ordered-list', 'task-list',
        'quote', 'code-block', 'table', 'link', 'alert-type', 'alert',
      ])
    })
  })

  it('inserts a GFM table', async () => {
    const out = await withEditor('Intro.\n\nplaceholder\n', (editor) => {
      select(editor, 'placeholder', true)
      click(enabledToolbar(editor), 'table')
      return markdown(editor)
    })
    expect(out).toBe('Intro.\n\n| | | |\n| --- | --- | --- |\n| | | |\n')
  })

  it('makes bold, italic, strike, and inline code from a selection', async () => {
    for (const [control, expected] of [['bold', '**word**'], ['italic', '*word*'], ['strike', '~~word~~'], ['code', '`word`']]) {
      const out = await withEditor('a word here\n', (editor) => {
        select(editor, 'word', true)
        click(enabledToolbar(editor), control ?? '')
        return markdown(editor)
      })
      expect(out).toBe(`a ${expected} here\n`)
    }
  })

  it('makes lists, tasks, quotes, and code blocks', async () => {
    const cases: Array<[string, string]> = [
      ['bullet-list', '- item\n'],
      ['ordered-list', '1. item\n'],
      ['task-list', '- [ ] item\n'],
      ['quote', '> item\n'],
      ['code-block', '```\nitem\n```\n'],
    ]
    for (const [control, expected] of cases) {
      const out = await withEditor('item\n', (editor) => {
        select(editor, 'item')
        click(enabledToolbar(editor), control)
        return markdown(editor)
      })
      expect(out, control).toBe(expected)
    }
  })

  it('turns an existing bullet list into tasks', async () => {
    const out = await withEditor('- one\n- two\n', (editor) => {
      select(editor, 'two')
      click(enabledToolbar(editor), 'task-list')
      return markdown(editor)
    })
    expect(out).toBe('- one\n- [ ] two\n')
  })

  it('sets heading levels and back to a paragraph', async () => {
    const out = await withEditor('Title\n', (editor) => {
      select(editor, 'Title')
      setBlockLevel(editor, 2)
      const heading = markdown(editor)
      setBlockLevel(editor, 0)
      return [heading, markdown(editor)]
    })
    expect(out).toEqual(['## Title\n', 'Title\n'])
  })
})

describe('alerts from the toolbar', () => {
  it('wraps a paragraph in a WARNING alert', async () => {
    const out = await withEditor('Mind the gap.\n', (editor) => {
      select(editor, 'Mind')
      const toolbar = enabledToolbar(editor)
      const kind = toolbar.querySelector<HTMLSelectElement>('[data-control="alert-type"]')
      if (!kind) throw new Error('no alert-type picker')
      kind.value = 'WARNING'
      click(toolbar, 'alert')
      return markdown(editor)
    })
    expect(out).toBe('> [!WARNING]\n> Mind the gap.\n')
  })

  it('changes the type of the alert the cursor is in', async () => {
    const out = await withEditor('> [!NOTE]\n> Body.\n', (editor) => {
      select(editor, 'Body')
      setAlert(editor, 'CAUTION')
      return markdown(editor)
    })
    expect(out).toBe('> [!CAUTION]\n> Body.\n')
  })

  it('refuses an unknown type loudly', async () => {
    await withEditor('x\n', (editor) => {
      expect(() => setAlert(editor, 'HINT')).toThrow('unknown alert type HINT')
    })
  })
})
