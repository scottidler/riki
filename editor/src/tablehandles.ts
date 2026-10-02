// Table handles: Milkdown's table-block node view (`@milkdown/kit/component/table-block`) gives
// every table a column handle (select, drag, align left/center/right, delete), a row handle
// (select, drag, delete), and + buttons on the cell boundaries (add a row or column there).
// riki labels those buttons and adds an "Add column after" entry to the column handle's menu,
// which table-block 7.22.2 does not have. New cells are unaligned (`table.ts`), so added
// columns serialize `| --- |`.

import type { Ctx } from '@milkdown/kit/ctx'
import { commandsCtx } from '@milkdown/kit/core'
import { tableBlock, tableBlockConfig } from '@milkdown/kit/component/table-block'
import type { RenderType } from '@milkdown/kit/component/table-block'
import { addColAfterCommand } from '@milkdown/kit/preset/gfm'
import { Plugin, PluginKey } from '@milkdown/kit/prose/state'
import type { EditorView } from '@milkdown/kit/prose/view'
import { $prose } from '@milkdown/kit/utils'

/** The text of each table-block button; the text is also the button's accessible name. */
export const TABLE_BUTTONS: Record<RenderType, string> = {
  add_row: '+',
  add_col: '+',
  delete_row: 'Delete row',
  delete_col: 'Delete column',
  align_col_left: 'Align left',
  align_col_center: 'Align center',
  align_col_right: 'Align right',
  col_drag_handle: '⋯',
  row_drag_handle: '⋮',
}

export const ADD_COLUMN_AFTER = 'Add column after'

export function configureTableHandles(ctx: Ctx): void {
  ctx.update(tableBlockConfig.key, (prev) => ({ ...prev, renderButton: (type: RenderType) => TABLE_BUTTONS[type] }))
}

const COLUMN_MENU = '.milkdown-table-block [data-role="col-drag-handle"] .button-group'

/** Add the column after the selected one. Clicking the column handle selects its column. */
function addColumnAfter(ctx: Ctx, view: EditorView): void {
  if (!view.editable) return
  ctx.get(commandsCtx).call(addColAfterCommand.key)
  view.focus()
}

/** Put the riki entry first in every column menu that lacks it. table-block renders its
 *  handles once per table (its render reads no reactive state), so the entry stays put. */
export function addColumnMenuEntries(ctx: Ctx, view: EditorView): void {
  for (const menu of view.dom.querySelectorAll<HTMLElement>(COLUMN_MENU)) {
    if (menu.querySelector('[data-control="table-add-col-after"]')) continue
    const button = document.createElement('button')
    button.type = 'button'
    button.dataset['control'] = 'table-add-col-after'
    button.textContent = ADD_COLUMN_AFTER
    // Acts on pointerdown like table-block's own buttons. The click must not reach the
    // handle: its click handler re-selects the hovered column and toggles the menu back open.
    button.addEventListener('pointerdown', (event) => {
      event.preventDefault()
      event.stopPropagation()
      addColumnAfter(ctx, view)
    })
    button.addEventListener('click', (event) => event.stopPropagation())
    menu.prepend(button)
  }
}

export const tableMenuEntries = $prose(
  (ctx) =>
    new Plugin({
      key: new PluginKey('RIKI_TABLE_MENU'),
      view: (view) => {
        addColumnMenuEntries(ctx, view)
        return { update: (updated) => addColumnMenuEntries(ctx, updated) }
      },
    }),
)

/** table-block's config and node view, then riki's column-menu entry. */
export const tableHandles = [...tableBlock, tableMenuEntries]
