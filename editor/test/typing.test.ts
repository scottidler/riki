import { describe, expect, it } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import { TextSelection } from '@milkdown/kit/prose/state'
import { getMarkdown } from '@milkdown/kit/utils'
import { withEditor } from './serialize'

/** Type through ProseMirror's handleTextInput so input rules fire as they do for a keystroke. */
function type(editor: Editor, text: string): void {
  const view = editor.ctx.get(editorViewCtx)
  for (const ch of text) {
    const { from, to } = view.state.selection
    if (!view.someProp('handleTextInput', (f) => f(view, from, to, ch, () => view.state.tr.insertText(ch, from, to)))) {
      view.dispatch(view.state.tr.insertText(ch, from, to))
    }
  }
}

const startEmpty = (editor: Editor) => {
  const view = editor.ctx.get(editorViewCtx)
  view.dispatch(view.state.tr.setSelection(TextSelection.atStart(view.state.doc)))
}

describe('typing shortcuts', () => {
  it.each(['NOTE', 'TIP', 'IMPORTANT', 'WARNING', 'CAUTION'])('> [!%s] then text becomes that alert', async (kind) => {
    const out = await withEditor('', (editor) => {
      startEmpty(editor)
      type(editor, `> [!${kind}] text`)
      return editor.action(getMarkdown())
    })
    expect(out).toBe(`> [!${kind}]\n> text\n`)
  })

  it('[!TIP] typed outside a quote stays text', async () => {
    const out = await withEditor('', (editor) => {
      startEmpty(editor)
      type(editor, '[!TIP] x')
      return editor.action(getMarkdown())
    })
    expect(out).toBe('\\[!TIP] x\n')
  })

  it('|2x2| builds an unaligned table', async () => {
    const out = await withEditor('', (editor) => {
      startEmpty(editor)
      type(editor, '|2x2| ')
      return editor.action(getMarkdown())
    })
    expect(out).toContain('| --- | --- |')
    expect(out).not.toContain(':')
  })
})
