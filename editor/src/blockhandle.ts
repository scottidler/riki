// The block handle: on hover, the editor's left gutter shows + (add a block below) and ⋮⋮ (drag
// to reorder; click for Turn into / Duplicate / Delete). It is Milkdown's block plugin, so the
// drag is the plugin's own native HTML drag of the hovered node. Registered inside `makeEditor`
// so the fixture suite loads it; it adds no node, mark or schema change.

import type { Ctx } from '@milkdown/kit/ctx'
import { editorViewCtx } from '@milkdown/kit/core'
import { BlockProvider, block } from '@milkdown/kit/plugin/block'
import type { Node } from '@milkdown/kit/prose/model'
import { Selection, TextSelection } from '@milkdown/kit/prose/state'
import type { EditorState } from '@milkdown/kit/prose/state'
import type { EditorView } from '@milkdown/kit/prose/view'
import { TURN_INTO } from './selectiontoolbar'
import type { TurnInto } from './selectiontoolbar'
import { icon } from './toolbar'

/** A block the handle acts on: the node and the position just before it. */
export interface Block {
  pos: number
  node: Node
}

/** The block is still at `pos`, unchanged: an action on a stale block does nothing. */
export function blockIsCurrent(state: EditorState, target: Block): boolean {
  return state.doc.nodeAt(target.pos) === target.node
}

/** A text range spanning the block's own text, or null when it has none (a divider, a table). */
function textRangeOf(state: EditorState, target: Block): Selection | null {
  if (target.node.type.name === 'table') return null
  const end = target.pos + target.node.nodeSize
  const range = TextSelection.between(state.doc.resolve(target.pos), state.doc.resolve(end))
  if (!range.$from.parent.inlineContent || range.from < target.pos || range.to > end) return null
  return range
}

/** Turn into applies: the block holds text the Turn into commands can retype. */
export function canTurnInto(state: EditorState, target: Block): boolean {
  return blockIsCurrent(state, target) && textRangeOf(state, target) !== null
}

/** Run a Turn into entry (the selection toolbar's) over the whole block, then leave a cursor. */
export function turnBlockInto(ctx: Ctx, target: Block, item: TurnInto): boolean {
  const view = ctx.get(editorViewCtx)
  if (!blockIsCurrent(view.state, target)) return false
  const range = textRangeOf(view.state, target)
  if (!range) return false
  view.dispatch(view.state.tr.setSelection(range))
  item.run(ctx)
  const { state } = view
  view.dispatch(state.tr.setSelection(Selection.near(state.doc.resolve(state.selection.from))))
  return true
}

/** Insert a copy of the block right after it. */
export function duplicateBlock(view: EditorView, target: Block): boolean {
  if (!blockIsCurrent(view.state, target)) return false
  view.dispatch(view.state.tr.insert(target.pos + target.node.nodeSize, target.node))
  return true
}

/** Remove the block. A block that is its parent's only child takes the parent with it (the
 *  only item of a list removes the list), so no empty container is left behind. */
export function deleteBlock(view: EditorView, target: Block): boolean {
  const { state } = view
  if (!blockIsCurrent(state, target)) return false
  const $pos = state.doc.resolve(target.pos)
  let from = target.pos
  let to = target.pos + target.node.nodeSize
  for (let depth = $pos.depth; depth > 0 && $pos.node(depth).childCount === 1; depth--) {
    from = $pos.before(depth)
    to = $pos.after(depth)
  }
  view.dispatch(state.tr.delete(from, to).scrollIntoView())
  return true
}

/** The gutter +: a cursor in an empty paragraph below the block (the block itself when it is an
 *  empty paragraph already). */
export function addBlockBelow(view: EditorView, target: Block): boolean {
  const { state } = view
  if (!blockIsCurrent(state, target)) return false
  const paragraph = state.schema.nodes['paragraph']
  if (!paragraph) throw new Error('schema has no paragraph node')
  const { node, pos } = target
  if (node.type === paragraph && node.content.size === 0) {
    view.dispatch(state.tr.setSelection(TextSelection.create(state.doc, pos + 1)))
    return true
  }
  const at = pos + node.nodeSize
  const $at = state.doc.resolve(at)
  if (!$at.parent.canReplaceWith($at.index(), $at.index(), paragraph)) return false
  const tr = state.tr.insert(at, paragraph.create())
  view.dispatch(tr.setSelection(TextSelection.create(tr.doc, at + 1)).scrollIntoView())
  return true
}

/** The handle's menu, in order. */
export const BLOCK_MENU = ['turn-into', 'duplicate', 'delete'] as const

const ADD_ICON = 'M8 3.25v9.5M3.25 8h9.5'
const DRAG_ICON = 'M6 3.5h.01M10 3.5h.01M6 8h.01M10 8h.01M6 12.5h.01M10 12.5h.01'

function button(control: string, title: string): HTMLButtonElement {
  const el = document.createElement('button')
  el.type = 'button'
  el.dataset['control'] = control
  el.title = title
  el.setAttribute('aria-label', title)
  return el
}

function menuEntry(control: string, label: string): HTMLButtonElement {
  const el = button(control, label)
  el.setAttribute('role', 'menuitem')
  el.textContent = label
  // The editor keeps focus and its selection while the menu is used.
  el.addEventListener('mousedown', (event) => event.preventDefault())
  return el
}

/** The plugin view: the gutter handle (placed by the block provider) and its menu. */
class BlockHandleView {
  readonly #ctx: Ctx
  readonly #provider: BlockProvider
  readonly #handle: HTMLElement
  readonly #menu: HTMLElement
  readonly #turnMenu: HTMLElement
  readonly #turnButton: HTMLButtonElement
  #target: Block | null = null

  constructor(ctx: Ctx) {
    this.#ctx = ctx
    const handle = document.createElement('div')
    handle.className = 'riki-block-handle'
    const add = button('block-add', 'Add a block below')
    add.append(icon(ADD_ICON))
    const drag = button('block-handle', 'Drag to move, click for options')
    drag.append(icon(DRAG_ICON))
    drag.setAttribute('aria-haspopup', 'menu')
    drag.setAttribute('aria-expanded', 'false')
    handle.append(add, drag)
    this.#handle = handle

    const menu = document.createElement('div')
    menu.className = 'riki-block-menu'
    menu.setAttribute('role', 'menu')
    menu.hidden = true
    const turn = menuEntry('block-turn-into', 'Turn into')
    turn.setAttribute('aria-haspopup', 'menu')
    const turnMenu = document.createElement('div')
    turnMenu.className = 'riki-block-submenu'
    turnMenu.setAttribute('role', 'menu')
    turnMenu.hidden = true
    for (const item of TURN_INTO) {
      const entry = menuEntry(`block-turn-${item.id}`, item.label)
      entry.addEventListener('click', () => this.#act((target) => turnBlockInto(this.#ctx, target, item)))
      turnMenu.append(entry)
    }
    turn.addEventListener('click', () => {
      turnMenu.hidden = !turnMenu.hidden
    })
    const duplicate = menuEntry('block-duplicate', 'Duplicate')
    duplicate.addEventListener('click', () => this.#act((target) => duplicateBlock(this.#view(), target)))
    const remove = menuEntry('block-delete', 'Delete')
    remove.addEventListener('click', () => this.#act((target) => deleteBlock(this.#view(), target)))
    menu.append(turn, turnMenu, duplicate, remove)
    this.#menu = menu
    this.#turnMenu = turnMenu
    this.#turnButton = turn

    this.#provider = new BlockProvider({
      ctx,
      content: handle,
      getPlacement: () => 'left-start',
      getOffset: () => ({ mainAxis: 4 }),
    })

    add.addEventListener('click', () => {
      const target = this.#hovered()
      if (!target) return
      this.closeMenu()
      addBlockBelow(this.#view(), target)
      this.#view().focus()
    })
    drag.addEventListener('click', () => {
      if (!this.#menu.hidden) {
        this.closeMenu()
        return
      }
      const target = this.#hovered()
      if (target) this.openMenu(target)
    })
    document.addEventListener('mousedown', this.#outside)
    document.addEventListener('keydown', this.#escape)
    this.#provider.update()
  }

  #view(): EditorView {
    return this.#ctx.get(editorViewCtx)
  }

  #hovered(): Block | null {
    const active = this.#provider.active
    return active ? { pos: active.$pos.pos, node: active.node } : null
  }

  /** Open the menu for `target`, under the handle. */
  openMenu(target: Block): void {
    this.#target = target
    const root = this.#handle.parentElement ?? this.#view().dom.parentElement
    if (!root) throw new Error('editor view has no root element')
    if (this.#menu.parentElement !== root) root.append(this.#menu)
    this.#menu.style.left = this.#handle.style.left
    this.#menu.style.top = `${this.#handle.offsetTop + this.#handle.offsetHeight + 4}px`
    this.#turnButton.disabled = !canTurnInto(this.#view().state, target)
    this.#turnMenu.hidden = true
    this.#menu.hidden = false
    this.#handle.querySelector('[data-control="block-handle"]')?.setAttribute('aria-expanded', 'true')
  }

  closeMenu(): void {
    this.#target = null
    this.#menu.hidden = true
    this.#turnMenu.hidden = true
    this.#handle.querySelector('[data-control="block-handle"]')?.setAttribute('aria-expanded', 'false')
  }

  #act(action: (target: Block) => boolean): void {
    const target = this.#target
    this.closeMenu()
    this.#provider.hide()
    if (!target) return
    action(target)
    this.#view().focus()
  }

  readonly #outside = (event: MouseEvent): void => {
    if (this.#menu.hidden) return
    const node = event.target as globalThis.Node | null
    if (node && (this.#menu.contains(node) || this.#handle.contains(node))) return
    this.closeMenu()
  }

  readonly #escape = (event: KeyboardEvent): void => {
    if (event.key === 'Escape' && !this.#menu.hidden) this.closeMenu()
  }

  update = (view: EditorView, prev?: EditorState): void => {
    this.#provider.update()
    if (!view.editable) {
      this.#provider.hide()
      this.closeMenu()
      return
    }
    // The menu's block position is stale once the document changes under it.
    if (prev && !prev.doc.eq(view.state.doc) && this.#target) this.closeMenu()
  }

  destroy = (): void => {
    document.removeEventListener('mousedown', this.#outside)
    document.removeEventListener('keydown', this.#escape)
    this.#provider.destroy()
    this.#menu.remove()
  }
}

/** Point the block plugin's spec at the handle view. */
export const configureBlockHandle = (ctx: Ctx): void => {
  ctx.set(block.key, { view: () => new BlockHandleView(ctx) })
}

export const blockHandle = block
