// The selection toolbar: a floating bar over selected text with Turn into, bold, italic,
// strikethrough, inline code, and link (which opens the Ctrl+K link box). It is a Milkdown
// tooltip plugin registered inside `makeEditor`, so the fixture suite loads it. It adds no node
// or mark, so it cannot change the serialization. The fixed toolbar stays beside it.

import type { CmdKey } from '@milkdown/kit/core'
import type { Ctx } from '@milkdown/kit/ctx'
import { commandsCtx, editorViewCtx } from '@milkdown/kit/core'
import { TooltipProvider, tooltipFactory } from '@milkdown/kit/plugin/tooltip'
import {
  linkSchema,
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
import type { Node } from '@milkdown/kit/prose/model'
import { liftListItem } from '@milkdown/kit/prose/schema-list'
import { TextSelection } from '@milkdown/kit/prose/state'
import type { EditorState } from '@milkdown/kit/prose/state'
import { liftTarget } from '@milkdown/kit/prose/transform'
import type { EditorView } from '@milkdown/kit/prose/view'
import { openLinkBox } from './linkbox'
import { BUTTONS, icon, makeTaskList } from './toolbar'

/** One entry of the Turn into menu. */
export interface TurnInto {
  id: string
  label: string
  run: (ctx: Ctx) => void
}

const run = (ctx: Ctx, key: CmdKey<any>, payload?: unknown): void => {
  ctx.get(commandsCtx).call(key, payload)
}

/** Containers Turn into takes the selection out of before it applies its target. */
const WRAPPERS = new Set(['list_item', 'bullet_list', 'ordered_list', 'blockquote', 'alert'])
const LISTS = new Set(['list_item', 'bullet_list', 'ordered_list'])

/** The innermost wrapper holding the whole selection, or null when it sits in none. */
function innermostWrapper(state: EditorState): Node | null {
  const { $from, $to } = state.selection
  for (let depth = $from.sharedDepth($to.pos); depth > 0; depth--) {
    const node = $from.node(depth)
    if (WRAPPERS.has(node.type.name)) return node
  }
  return null
}

/** The list directly holding the selection's list item, or null outside a list. */
function innermostList(state: EditorState): Node | null {
  const { $from, $to } = state.selection
  for (let depth = $from.sharedDepth($to.pos); depth > 0; depth--) {
    const node = $from.node(depth)
    if (node.type.name === 'bullet_list' || node.type.name === 'ordered_list') return node
    if (WRAPPERS.has(node.type.name) && !LISTS.has(node.type.name)) return null
  }
  return null
}

/** Lift the selected blocks out of every list and quote around them, innermost first, so the
 *  target applies to plain blocks: wrap and setBlockType commands are no-ops (or nest) inside a
 *  list item or a quote. */
function liftOut(ctx: Ctx): void {
  const view = ctx.get(editorViewCtx)
  for (let wrapper = innermostWrapper(view.state); wrapper; wrapper = innermostWrapper(view.state)) {
    const { state } = view
    if (LISTS.has(wrapper.type.name)) {
      const item = state.schema.nodes['list_item']
      if (!item || !liftListItem(item)(state, view.dispatch)) return
      continue
    }
    const { $from, $to } = state.selection
    const range = $from.blockRange($to, (node) => node === wrapper)
    const target = range && liftTarget(range)
    if (range == null || target == null) return
    view.dispatch(state.tr.lift(range, target))
  }
}

/** Clear the task checkbox of every list item in the selection. */
function clearTasks(ctx: Ctx): void {
  const view = ctx.get(editorViewCtx)
  const { state } = view
  const tr = state.tr
  state.doc.nodesBetween(state.selection.from, state.selection.to, (node, pos) => {
    if (node.type.name === 'list_item' && node.attrs['checked'] != null) {
      tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: null })
    }
  })
  if (tr.docChanged) view.dispatch(tr)
}

const listIs = (ctx: Ctx, name: string): boolean =>
  innermostList(ctx.get(editorViewCtx).state)?.type.name === name

/** Lift out of lists and quotes, then run `command`. Its key is read at run time: Milkdown sets
 *  a command's `key` only when the editor loads the plugin. */
const lifted = (command: { key: CmdKey<any> }, payload?: unknown) => (ctx: Ctx) => {
  liftOut(ctx)
  run(ctx, command.key, payload)
}

export const TURN_INTO: TurnInto[] = [
  { id: 'text', label: 'Text', run: lifted(turnIntoTextCommand) },
  { id: 'h1', label: 'Heading 1', run: lifted(wrapInHeadingCommand, 1) },
  { id: 'h2', label: 'Heading 2', run: lifted(wrapInHeadingCommand, 2) },
  { id: 'h3', label: 'Heading 3', run: lifted(wrapInHeadingCommand, 3) },
  {
    id: 'bullet-list',
    label: 'Bulleted list',
    // Already a bullet list: only a task checkbox goes, so the list is not split around the item.
    run: (ctx) => (listIs(ctx, 'bullet_list') ? clearTasks(ctx) : lifted(wrapInBulletListCommand)(ctx)),
  },
  {
    id: 'ordered-list',
    label: 'Numbered list',
    run: (ctx) => {
      if (!listIs(ctx, 'ordered_list')) lifted(wrapInOrderedListCommand)(ctx)
    },
  },
  {
    id: 'task-list',
    label: 'Task list',
    run: (ctx) => {
      if (!listIs(ctx, 'bullet_list')) liftOut(ctx)
      makeTaskList(ctx)
    },
  },
  { id: 'quote', label: 'Quote', run: lifted(wrapInBlockquoteCommand) },
]

/** Text is selected in a block that takes inline formatting (not a code block). */
export function shouldShowSelectionToolbar(state: EditorState): boolean {
  const { selection } = state
  if (!(selection instanceof TextSelection) || selection.empty) return false
  if (!state.doc.textBetween(selection.from, selection.to).length) return false
  return !selection.$from.parent.type.spec.code
}

const selectionTooltip = tooltipFactory('RIKI_SELECTION')

/** Inline controls, in order; their icons come from the fixed toolbar's table. */
const INLINE = [
  { id: 'bold', title: 'Bold', command: toggleStrongCommand },
  { id: 'italic', title: 'Italic', command: toggleEmphasisCommand },
  { id: 'strike', title: 'Strikethrough', command: toggleStrikethroughCommand },
  { id: 'code', title: 'Inline code', command: toggleInlineCodeCommand },
] as const

function iconFor(id: string): SVGSVGElement {
  const button = BUTTONS.find((b) => b.id === id)
  if (!button) throw new Error(`toolbar has no button ${id}`)
  return icon(button.icon)
}

/** Controls keep the editor's selection: a mousedown on them never takes focus. */
function control(id: string, title: string): HTMLButtonElement {
  const el = document.createElement('button')
  el.type = 'button'
  el.dataset['control'] = `selection-${id}`
  el.title = title
  el.setAttribute('aria-label', title)
  el.addEventListener('mousedown', (event) => event.preventDefault())
  return el
}

function buildBar(ctx: Ctx, sourceFile: string, hide: () => void): HTMLElement {
  const bar = document.createElement('div')
  bar.className = 'riki-toolbar riki-selection-toolbar'
  bar.setAttribute('role', 'toolbar')
  bar.setAttribute('aria-label', 'Format selection')
  const view = (): EditorView => ctx.get(editorViewCtx)

  const turn = control('turn-into', 'Turn into')
  turn.classList.add('riki-toolbar-text')
  turn.append('Turn into')
  turn.setAttribute('aria-haspopup', 'menu')
  turn.setAttribute('aria-expanded', 'false')
  const menu = document.createElement('div')
  menu.className = 'riki-selection-menu'
  menu.setAttribute('role', 'menu')
  menu.hidden = true
  const setMenu = (open: boolean): void => {
    menu.hidden = !open
    turn.setAttribute('aria-expanded', String(open))
  }
  for (const item of TURN_INTO) {
    const entry = control(`turn-${item.id}`, item.label)
    entry.setAttribute('role', 'menuitem')
    entry.textContent = item.label
    entry.addEventListener('click', () => {
      setMenu(false)
      item.run(ctx)
      view().focus()
    })
    menu.append(entry)
  }
  turn.addEventListener('click', () => setMenu(Boolean(menu.hidden)))
  bar.append(turn, menu)

  for (const item of INLINE) {
    const el = control(item.id, item.title)
    el.append(iconFor(item.id))
    el.addEventListener('click', () => {
      run(ctx, item.command.key)
      view().focus()
    })
    bar.append(el)
  }

  const link = control('link', 'Link (Ctrl+K)')
  link.append(iconFor('link'))
  link.addEventListener('click', () => {
    const editorView = view()
    const type = linkSchema.type(ctx)
    hide()
    if (!editorView.state.selection.$from.parent.type.allowsMarkType(type)) return
    openLinkBox({ view: editorView, link: type, sourceFile })
  })
  bar.append(link)
  return bar
}

/** Point the tooltip plugin's spec at the toolbar view. `sourceFile` is the file being edited
 *  (the link box writes hrefs relative to it). */
export const configureSelectionToolbar = (sourceFile: string) => (ctx: Ctx) => {
  ctx.set(selectionTooltip.key, {
    view: () => {
      let provider: TooltipProvider | undefined
      const bar = buildBar(ctx, sourceFile, () => provider?.hide())
      provider = new TooltipProvider({
        content: bar,
        debounce: 20,
        offset: 8,
        shouldShow: (view) =>
          view.editable && (view.hasFocus() || bar.contains(document.activeElement)) && shouldShowSelectionToolbar(view.state),
      })
      provider.onHide = () => {
        const menu = bar.querySelector<HTMLElement>('.riki-selection-menu')
        if (menu) menu.hidden = true
        bar.querySelector('[data-control="turn-into"]')?.setAttribute('aria-expanded', 'false')
      }
      return {
        update: provider.update,
        destroy: () => {
          provider?.destroy()
          bar.remove()
        },
      }
    },
  })
}

export const selectionToolbar = selectionTooltip
