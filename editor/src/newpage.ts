// What a "+" new page starts with. Shared by both bundles: the editor reads it from `?new=`.

/** The text a new page starts with: its title as an H1. Markdown punctuation in the title is
 *  escaped so it stays literal (`#` too: a trailing ` #` is an ATX closing sequence), and line
 *  breaks collapse to spaces. */
export function newPageBody(title: string): string {
  const text = title.replace(/\s+/g, ' ').trim().replace(/[\\`*_[\]<>~|&#]/g, (c) => `\\${c}`)
  return `# ${text}\n\n`
}

/** The starting text for the `?new=<title>` in `search`; null when there is no usable title. */
export function newPageBodyFromSearch(search: string): string | null {
  const title = new URLSearchParams(search).get('new')
  if (title === null || title.trim() === '') return null
  return newPageBody(title)
}

/** `href` without its `?new=` parameter, keeping every other parameter and the hash; null when
 *  there is no `new` parameter, so nothing needs replacing. */
export function withoutNewParam(href: string): string | null {
  const url = new URL(href)
  if (!url.searchParams.has('new')) return null
  url.searchParams.delete('new')
  return `${url.pathname}${url.search}${url.hash}`
}
