import { editorViewCtx } from '@milkdown/kit/core'
import type { Editor } from '@milkdown/kit/core'
import { getMarkdown } from '@milkdown/kit/utils'
import { makeEditor } from '../src/setup'

/** Load `markdown` into riki's editor, run `edit` (if any), and return the editor and output. */
export async function withEditor<T>(
  markdown: string,
  use: (editor: Editor, root: HTMLElement) => T | Promise<T>,
  sourceFile = 'README.md',
): Promise<T> {
  const root = document.createElement('div')
  document.body.appendChild(root)
  const editor = await makeEditor({ root, markdown, sourceFile, editable: () => true }).create()
  try {
    return await use(editor, root)
  } finally {
    await editor.destroy()
    root.remove()
  }
}

/** The no-edit serialization of `markdown`. */
export function roundTrip(markdown: string): Promise<string> {
  return withEditor(markdown, (editor) => editor.action(getMarkdown()))
}

/** Names of every node in the loaded doc, for structure assertions. */
export function nodeNames(markdown: string): Promise<string[]> {
  return withEditor(markdown, (editor) => {
    const names: string[] = []
    editor.ctx.get(editorViewCtx).state.doc.descendants((node) => {
      names.push(node.type.name)
    })
    return names
  })
}
