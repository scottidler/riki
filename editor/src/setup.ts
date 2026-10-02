// The Milkdown setup riki edits with: presets, the untitled-image fix, the alert node, and the
// stringify set Phase 0a picked (`{bullet: '-', rule: '-'}` plus the `---` table handler). The
// serializer's output is the content repo's canonical format, so tests and the browser build
// share this one function.

import type { Root, Table } from 'mdast'
import type { Handle, Options as ToMarkdownOptions } from 'mdast-util-to-markdown'
import { gfmTableToMarkdown } from 'mdast-util-gfm-table'
import {
  Editor,
  defaultValueCtx,
  editorViewOptionsCtx,
  remarkStringifyOptionsCtx,
  rootCtx,
} from '@milkdown/kit/core'
import { blockContainerTypes, commonmark } from '@milkdown/kit/preset/commonmark'
import { gfm } from '@milkdown/kit/preset/gfm'
import { history } from '@milkdown/kit/plugin/history'
import { $remark } from '@milkdown/kit/utils'
import { visit } from 'unist-util-visit'
import { alertHandler, alertPlugins } from './alert'
import { imageView } from './images'
import { insertTableCommand } from './table'
import { taskPlugins } from './tasks'

/** `@milkdown/preset-commonmark` 7.22.2 passes mdast `title: null` into an image attr declared
 *  `validate: 'string'`; ProseMirror throws and the image is dropped on load. Registered ahead
 *  of the presets so the coercion runs before their transforms. */
export const imageTitleFix = $remark('rikiImageTitleFix', () => () => (tree: Root) => {
  visit(tree, 'image', (node) => {
    node.title ??= ''
    node.alt ??= ''
  })
})

const gfmTable = gfmTableToMarkdown({ tablePipeAlign: false }).handlers?.['table']
if (!gfmTable) throw new Error('mdast-util-gfm-table exposes no table handler')
const innerTable: Handle = gfmTable

/** gfm's table handler without pipe alignment, delimiter cells widened to `---` (alignment
 *  colons kept): the GitHub-typical `| --- |` row. */
export const tableHandler: Handle = (node: Table, parent, state, info) => {
  const lines = innerTable(node, parent, state, info).split('\n')
  const delimiter = lines[1]
  if (delimiter !== undefined) lines[1] = delimiter.replace(/-+/g, '---')
  return lines.join('\n')
}

/** The chosen stringify set, merged over Milkdown's defaults. */
export function stringifyOptions(prev: ToMarkdownOptions): ToMarkdownOptions {
  return {
    ...prev,
    bullet: '-',
    rule: '-',
    handlers: { ...prev.handlers, alert: alertHandler, table: tableHandler },
  }
}

export interface EditorSetup {
  root: HTMLElement
  markdown: string
  /** Repo path of the file being edited; relative images display resolved against its
   *  directory (the document keeps the author's `src`). */
  sourceFile: string
  /** Read by ProseMirror on every transaction; flip it and dispatch to change editability. */
  editable: () => boolean
}

/** Build (not create) an editor with riki's schema and serializer. Callers add UI plugins. */
export function makeEditor({ root, markdown, sourceFile, editable }: EditorSetup): Editor {
  return Editor.make()
    .config((ctx) => {
      ctx.set(rootCtx, root)
      ctx.set(defaultValueCtx, markdown)
      ctx.update(editorViewOptionsCtx, (prev) => ({ ...prev, editable }))
      ctx.update(remarkStringifyOptionsCtx, stringifyOptions)
    })
    .use(imageTitleFix)
    .use(commonmark)
    .use(gfm)
    .use(history)
    .use(taskPlugins)
    .use(insertTableCommand)
    .use(alertPlugins)
    .use(imageView(sourceFile))
    .config((ctx) => {
      ctx.update(blockContainerTypes.key, (prev) => [...prev, 'alert'])
    })
}
