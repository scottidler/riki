// The no-syntax authoring UI: one toolbar of kit commands. Authors never type Markdown; the
// input rules (`## `) stay on as a shortcut, and nothing here needs them.

import type { Ctx } from '@milkdown/kit/ctx'
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
import { ALERT_ICON_PATHS, ALERT_KINDS, isAlertKind, setAlertCommand } from './alert'
import { insertTableCommand } from './table'
import { makeTasksCommand } from './tasks'

/** A new table: a header row plus one body row, three columns. */
export const NEW_TABLE = { row: 2, col: 3 }

export interface Button {
  id: string
  title: string
  /** 16x16 stroked path data for the button's icon. */
  icon: string
  /** Starts a new group: a divider goes before it. */
  group?: boolean
  run: (editor: Editor) => void
}

const call = (editor: Editor, key: Parameters<typeof callCommand>[0], payload?: unknown) =>
  editor.action(callCommand(key, payload))

/** Turn the selection into a task list: a bullet list first if it is in no list yet. */
export function makeTaskList(ctx: Ctx): void {
  if (!callCommand(makeTasksCommand.key)(ctx)) {
    callCommand(wrapInBulletListCommand.key)(ctx)
    callCommand(makeTasksCommand.key)(ctx)
  }
}

const taskList = (editor: Editor): void => editor.action(makeTaskList)

export const BUTTONS: Button[] = [
  { id: 'bold', title: 'Bold', icon: 'M4.75 2.75h4a2.6 2.6 0 0 1 0 5.2h-4Zm0 5.2h4.75a2.65 2.65 0 0 1 0 5.3H4.75Z', run: (e) => call(e, toggleStrongCommand.key) },
  { id: 'italic', title: 'Italic', icon: 'M7 2.75h5.25M3.75 13.25H9M9.65 2.75l-3.3 10.5', run: (e) => call(e, toggleEmphasisCommand.key) },
  { id: 'strike', title: 'Strikethrough', icon: 'M2.75 8h10.5M10.85 4.75C10.4 3.5 9.3 2.75 8 2.75c-1.75 0-3 .95-3 2.3 0 .75.35 1.35 1.05 1.8M5.1 11.1c.4 1.35 1.55 2.15 3.05 2.15 1.85 0 3.1-.95 3.1-2.4 0-.5-.15-.95-.45-1.35', run: (e) => call(e, toggleStrikethroughCommand.key) },
  { id: 'code', title: 'Inline code', icon: 'M5.75 4.5 2.25 8l3.5 3.5M10.25 4.5l3.5 3.5-3.5 3.5', run: (e) => call(e, toggleInlineCodeCommand.key) },
  { id: 'bullet-list', title: 'Bulleted list', group: true, icon: 'M6.25 4h7M6.25 8h7M6.25 12h7M2.9 4h.01M2.9 8h.01M2.9 12h.01', run: (e) => call(e, wrapInBulletListCommand.key) },
  { id: 'ordered-list', title: 'Numbered list', icon: 'M6.75 4h6.5M6.75 8h6.5M6.75 12h6.5M2.5 2.75h1v3M2.25 5.75h2M2.25 9.5c.1-.5.5-.75.95-.75.55 0 .9.35.9.8 0 .8-1.85 1.25-1.85 2.45h1.9', run: (e) => call(e, wrapInOrderedListCommand.key) },
  { id: 'task-list', title: 'Task list', icon: 'M2.5 3.25h3.25v3.25H2.5ZM2.5 9.5h3.25v3.25H2.5ZM8.25 4.9h5M8.25 11.1h5', run: taskList },
  { id: 'quote', title: 'Quote', group: true, icon: 'M3.25 3.5v9M6.5 5h6.25M6.5 8h6.25M6.5 11h4', run: (e) => call(e, wrapInBlockquoteCommand.key) },
  { id: 'code-block', title: 'Code block', icon: 'M2.75 3.25h10.5v9.5H2.75ZM6.25 6.25 4.75 8l1.5 1.75M9.75 6.25 11.25 8l-1.5 1.75', run: (e) => call(e, createCodeBlockCommand.key) },
  { id: 'table', title: 'Insert a table', icon: 'M2.75 3.25h10.5v9.5H2.75ZM2.75 6.5h10.5M2.75 9.75h10.5M6.25 6.5v6.25', run: (e) => call(e, insertTableCommand.key, NEW_TABLE) },
  { id: 'link', title: 'Link the selected text', icon: 'M6.75 9.25a3 3 0 0 0 4.24 0l2.13-2.12a3 3 0 0 0-4.25-4.25l-.7.71M9.25 6.75a3 3 0 0 0-4.24 0L2.88 8.87a3 3 0 0 0 4.25 4.25l.7-.71', run: (e) => call(e, toggleLinkCommand.key) },
]

const SVG_NS = 'http://www.w3.org/2000/svg'

/** A 16x16 stroked icon for a toolbar control. */
export function icon(path: string): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg')
  svg.setAttribute('viewBox', '0 0 16 16')
  svg.setAttribute('aria-hidden', 'true')
  const stroke = document.createElementNS(SVG_NS, 'path')
  stroke.setAttribute('d', path)
  svg.append(stroke)
  return svg
}

function divider(): HTMLElement {
  const el = document.createElement('span')
  el.className = 'riki-toolbar-divider'
  el.setAttribute('aria-hidden', 'true')
  return el
}

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
  element.append(levels, divider())
  controls.push(levels)

  for (const button of BUTTONS) {
    if (button.group) element.append(divider())
    const el = document.createElement('button')
    el.type = 'button'
    el.dataset['control'] = button.id
    el.append(icon(button.icon))
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
  alert.className = 'riki-toolbar-text'
  alert.append(icon(ALERT_ICON_PATHS.NOTE), 'Alert')
  alert.title = 'Make an alert of the chosen type'
  alert.setAttribute('aria-label', alert.title)
  alert.addEventListener('mousedown', (event) => event.preventDefault())
  alert.addEventListener('click', () => {
    setAlert(editor, kinds.value)
    focus()
  })
  element.append(divider(), kinds, alert)
  controls.push(kinds, alert)

  const setEnabled = (enabled: boolean) => {
    for (const control of controls) control.disabled = !enabled
  }
  setEnabled(false)
  return { element, setEnabled }
}
