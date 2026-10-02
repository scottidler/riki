// New table cells carry no column alignment, so a new table or column serializes to the
// GitHub-typical `| --- |` delimiter row. preset-gfm declares the cell `alignment` attr with
// `default: "left"`, so every `createAndFill()` cell (insertTableCommand, the `|NxM| ` input
// rule, prosemirror-tables' add-column, the table-block buttons) would write `| :--- |`.
// Parsed cells set `alignment` from the Markdown (null for `---`), so the default only reaches
// cells the editor creates. An alignment the author picks still serializes as `:---:` etc.

import type { MilkdownPlugin } from '@milkdown/kit/ctx'
import { tableCellSchema, tableHeaderSchema } from '@milkdown/kit/preset/gfm'
import type { $NodeSchema } from '@milkdown/kit/utils'

function withUnalignedDefault<T extends string>(schema: $NodeSchema<T>): $NodeSchema<T> {
  return schema.extendSchema((prev) => (ctx) => {
    const spec = prev(ctx)
    const alignment = spec.attrs?.['alignment']
    if (!alignment) throw new Error(`preset-gfm ${schema.id} has no alignment attr to default to null`)
    return { ...spec, attrs: { ...spec.attrs, alignment: { ...alignment, default: null } } }
  })
}

/** Registered after `gfm`; replaces its `table_cell` and `table_header` node specs by id. */
export const unalignedTableCells: MilkdownPlugin[] = [
  withUnalignedDefault(tableCellSchema),
  withUnalignedDefault(tableHeaderSchema),
].flat()
