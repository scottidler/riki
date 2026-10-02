// Task lists without syntax: a command that turns the selected list items into tasks, and a
// click handler that ticks a task's box (the box is the `li`'s own padding, drawn in CSS).

import type { Node as ProseNode, NodeType } from '@milkdown/kit/prose/model'
import { Plugin, PluginKey } from '@milkdown/kit/prose/state'
import type { Command, EditorState, Transaction } from '@milkdown/kit/prose/state'
import { listItemSchema } from '@milkdown/kit/preset/commonmark'
import { $command, $prose } from '@milkdown/kit/utils'

/** Positions of every list item touching the selection, innermost included. */
export function listItemsInSelection(state: EditorState, type: NodeType): number[] {
  const positions = new Set<number>()
  const { from, to, $from } = state.selection
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type === type) {
      positions.add($from.before(depth))
      break
    }
  }
  state.doc.nodesBetween(from, to, (node, pos) => {
    if (node.type === type) positions.add(pos)
  })
  return [...positions].sort((a, b) => a - b)
}

/** Mark the selected list items as unchecked tasks. False when the selection is in no list. */
export function makeTasks(type: NodeType): Command {
  return (state, dispatch) => {
    const items = listItemsInSelection(state, type).filter((pos) => state.doc.nodeAt(pos)?.attrs['checked'] == null)
    if (!items.length) return listItemsInSelection(state, type).length > 0
    if (dispatch) {
      const tr = state.tr
      for (const pos of items) {
        const node = state.doc.nodeAt(pos)
        if (node) tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: false })
      }
      dispatch(tr)
    }
    return true
  }
}

export const makeTasksCommand = $command('RikiMakeTasks', (ctx) => () => makeTasks(listItemSchema.type(ctx)))

/** Flip a task's checked state; true when `node` at `pos` is a task. */
export function toggleTask(state: EditorState, node: ProseNode, pos: number, dispatch?: (tr: Transaction) => void): boolean {
  if (node.attrs['checked'] == null) return false
  dispatch?.(state.tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: !node.attrs['checked'] }))
  return true
}

export const taskClickPlugin = $prose((ctx) => {
  const type = listItemSchema.type(ctx)
  return new Plugin({
    key: new PluginKey('rikiTaskClick'),
    props: {
      handleClickOn(view, _pos, node, nodePos, event, direct) {
        if (!direct || node.type !== type || !view.editable) return false
        // Only a click on the item's own box (its padding), not on the text inside it.
        if (event.target instanceof HTMLElement && event.target.tagName !== 'LI') return false
        return toggleTask(view.state, node, nodePos, view.dispatch)
      },
    },
  })
})

export const taskPlugins = [makeTasksCommand, taskClickPlugin]
