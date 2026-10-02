import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import type { Editor } from '@milkdown/kit/core'
import { commandsCtx, editorViewCtx } from '@milkdown/kit/core'
import { addColAfterCommand, selectColCommand, tableCellSchema, tableHeaderSchema } from '@milkdown/kit/preset/gfm'
import { getMarkdown } from '@milkdown/kit/utils'
import { makeEditor } from '../src/setup'
import { ADD_COLUMN_AFTER, TABLE_BUTTONS } from '../src/tablehandles'
import { withEditor } from './serialize'

const CANONICAL = path.join(import.meta.dirname, '..', 'fixtures', 'canonical')
const BASIC = fs.readFileSync(path.join(CANONICAL, 'tables--basic.md'), 'utf8')

const markdown = (editor: Editor) => editor.action(getMarkdown())
const frame = () => new Promise((resolve) => setTimeout(resolve, 30))

const colHandle = (root: HTMLElement) => root.querySelector<HTMLElement>('[data-role="col-drag-handle"]')!
const colMenu = (root: HTMLElement) => colHandle(root).querySelector<HTMLElement>('.button-group')!

function menuButton(root: HTMLElement, label: string): HTMLButtonElement {
  const button = [...colMenu(root).querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === label)
  if (!button) throw new Error(`no column menu button ${label}`)
  return button
}

/** table-block's buttons act on pointerdown, then the click bubbles to the handle. */
function press(button: HTMLElement): void {
  button.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, cancelable: true }))
  button.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
}

/** Click the column handle: table-block selects the hovered column (the first, before any
 *  pointer movement). The selection re-creates the table's node view, which reopens the
 *  column's menu on its next animation frame, so the handle is looked up again after. */
async function openColumnMenu(root: HTMLElement): Promise<void> {
  colHandle(root).click()
  await frame()
  expect(colMenu(root).dataset['show']).toBe('true')
}

describe('table handles', () => {
  it('render table-block around the table, with labeled buttons and the Add column after entry', async () => {
    await withEditor(BASIC, async (editor, root) => {
      expect(root.querySelector('.milkdown-table-block')).not.toBeNull()
      expect(root.querySelector('[data-role="row-drag-handle"]')).not.toBeNull()
      const labels = [...colMenu(root).querySelectorAll('button')].map((b) => b.textContent)
      expect(labels).toEqual([ADD_COLUMN_AFTER, 'Align left', 'Align center', 'Align right', 'Delete column'])
      expect(colHandle(root).textContent).toContain(TABLE_BUTTONS.col_drag_handle)
      expect(markdown(editor)).toBe(BASIC)
    })
  })

  it('a column added through the handle serializes | --- |', async () => {
    const out = await withEditor(BASIC, async (editor, root) => {
      await openColumnMenu(root)
      press(menuButton(root, ADD_COLUMN_AFTER))
      return markdown(editor)
    })
    expect(out).toBe('| Name | | Value |\n| --- | --- | --- |\n| alpha | | 1 |\n| beta | | 2 |\n')
  })

  it('the boundary + path (select the column, add after) adds an unaligned column too', async () => {
    const out = await withEditor(BASIC, (editor) => {
      editor.action((ctx) => {
        const view = ctx.get(editorViewCtx)
        const commands = ctx.get(commandsCtx)
        let pos = -1
        view.state.doc.descendants((node, at) => {
          if (node.type.name === 'table') pos = at + 1
        })
        commands.call(selectColCommand.key, { pos, index: 1 }) // reports false even when it selects
        expect(commands.call(addColAfterCommand.key)).toBe(true)
      })
      return markdown(editor)
    })
    expect(out).toBe('| Name | Value | |\n| --- | --- | --- |\n| alpha | 1 | |\n| beta | 2 | |\n')
  })

  it('Align center serializes | :---: |', async () => {
    const out = await withEditor(BASIC, async (editor, root) => {
      await openColumnMenu(root)
      press(menuButton(root, 'Align center'))
      await frame()
      return markdown(editor)
    })
    expect(out).toBe('| Name | Value |\n| :---: | --- |\n| alpha | 1 |\n| beta | 2 |\n')
  })

  it('new header and body cells default to no alignment', async () => {
    await withEditor('x\n', (editor) => {
      editor.action((ctx) => {
        expect(tableCellSchema.type(ctx).createAndFill()?.attrs['alignment']).toBeNull()
        expect(tableHeaderSchema.type(ctx).createAndFill()?.attrs['alignment']).toBeNull()
      })
    })
  })

  it('Add column after does nothing while the editor is read-only', async () => {
    const root = document.createElement('div')
    document.body.appendChild(root)
    let editable = true
    const editor = await makeEditor({ root, markdown: BASIC, sourceFile: 'README.md', editable: () => editable }).create()
    try {
      await openColumnMenu(root)
      editable = false
      const view = editor.ctx.get(editorViewCtx)
      view.dispatch(view.state.tr) // ProseMirror re-reads `editable`
      press(menuButton(root, ADD_COLUMN_AFTER))
      expect(markdown(editor)).toBe(BASIC)
    } finally {
      await editor.destroy()
      root.remove()
    }
  })
})
