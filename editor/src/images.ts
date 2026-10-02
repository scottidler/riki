// Display-only image URLs in edit mode. The read side (core/src/render.rs `rewrite_image`)
// resolves a relative image against the source file's directory and serves it from
// /_riki/raw/<path>. The editor must show the same image, but the document keeps the author's
// `src` untouched so serialization stays byte-identical: only the node view's <img> is rewritten.

import type { Node } from '@milkdown/kit/prose/model'
import type { NodeViewConstructor } from '@milkdown/kit/prose/view'
import { imageSchema } from '@milkdown/kit/preset/commonmark'
import { $view } from '@milkdown/kit/utils'

/** Where image blobs are served from; matches `RAW_PREFIX` in core/src/render.rs. */
export const RAW_PREFIX = '/_riki/raw/'

/** Split a relative URL into path and `?query`/`#fragment` tail; `null` for anything that is not
 *  relative: empty, `#frag`, `/abs`, `//host`, or a URL with a scheme. */
function relative(url: string): { path: string; tail: string } | null {
  if (url === '' || url.startsWith('#') || url.startsWith('/')) return null
  const split = url.search(/[?#]/)
  const at = split === -1 ? url.length : split
  const path = url.slice(0, at)
  if ((path.split('/')[0] ?? '').includes(':')) return null
  return { path, tail: url.slice(at) }
}

/** Resolve `target` against the directory of `sourceFile`; `null` when `..` climbs out of the
 *  repo root. */
export function resolve(sourceFile: string, target: string): string | null {
  const slash = sourceFile.lastIndexOf('/')
  const segments = slash === -1 ? [] : sourceFile.slice(0, slash).split('/')
  for (const segment of target.split('/')) {
    if (segment === '' || segment === '.') continue
    if (segment === '..') {
      if (segments.pop() === undefined) return null
      continue
    }
    segments.push(segment)
  }
  return segments.join('/')
}

/** The URL the browser loads for image `src` in `sourceFile`. Non-relative and unresolvable
 *  URLs pass through unchanged, as on the read side. */
export function rawImageUrl(sourceFile: string, src: string): string {
  const rel = relative(src)
  if (!rel) return src
  const resolved = resolve(sourceFile, rel.path)
  if (resolved === null) return src
  return `${RAW_PREFIX}${resolved}${rel.tail}`
}

/** An <img> node view showing `node`'s image at its resolved URL. */
export function imageNodeView(sourceFile: string): NodeViewConstructor {
  return (initial: Node) => {
    const dom = document.createElement('img')
    const show = (node: Node) => {
      const attrs = node.attrs as { src: string; alt: string; title: string }
      dom.setAttribute('src', rawImageUrl(sourceFile, attrs.src))
      dom.setAttribute('alt', attrs.alt)
      if (attrs.title) dom.setAttribute('title', attrs.title)
      else dom.removeAttribute('title')
    }
    show(initial)
    return {
      dom,
      update: (node: Node) => {
        if (node.type !== initial.type) return false
        show(node)
        return true
      },
    }
  }
}

/** The Milkdown plugin installing [`imageNodeView`] for the commonmark image node. */
export function imageView(sourceFile: string) {
  return $view(imageSchema.node, () => imageNodeView(sourceFile))
}
