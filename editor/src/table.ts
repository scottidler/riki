// Insert a table with no column alignment, so it serializes to the GitHub-typical `| --- |`
// delimiter row. The kit's insertTableCommand creates left-aligned cells (`| :--- |`).

import { Selection } from '@milkdown/kit/prose/state'
import type { Command } from '@milkdown/kit/prose/state'
import type { Node as ProseNode, NodeType } from '@milkdown/kit/prose/model'
import {
  tableCellSchema,
  tableHeaderRowSchema,
  tableHeaderSchema,
  tableRowSchema,
  tableSchema,
} from '@milkdown/kit/preset/gfm'
import type { Ctx } from '@milkdown/kit/ctx'
import { $command } from '@milkdown/kit/utils'

export interface TableSize {
  /** Rows including the header row; at least 2 (GFM needs a header and a delimiter row). */
  row: number
  col: number
}

export function createTable(ctx: Ctx, { row, col }: TableSize): ProseNode {
  if (row < 2 || col < 1) throw new Error(`a table needs a header row, a body row and a column; got ${row}x${col}`)
  const cells = (type: NodeType) =>
    Array.from({ length: col }, () => {
      const cell = type.createAndFill({ alignment: null })
      if (!cell) throw new Error('could not create a table cell')
      return cell
    })
  const header = tableHeaderRowSchema.type(ctx).create(null, cells(tableHeaderSchema.type(ctx)))
  const body = Array.from({ length: row - 1 }, () => tableRowSchema.type(ctx).create(null, cells(tableCellSchema.type(ctx))))
  return tableSchema.type(ctx).create(null, [header, ...body])
}

/** Replace the selection with a new table and put the cursor in its first header cell. */
export function insertTable(ctx: Ctx, size: TableSize): Command {
  return (state, dispatch) => {
    const { from } = state.selection
    const tr = state.tr.replaceSelectionWith(createTable(ctx, size))
    const selection = Selection.findFrom(tr.doc.resolve(from), 1, true)
    if (selection) tr.setSelection(selection)
    dispatch?.(tr.scrollIntoView())
    return true
  }
}

export const insertTableCommand = $command('RikiInsertTable', (ctx) => (size?: TableSize) => insertTable(ctx, size ?? { row: 2, col: 3 }))
