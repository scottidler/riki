// GitHub alerts (`> [!NOTE]`) as their own node: a remark transform that turns a matching
// blockquote into an `alert` mdast node, the ProseMirror schema, the remark-stringify handler
// that writes the marker line back, and the commands the toolbar uses.
//
// Registered AFTER the presets: ProseMirror fills an empty doc with the schema's first `block`
// node, and an alert registered first would become that fill type (Phase 0a).

import type { Blockquote, BlockContent, DefinitionContent, Paragraph, Parents, Root } from 'mdast'
import type { Handle, State, Info } from 'mdast-util-to-markdown'
import type { VFile } from 'vfile'
import { InputRule } from '@milkdown/kit/prose/inputrules'
import { $command, $inputRule, $nodeSchema, $remark } from '@milkdown/kit/utils'
import type { Node as ProseNode, NodeType } from '@milkdown/kit/prose/model'
import { findWrapping } from '@milkdown/kit/prose/transform'
import { TextSelection } from '@milkdown/kit/prose/state'
import type { Command } from '@milkdown/kit/prose/state'
import { visit } from 'unist-util-visit'

export const ALERT_KINDS = ['NOTE', 'TIP', 'IMPORTANT', 'WARNING', 'CAUTION'] as const
export type AlertKind = (typeof ALERT_KINDS)[number]

const MARKER = /^\[!(note|tip|important|warning|caution)\]/i
/** comrak's scanner: the line reads `> [!type]` (one `>`, one space) at its first non-space. */
const SOURCE_LINE = /^([ \t]*> )(\[![a-z]+\])(.*)$/i

/** The mdast node the transform produces and the stringify handler consumes. */
export interface AlertNode {
  type: 'alert'
  kind: AlertKind
  /** The marker as written (`[!note]` keeps its case). */
  marker: string
  /** Plain-text title after the marker; empty for none. */
  title: string
  /** The marker sat in its own paragraph (`>` blank line after it). */
  separated: boolean
  children: Array<BlockContent | DefinitionContent>
}

declare module 'mdast' {
  interface RootContentMap {
    alert: AlertNode
  }
  interface BlockContentMap {
    alert: AlertNode
  }
}

/** The icon in each alert's title, the same 16x16 paths the server renders
 *  (`core/src/render.rs:alert_icon_path`), so an alert looks the same read and edited. */
export const ALERT_ICON_PATHS: Record<AlertKind, string> = {
  NOTE: 'M8 14.25a6.25 6.25 0 1 0 0-12.5 6.25 6.25 0 0 0 0 12.5ZM8 7.25v4M8 4.75v.01',
  TIP: 'M6 12.25h4M6.75 14.5h2.5M5.6 10.4a4.25 4.25 0 1 1 4.8 0c-.4.3-.65.75-.65 1.25v.6h-3.5v-.6c0-.5-.25-.95-.65-1.25Z',
  IMPORTANT:
    'M2.25 3.25c0-.55.45-1 1-1h9.5c.55 0 1 .45 1 1v7c0 .55-.45 1-1 1H8l-3.25 2.5v-2.5h-1.5c-.55 0-1-.45-1-1ZM8 4.75v2.75M8 9.5v.01',
  WARNING: 'M7.13 2.5a1 1 0 0 1 1.74 0l5.4 9.5a1 1 0 0 1-.87 1.5H2.6a1 1 0 0 1-.87-1.5ZM8 6.25v3M8 11.25v.01',
  CAUTION: 'M5.4 1.75h5.2l3.65 3.65v5.2l-3.65 3.65H5.4L1.75 10.6V5.4ZM8 4.75v3.75M8 10.75v.01',
}

/** The title an alert shows: its own title text, else the kind in comrak's casing (`Warning`). */
export function alertTitle(kind: AlertKind, title: string): string {
  const own = title.trim()
  return own || kind[0] + kind.slice(1).toLowerCase()
}

const SVG_NS = 'http://www.w3.org/2000/svg'

export function isAlertKind(value: string): value is AlertKind {
  return (ALERT_KINDS as readonly string[]).includes(value)
}

/** Rewrite blockquotes whose source line is an alert marker into `alert` nodes, in place. */
export function transformAlerts(tree: Root, source: string): void {
  visit(tree, 'blockquote', (node: Blockquote, index, parent: Parents | undefined) => {
    if (index === undefined || !parent) return
    const alert = toAlert(node, source)
    if (alert) parent.children.splice(index, 1, alert as never)
  })
}

function toAlert(node: Blockquote, source: string): AlertNode | null {
  const para = node.children[0]
  if (!para || para.type !== 'paragraph' || !node.position) return null
  const text = para.children[0]
  if (!text || text.type !== 'text') return null
  const match = MARKER.exec(text.value)
  if (!match?.[1]) return null
  const offset = node.position.start.offset ?? 0
  const lineStart = source.lastIndexOf('\n', offset - 1) + 1
  const lineEnd = source.indexOf('\n', offset)
  const line = source.slice(lineStart, lineEnd === -1 ? source.length : lineEnd)
  const lineMatch = SOURCE_LINE.exec(line)
  if (!lineMatch?.[2] || lineMatch[2].toLowerCase() !== match[0].toLowerCase()) return null
  const marker = lineMatch[2]
  const title = lineMatch[3] ?? ''
  const newline = text.value.indexOf('\n')
  const firstLine = newline === -1 ? text.value : text.value.slice(0, newline)
  // A title carrying inline markup is split across nodes; leave it a plain blockquote.
  if (firstLine !== marker + title) return null
  if (!stripMarker(para, text.value, newline)) return null
  const separated = para.children.length === 0
  const kind = match[1].toUpperCase()
  if (!isAlertKind(kind)) return null
  return {
    type: 'alert',
    kind,
    marker,
    title,
    separated,
    children: separated ? node.children.slice(1) : node.children,
  }
}

/** Remove the marker line from the first paragraph. By the time this runs the preset's
 *  `remarkLineBreak` has turned the soft break after the marker into a `break` node. */
function stripMarker(para: Paragraph, value: string, newline: number): boolean {
  const first = para.children[0]
  if (!first || first.type !== 'text') return false
  if (newline === -1) {
    const next = para.children[1]
    if (next && next.type !== 'break') return false
    para.children.splice(0, next ? 2 : 1)
    return true
  }
  first.value = value.slice(newline + 1)
  if (first.value === '') para.children.shift()
  return true
}

export const remarkAlert = $remark('remarkAlert', () => () => (tree: Root, file: VFile) => {
  transformAlerts(tree, String(file.value ?? ''))
})

export const alertSchema = $nodeSchema('alert', () => ({
  content: 'block*',
  group: 'block',
  defining: true,
  attrs: {
    kind: { default: 'NOTE' },
    marker: { default: '[!NOTE]' },
    title: { default: '' },
    separated: { default: false },
  },
  parseDOM: [
    {
      tag: 'div[data-alert]',
      contentElement: (dom: Node) =>
        (dom instanceof HTMLElement ? dom.querySelector<HTMLElement>(':scope > .markdown-alert-body') : null) ??
        (dom as HTMLElement),
      getAttrs: (dom: HTMLElement) => {
        const kind = dom.dataset['kind'] ?? 'NOTE'
        return { kind, marker: `[!${kind}]` }
      },
    },
  ],
  // The server's alert markup (title row with icon, then the body), so the shared stylesheet
  // styles both the same. The title is not part of the document: the `title` attr is.
  toDOM: (node: ProseNode) => {
    const raw = String(node.attrs['kind'])
    const kind: AlertKind = isAlertKind(raw) ? raw : 'NOTE'
    return [
      'div',
      { 'data-alert': '', 'data-kind': kind, class: `markdown-alert markdown-alert-${kind.toLowerCase()}` },
      [
        'p',
        { class: 'markdown-alert-title', contenteditable: 'false' },
        [
          `${SVG_NS} svg`,
          { class: 'riki-alert-icon', viewBox: '0 0 16 16', width: '16', height: '16', 'aria-hidden': 'true' },
          [`${SVG_NS} path`, { d: ALERT_ICON_PATHS[kind] }],
        ],
        alertTitle(kind, String(node.attrs['title'] ?? '')),
      ],
      ['div', { class: 'markdown-alert-body' }, 0],
    ]
  },
  parseMarkdown: {
    match: ({ type }) => type === 'alert',
    runner: (state, node, type) => {
      const alert = node as unknown as AlertNode
      state
        .openNode(type, { kind: alert.kind, marker: alert.marker, title: alert.title, separated: alert.separated })
        .next(alert.children as never)
        .closeNode()
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === 'alert',
    runner: (state, node) => {
      state.openNode('alert', undefined, { ...node.attrs }).next(node.content).closeNode()
    },
  },
}))

/** remark-stringify handler: the blockquote handler's shape with the marker line in front. */
export const alertHandler: Handle = (node: AlertNode, _parent, state: State, info: Info) => {
  const exit = state.enter('blockquote')
  const tracker = state.createTracker(info)
  tracker.move('> ')
  tracker.shift(2)
  const head = node.marker + node.title
  // An alert with no body reaches here without `children` at all.
  const inner = node.children?.length ? state.containerFlow(node as never, tracker.current()) : ''
  let value = head
  if (inner) value += (node.separated ? '\n\n' : '\n') + inner
  exit()
  return state.indentLines(value, (line, _line, blank) => '>' + (blank ? '' : ' ') + line)
}

/** The alert containing the selection's start, with its position, or null. */
export function alertAround(doc: ProseNode, pos: number, type: NodeType): { node: ProseNode; pos: number } | null {
  const $pos = doc.resolve(pos)
  for (let depth = $pos.depth; depth > 0; depth--) {
    const node = $pos.node(depth)
    if (node.type === type) return { node, pos: $pos.before(depth) }
  }
  return null
}

/** Wrap the selected blocks in an alert of `kind`; inside an alert already, change its kind. */
export function setAlert(type: NodeType, kind: AlertKind): Command {
  return (state, dispatch) => {
    const marker = `[!${kind}]`
    const around = alertAround(state.doc, state.selection.from, type)
    if (around) {
      if (dispatch) {
        const attrs = { ...around.node.attrs, kind, marker }
        dispatch(state.tr.setNodeMarkup(around.pos, undefined, attrs).scrollIntoView())
      }
      return true
    }
    const range = state.selection.$from.blockRange(state.selection.$to)
    if (!range) return false
    const wrapping = findWrapping(range, type, { kind, marker, title: '', separated: false })
    if (!wrapping) return false
    if (dispatch) dispatch(state.tr.wrap(range, wrapping).scrollIntoView())
    return true
  }
}

export const setAlertCommand = $command('SetAlert', (ctx) => (kind?: AlertKind) => setAlert(alertSchema.type(ctx), kind ?? 'NOTE'))

/** `[!TIP] ` typed at the start of a quote: the quote becomes that alert (the `> ` rule has
 *  already wrapped the line, so this is `> [!TIP] ` typed out). The marker text is removed. */
export const alertInputRule = $inputRule(
  (ctx) =>
    new InputRule(/^\[!(note|tip|important|warning|caution)\]\s$/i, (state, match, start, end) => {
      const kind = match[1]?.toUpperCase()
      if (!kind || !isAlertKind(kind)) return null
      const $from = state.doc.resolve(start)
      const quote = $from.depth >= 2 ? $from.node($from.depth - 1) : null
      if (!quote || quote.type.name !== 'blockquote' || $from.index($from.depth - 1) !== 0) return null
      const quoteStart = $from.before($from.depth - 1)
      const tr = state.tr.delete(start, end)
      const alert = alertSchema.type(ctx).create(
        { kind, marker: `[!${kind}]`, title: '', separated: false },
        tr.doc.nodeAt(quoteStart)?.content,
      )
      tr.replaceWith(quoteStart, quoteStart + tr.doc.nodeAt(quoteStart)!.nodeSize, alert)
      return tr.setSelection(TextSelection.near(tr.doc.resolve(start))).scrollIntoView()
    }),
)

export const alertPlugins = [remarkAlert, alertSchema, setAlertCommand, alertInputRule].flat()
