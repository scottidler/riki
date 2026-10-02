// The no-syntax authoring UI: one toolbar of kit commands. Authors never type Markdown; the
// input rules (`## `) stay on as a shortcut, and nothing here needs them.

import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import {
  createCodeBlockCommand,
  toggleEmphasisCommand,
  toggleInlineCodeCommand,
  toggleStrongCommand,
  turnIntoTextCommand,
  wrapInBlockquoteCommand,
  wrapInBulletListCommand,
  wrapInHeadingCommand,
  wrapInOrderedListCommand,
} from '@milkdown/kit/preset/commonmark'
import { toggleStrikethroughCommand } from '@milkdown/kit/preset/gfm'
import { toggleLinkCommand } from '@milkdown/kit/component/link-tooltip'
import { callCommand } from '@milkdown/kit/utils'
import { ALERT_KINDS, isAlertKind, setAlertCommand } from './alert'
import { insertTableCommand } from './table'
import { makeTasksCommand } from './tasks'

/** A new table: a header row plus one body row, three columns. */
export const NEW_TABLE = { row: 2, col: 3 }

export interface Button {
  id: string
  label: string
  title: string
  run: (editor: Editor) => void
}

const call = (editor: Editor, key: Parameters<typeof callCommand>[0], payload?: unknown) =>
  editor.action(callCommand(key, payload))

/** Turn the selection into a task list: a bullet list first if it is in no list yet. */
function taskList(editor: Editor): void {
  if (!call(editor, makeTasksCommand.key)) {
    call(editor, wrapInBulletListCommand.key)
    call(editor, makeTasksCommand.key)
  }
}

export const BUTTONS: Button[] = [
  { id: 'bold', label: 'B', title: 'Bold', run: (e) => call(e, toggleStrongCommand.key) },
  { id: 'italic', label: 'I', title: 'Italic', run: (e) => call(e, toggleEmphasisCommand.key) },
  { id: 'strike', label: 'S', title: 'Strikethrough', run: (e) => call(e, toggleStrikethroughCommand.key) },
  { id: 'code', label: 'Code', title: 'Inline code', run: (e) => call(e, toggleInlineCodeCommand.key) },
  { id: 'bullet-list', label: '• List', title: 'Bulleted list', run: (e) => call(e, wrapInBulletListCommand.key) },
  { id: 'ordered-list', label: '1. List', title: 'Numbered list', run: (e) => call(e, wrapInOrderedListCommand.key) },
  { id: 'task-list', label: '☐ Tasks', title: 'Task list', run: taskList },
  { id: 'quote', label: 'Quote', title: 'Quote', run: (e) => call(e, wrapInBlockquoteCommand.key) },
  { id: 'code-block', label: 'Code block', title: 'Code block', run: (e) => call(e, createCodeBlockCommand.key) },
  { id: 'table', label: 'Table', title: 'Insert a table', run: (e) => call(e, insertTableCommand.key, NEW_TABLE) },
  { id: 'link', label: 'Link', title: 'Link the selected text', run: (e) => call(e, toggleLinkCommand.key) },
]

/** Block styles the heading picker offers; 0 is a plain paragraph. */
export const BLOCK_LEVELS = [0, 1, 2, 3, 4, 5, 6] as const

export function setBlockLevel(editor: Editor, level: number): void {
  if (level === 0) call(editor, turnIntoTextCommand.key)
  else call(editor, wrapInHeadingCommand.key, level)
}

export function setAlert(editor: Editor, kind: string): void {
  if (!isAlertKind(kind)) throw new Error(`unknown alert type ${kind}`)
  call(editor, setAlertCommand.key, kind)
}

export interface Toolbar {
  element: HTMLElement
  setEnabled: (enabled: boolean) => void
}

/** Build the toolbar for `editor`. Controls keep the editor's selection: a mousedown on them
 *  never takes focus, and each action hands focus back to the editor. */
export function buildToolbar(editor: Editor): Toolbar {
  const element = document.createElement('div')
  element.className = 'riki-toolbar'
  element.setAttribute('role', 'toolbar')
  const controls: Array<HTMLButtonElement | HTMLSelectElement> = []
  const focus = () => editor.action((ctx) => ctx.get(editorViewCtx).focus())

  const levels = document.createElement('select')
  levels.dataset['control'] = 'block-level'
  levels.title = 'Text style'
  for (const level of BLOCK_LEVELS) {
    levels.append(new Option(level === 0 ? 'Paragraph' : `Heading ${level}`, String(level)))
  }
  levels.addEventListener('change', () => {
    setBlockLevel(editor, Number(levels.value))
    levels.value = '0'
    focus()
  })
  element.append(levels)
  controls.push(levels)

  for (const button of BUTTONS) {
    const el = document.createElement('button')
    el.type = 'button'
    el.dataset['control'] = button.id
    el.textContent = button.label
    el.title = button.title
    el.setAttribute('aria-label', button.title)
    el.addEventListener('mousedown', (event) => event.preventDefault())
    el.addEventListener('click', () => {
      button.run(editor)
      if (button.id !== 'link') focus()
    })
    element.append(el)
    controls.push(el)
  }

  const kinds = document.createElement('select')
  kinds.dataset['control'] = 'alert-type'
  kinds.title = 'Alert type'
  for (const kind of ALERT_KINDS) kinds.append(new Option(kind[0] + kind.slice(1).toLowerCase(), kind))
  const alert = document.createElement('button')
  alert.type = 'button'
  alert.dataset['control'] = 'alert'
  alert.textContent = 'Alert'
  alert.title = 'Make an alert of the chosen type'
  alert.setAttribute('aria-label', alert.title)
  alert.addEventListener('mousedown', (event) => event.preventDefault())
  alert.addEventListener('click', () => {
    setAlert(editor, kinds.value)
    focus()
  })
  element.append(kinds, alert)
  controls.push(kinds, alert)

  const setEnabled = (enabled: boolean) => {
    for (const control of controls) control.disabled = !enabled
  }
  setEnabled(false)
  return { element, setEnabled }
}
