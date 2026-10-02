// The slash menu: typing `/` at the start of an empty paragraph (or the gutter +) opens a
// filterable list of blocks. Every entry reuses an existing block command: the selection
// toolbar's Turn into entries, the code block and table inserts, the divider, and the alert
// command (so Warning is the same GitHub `> [!WARNING]` the fixed toolbar writes). Registered
// inside `makeEditor`; it adds no node, mark or schema change.

import type { CmdKey } from '@milkdown/kit/core'
import type { Ctx } from '@milkdown/kit/ctx'
import { commandsCtx, editorViewCtx } from '@milkdown/kit/core'
import { SlashProvider, slashFactory } from '@milkdown/kit/plugin/slash'
import { createCodeBlockCommand, insertHrCommand } from '@milkdown/kit/preset/commonmark'
import { TextSelection } from '@milkdown/kit/prose/state'
import type { EditorState } from '@milkdown/kit/prose/state'
import type { EditorView } from '@milkdown/kit/prose/view'
import { ALERT_KINDS, setAlertCommand } from './alert'
import type { AlertKind } from './alert'
import { TURN_INTO } from './selectiontoolbar'
import type { TurnInto } from './selectiontoolbar'
import { insertTableCommand } from './table'
import { NEW_TABLE } from './toolbar'

const slashPlugin = slashFactory('RIKI_SLASH')

const alertLabel = (kind: AlertKind): string => kind[0] + kind.slice(1).toLowerCase()
const call = (ctx: Ctx, key: CmdKey<any>, payload?: unknown): void => {
  ctx.get(commandsCtx).call(key, payload)
}

/** Every entry of the menu, in order: the Turn into entries, then the inserts, then alerts. */
export const SLASH_ITEMS: TurnInto[] = [
  ...TURN_INTO,
  { id: 'code-block', label: 'Code block', run: (ctx) => call(ctx, createCodeBlockCommand.key) },
  { id: 'table', label: 'Table', run: (ctx) => call(ctx, insertTableCommand.key, NEW_TABLE) },
  { id: 'divider', label: 'Divider', run: (ctx) => call(ctx, insertHrCommand.key) },
  ...ALERT_KINDS.map((kind) => ({
    id: kind.toLowerCase(),
    label: alertLabel(kind),
    run: (ctx: Ctx) => call(ctx, setAlertCommand.key, kind),
  })),
]

/** The entries whose label contains `filter`, case-insensitively; all of them for ''. */
export function filterSlashItems(filter: string): TurnInto[] {
  const needle = filter.trim().toLowerCase()
  return SLASH_ITEMS.filter((item) => item.label.toLowerCase().includes(needle))
}

const TYPED = /^\/(\S*)$/

/** The paragraph holding the cursor, as {start, end} of its content; null when the cursor is not
 *  collapsed in a paragraph outside a table. */
function cursorParagraph(state: EditorState): { start: number; end: number; text: string } | null {
  const { selection } = state
  if (!(selection instanceof TextSelection) || !selection.empty) return null
  const { $from } = selection
  if (!$from.parent.isTextblock || $from.parent.type.name !== 'paragraph') return null
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type.name.startsWith('table')) return null
  }
  return { start: $from.start(), end: $from.end(), text: $from.parent.textContent }
}

/** The text after the typed `/` when the menu applies to the cursor's paragraph (`''` for a
 *  forced-open empty paragraph), else null. */
export function slashFilter(state: EditorState, forced: boolean): string | null {
  const paragraph = cursorParagraph(state)
  if (!paragraph) return null
  const typed = TYPED.exec(paragraph.text)
  if (typed) return typed[1] ?? ''
  return forced && paragraph.text === '' ? '' : null
}

const controllers = new WeakMap<EditorView, SlashMenuView>()

/** Open the menu on the cursor's (empty) paragraph, as the block handle's + does. */
export function openSlashMenu(view: EditorView): boolean {
  const controller = controllers.get(view)
  if (!controller) throw new Error('the slash menu is not registered on this editor')
  return controller.open(view)
}

class SlashMenuView {
  readonly #ctx: Ctx
  readonly #provider: SlashProvider
  readonly #menu: HTMLElement
  readonly #buttons = new Map<string, HTMLButtonElement>()
  #forced = false
  #dismissedAt: number | null = null
  #visible: TurnInto[] = []
  #selected = 0
  #lastFilter: string | null = null

  constructor(ctx: Ctx, view: EditorView) {
    this.#ctx = ctx
    const menu = document.createElement('div')
    menu.className = 'riki-slash-menu'
    menu.setAttribute('role', 'listbox')
    menu.setAttribute('aria-label', 'Insert a block')
    menu.dataset['show'] = 'false'
    for (const item of SLASH_ITEMS) {
      const entry = document.createElement('button')
      entry.type = 'button'
      entry.dataset['control'] = `slash-${item.id}`
      entry.setAttribute('role', 'option')
      entry.textContent = item.label
      // The editor keeps focus and its cursor while the menu is used.
      entry.addEventListener('mousedown', (event) => event.preventDefault())
      entry.addEventListener('click', () => this.choose(item))
      this.#buttons.set(item.id, entry)
      menu.append(entry)
    }
    this.#menu = menu
    this.#provider = new SlashProvider({
      content: menu,
      debounce: 20,
      offset: 6,
      shouldShow: (v) => this.#filterFor(v) !== null,
    })
    controllers.set(view, this)
  }

  #view(): EditorView {
    return this.#ctx.get(editorViewCtx)
  }

  #filterFor(view: EditorView): string | null {
    if (!view.editable || this.#dismissedAt === view.state.selection.from) return null
    const filter = slashFilter(view.state, this.#forced)
    return filter !== null && filterSlashItems(filter).length > 0 ? filter : null
  }

  open(view: EditorView): boolean {
    if (!view.editable) return false
    if (slashFilter(view.state, true) === null) return false
    this.#forced = true
    this.#dismissedAt = null
    this.update(view)
    return true
  }

  get active(): boolean {
    return this.#menu.dataset['show'] === 'true'
  }

  #render(filter: string | null): void {
    if (filter === null) {
      this.#forced = false
      this.#visible = []
      this.#lastFilter = null
      this.#menu.dataset['show'] = 'false'
      return
    }
    if (filter !== this.#lastFilter) this.#selected = 0
    this.#lastFilter = filter
    this.#visible = filterSlashItems(filter)
    this.#menu.dataset['show'] = 'true'
    for (const [id, button] of this.#buttons) {
      const index = this.#visible.findIndex((item) => item.id === id)
      button.hidden = index === -1
      button.setAttribute('aria-selected', String(index === this.#selected))
      button.classList.toggle('is-selected', index === this.#selected)
    }
  }

  update = (view: EditorView, prev?: EditorState): void => {
    if (this.#dismissedAt !== view.state.selection.from) this.#dismissedAt = null
    if (!this.#menu.isConnected) (view.dom.parentElement ?? document.body).append(this.#menu)
    this.#render(this.#filterFor(view))
    this.#provider.update(view, prev)
    if (!this.active) this.#provider.hide()
  }

  /** Handle a key while the menu is open; true when the key was used. */
  handleKey(event: KeyboardEvent): boolean {
    if (!this.active) return false
    const count = this.#visible.length
    switch (event.key) {
      case 'ArrowDown':
        this.#selected = (this.#selected + 1) % count
        break
      case 'ArrowUp':
        this.#selected = (this.#selected + count - 1) % count
        break
      case 'Enter': {
        const item = this.#visible[this.#selected]
        if (item) this.choose(item)
        return true
      }
      case 'Escape':
        this.#dismissedAt = this.#view().state.selection.from
        break
      default:
        return false
    }
    this.update(this.#view())
    return true
  }

  /** Remove the typed `/filter`, then run the entry on the empty paragraph. */
  choose(item: TurnInto): void {
    const view = this.#view()
    const paragraph = cursorParagraph(view.state)
    if (!paragraph) return
    this.#render(null)
    this.#provider.hide()
    if (paragraph.end > paragraph.start) view.dispatch(view.state.tr.delete(paragraph.start, paragraph.end))
    item.run(this.#ctx)
    view.focus()
  }

  destroy = (): void => {
    this.#provider.destroy()
    this.#menu.remove()
  }
}

/** Point the slash plugin's spec at the menu view and its keys. */
export const configureSlashMenu = (ctx: Ctx): void => {
  ctx.set(slashPlugin.key, {
    view: (view: EditorView) => new SlashMenuView(ctx, view),
    props: {
      handleKeyDown: (view: EditorView, event: KeyboardEvent) => controllers.get(view)?.handleKey(event) ?? false,
    },
  })
}

export const slashMenu = slashPlugin
