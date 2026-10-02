// The Phase 0a fixture set, synthetic: every histogram category has fixtures here. `canonical/`
// is the editor's own output format and must round-trip byte for byte (these pages pass the
// guard); `rewritten/` is valid GFM the serializer rewrites (these pages fail the guard, closed).

import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { nodeNames, roundTrip } from './serialize'

const FIXTURES = path.join(import.meta.dirname, '..', 'fixtures')

/** Phase 0a's failure-histogram categories, as fixture-name prefixes. */
const CATEGORIES = [
  'tables',
  'blank-line-inserted',
  'escapes',
  'bullet-char',
  'blank-lines-collapsed',
  'bare-url',
  'raw-html',
  'other',
  'list-spread',
  'indentation',
  'code-fence-style',
  'code-blocks',
  'links',
  'trailing-whitespace',
  'reference-links',
  'lists',
  'thematic-break',
  'list-marker-spacing',
  'hard-break',
  'ordered-list',
  'setext-heading',
  'code-span',
  'images',
  'autolink-brackets',
  'alerts',
]

/** Phase 0a's required alert fixtures: five types, multi-paragraph, list inside, inside a list. */
const REQUIRED_ALERTS = [
  'alerts--note',
  'alerts--tip',
  'alerts--important',
  'alerts--warning',
  'alerts--caution',
  'alerts--multi-paragraph',
  'alerts--list-inside',
  'alerts--inside-list-item',
]

function fixtures(kind: 'canonical' | 'rewritten'): Array<{ name: string; source: string }> {
  const dir = path.join(FIXTURES, kind)
  return fs
    .readdirSync(dir)
    .filter((file) => file.endsWith('.md'))
    .sort()
    .map((file) => ({ name: file.slice(0, -3), source: fs.readFileSync(path.join(dir, file), 'utf8') }))
}

/** The server's guard rule (`page::first_diff_line`): trailing newlines aside. */
function sameBody(a: string, b: string): boolean {
  return a.replace(/\n+$/, '') === b.replace(/\n+$/, '')
}

/** Rewrites that are not a fixed point: the first pass loses content (an empty-text link is
 *  dropped, leaving a trailing space the second pass trims). Still refused by the guard. */
const LOSSY = new Set(['links--empty-text'])

const canonical = fixtures('canonical')
/** Rewritten inputs whose point is trailing spaces live here, not in files: the repo's
 *  whitespace lint strips trailing spaces from every file it sees. */
const rewritten = [
  ...fixtures('rewritten'),
  { name: 'trailing-whitespace--spaces', source: 'Line with trailing space.   \n' },
  { name: 'hard-break--two-spaces', source: 'Line one  \nline two\n' },
]
const category = (name: string) => name.split('--')[0]

describe('fixture coverage', () => {
  it('has a fixture for every Phase 0a histogram category', () => {
    const covered = new Set([...canonical, ...rewritten].map(({ name }) => category(name)))
    expect(CATEGORIES.filter((c) => !covered.has(c))).toEqual([])
  })

  it('has every required alert fixture in the strict-pass set', () => {
    const names = new Set(canonical.map(({ name }) => name))
    expect(REQUIRED_ALERTS.filter((n) => !names.has(n))).toEqual([])
  })

  it('names every fixture by a known category', () => {
    const known = new Set([...CATEGORIES, 'nested-quote'])
    expect([...canonical, ...rewritten].map(({ name }) => name).filter((n) => !known.has(category(n) ?? ''))).toEqual([])
  })
})

describe('strict-pass fixtures serialize byte-identical', () => {
  it.each(canonical)('$name', async ({ source }) => {
    expect(await roundTrip(source)).toBe(source)
  })
})

describe('alert fixtures load as alert nodes', () => {
  it.each(canonical.filter(({ name }) => name.startsWith('alerts--')))('$name', async ({ source }) => {
    expect(await nodeNames(source)).toContain('alert')
  })
})

describe('rewritten fixtures fail the guard, and their rewrite is canonical', () => {
  it.each(rewritten)('$name', async ({ name, source }) => {
    const once = await roundTrip(source)
    expect(sameBody(once, source)).toBe(false)
    if (!LOSSY.has(name)) expect(await roundTrip(once)).toBe(once)
  })
})
