import { getMarkdown } from '@milkdown/kit/utils'
import { describe, expect, it } from 'vitest'
import { rawImageUrl, resolve } from '../src/images'
import { withEditor } from './serialize'

describe('rawImageUrl', () => {
  it('resolves relative images against the source file directory, like the read side', () => {
    expect(rawImageUrl('a/b/README.md', 'img.png')).toBe('/_riki/raw/a/b/img.png')
    expect(rawImageUrl('a/b/README.md', '../img.png')).toBe('/_riki/raw/a/img.png')
    expect(rawImageUrl('a/b/README.md', './x/img.png?v=1#f')).toBe('/_riki/raw/a/b/x/img.png?v=1#f')
    expect(rawImageUrl('README.md', 'img.png')).toBe('/_riki/raw/img.png')
  })

  it('passes absolute, external, and escaping URLs through', () => {
    for (const url of ['', '/abs.png', '//host/x.png', 'https://x/y.png', 'data:image/png;base64,AA', '#f']) {
      expect(rawImageUrl('a/b.md', url)).toBe(url)
    }
    expect(rawImageUrl('a.md', '../../x.png')).toBe('../../x.png')
    expect(resolve('a.md', '../x.png')).toBeNull()
  })
})

describe('image node view', () => {
  it('shows the resolved URL in edit mode and serializes the author src unchanged', async () => {
    const markdown = '![alt](img.png)\n'
    const [src, out] = await withEditor(
      markdown,
      (editor, root) => [root.querySelector('img:not(.ProseMirror-separator)')?.getAttribute('src'), editor.action(getMarkdown())],
      'a/b/README.md',
    )
    expect(src).toBe('/_riki/raw/a/b/img.png')
    expect(out).toBe(markdown)
  })
})
