import { describe, expect, it } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import { TextSelection } from '@milkdown/kit/prose/state'
import { getMarkdown } from '@milkdown/kit/utils'
import { ALERT_ICON_PATHS, ALERT_KINDS, alertTitle } from '../src/alert'
import { PROSE_CLASS } from '../src/setup'
import { setAlert } from '../src/toolbar'
import { withEditor } from './serialize'

describe('alert DOM', () => {
  it('titles an alert the way comrak does: its own title, else the kind', () => {
    expect(alertTitle('WARNING', '')).toBe('Warning')
    expect(alertTitle('IMPORTANT', '   ')).toBe('Important')
    expect(alertTitle('NOTE', ' Heads up')).toBe('Heads up')
  })

  it('every kind has its own icon', () => {
    expect(new Set(ALERT_KINDS.map((kind) => ALERT_ICON_PATHS[kind])).size).toBe(ALERT_KINDS.length)
  })

  it('renders the server markup: a title row with the icon, then the body', async () => {
    const source = '> [!WARNING]\n> Never commit credentials.\n'
    await withEditor(source, (editor, root) => {
      const alert = root.querySelector('.markdown-alert.markdown-alert-warning')
      expect(alert).not.toBeNull()
      const title = alert!.querySelector(':scope > p.markdown-alert-title')
      expect(title?.textContent).toBe('Warning')
      expect(title?.getAttribute('contenteditable')).toBe('false')
      expect(title?.querySelector('svg.riki-alert-icon path')?.getAttribute('d')).toBe(ALERT_ICON_PATHS.WARNING)
      expect(alert!.querySelector(':scope > .markdown-alert-body > p')?.textContent).toBe('Never commit credentials.')
      expect(editor.action(getMarkdown())).toBe(source)
    })
  })

  it('a custom title shows as the title and still serializes byte-identical', async () => {
    const source = '> [!TIP] Shortcut\n> Use the toolbar.\n'
    await withEditor(source, (editor, root) => {
      expect(root.querySelector('.markdown-alert-title')?.textContent).toBe('Shortcut')
      expect(editor.action(getMarkdown())).toBe(source)
    })
  })

  it('changing the kind redraws the title and icon', async () => {
    await withEditor('> [!NOTE]\n> Body.\n', (editor, root) => {
      setAlertAtBody(editor)
      expect(root.querySelector('.markdown-alert-title')?.textContent).toBe('Caution')
      expect(root.querySelector('.markdown-alert-title path')?.getAttribute('d')).toBe(ALERT_ICON_PATHS.CAUTION)
      expect(editor.action(getMarkdown())).toBe('> [!CAUTION]\n> Body.\n')
    })
  })

  it('the content root carries the page stylesheet class', async () => {
    await withEditor('# Title\n', (_, root) => {
      expect(root.querySelector('.ProseMirror')?.classList.contains(PROSE_CLASS)).toBe(true)
    })
  })
})

/** Put the cursor in the alert's body text and switch the alert to CAUTION. */
function setAlertAtBody(editor: Editor): void {
  editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    let at = -1
    view.state.doc.descendants((node, pos) => {
      if (at === -1 && node.isText && node.text === 'Body.') at = pos + 1
    })
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)))
  })
  setAlert(editor, 'CAUTION')
}
