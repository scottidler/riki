// One editing session on one page, in place: the URL never changes. Design doc, Editor flow:
// load the page JSON, mount the editor read-only, run the round-trip guard (failing closed),
// save, and on a 409 keep the author's text and offer "load latest".

import type { Editor } from '@milkdown/kit/core'
import { editorViewCtx } from '@milkdown/kit/core'
import { configureLinkTooltip, linkTooltipPlugin } from '@milkdown/kit/component/link-tooltip'
import { getMarkdown } from '@milkdown/kit/utils'
import { checkRoundTrip, loadPage, savePage } from './api'
import type { GuardVerdict, PageJson } from './api'
import { makeEditor } from './setup'
import { buildToolbar } from './toolbar'
import type { Toolbar } from './toolbar'

/** Where the round-trip guard stands. `skipped` is a new page: nothing to compare. */
export type GuardState = 'pending' | 'passed' | 'refused' | 'skipped'

/** Save is enabled only after the guard passed (or was skipped) and while no save is in
 *  flight. A refusal is final for the session: nothing turns Save on again. */
export function saveAllowed(guard: GuardState, saving: boolean): boolean {
  return (guard === 'passed' || guard === 'skipped') && !saving
}

export function guardStateFor(verdict: GuardVerdict): GuardState {
  return verdict.identical ? 'passed' : 'refused'
}

/** Marks the header controls a session hid, so closing it shows exactly those again. */
const HIDDEN_WHILE_EDITING = 'data-riki-hidden-while-editing'

export interface PageTarget {
  /** Repo path of the file, e.g. `a/b.md`. */
  path: string
  /** The file on GitHub, when the content remote is a GitHub repo. */
  source: string | null
}

export interface SessionHooks {
  /** Re-render the page from the good tip after a save (closes this session). */
  rerender: () => Promise<void>
  /** The session closed (cancel or re-render); the page may start another. */
  closed: () => void
}

export class Session {
  #target: PageTarget
  #hooks: SessionHooks
  #article: HTMLElement
  #container: HTMLElement | null = null
  #bar: HTMLElement | null = null
  #status: HTMLElement | null = null
  #saveButton: HTMLButtonElement | null = null
  #toolbar: Toolbar | null = null
  #editor: Editor | null = null
  #page: PageJson | null = null
  #guard: GuardState = 'pending'
  #saving = false
  #editable = false

  constructor(target: PageTarget, article: HTMLElement, hooks: SessionHooks) {
    this.#target = target
    this.#article = article
    this.#hooks = hooks
  }

  /** Steps 1-3: load, mount read-only, run the guard. */
  async open(): Promise<void> {
    const loaded = await loadPage(this.#target.path)
    if (!loaded.ok) {
      this.#notice(loaded.reason)
      this.#hooks.closed()
      return
    }
    const page = loaded.page
    if (!page.editable || page.body === null) {
      this.#notice(`This page cannot be edited in the browser: ${page.reason ?? 'not editable'}.`)
      this.#hooks.closed()
      return
    }
    this.#page = page
    await this.#mount(page.body)
    const baseOid = page['base-oid']
    if (baseOid === null) {
      this.#setGuard('skipped')
      this.#say('New page.')
      return
    }
    this.#say('Checking that the editor can reproduce this page exactly...')
    const serialized = this.#markdown()
    const verdict = await checkRoundTrip({ path: page.path, baseOid, serialized })
    this.#setGuard(guardStateFor(verdict))
    if (verdict.identical) this.#say('')
    else this.#refuse(`Editing is off for this page: ${verdict.reason}.`)
  }

  /** Tear the editor down and show the article again. */
  async close(): Promise<void> {
    await this.#unmount()
    this.#article.hidden = false
    document.querySelector('.riki-notice')?.remove()
    this.#hooks.closed()
  }

  async #unmount(): Promise<void> {
    await this.#editor?.destroy()
    this.#editor = null
    this.#toolbar = null
    this.#bar?.remove()
    this.#bar = null
    for (const el of document.querySelectorAll<HTMLElement>(`[${HIDDEN_WHILE_EDITING}]`)) {
      el.hidden = false
      el.removeAttribute(HIDDEN_WHILE_EDITING)
    }
    this.#container?.remove()
    this.#container = null
    this.#status = null
    this.#saveButton = null
  }

  async #mount(markdown: string): Promise<void> {
    // Cancel / Save take the header's Edit slot (the Edit button hides meanwhile); the sticky
    // head above the page holds the toolbar and the status line.
    const container = document.createElement('div')
    container.className = 'riki-editor'
    const head = document.createElement('div')
    head.className = 'riki-editor-head'
    const bar = document.createElement('div')
    bar.className = 'riki-editor-bar'
    const status = document.createElement('span')
    status.className = 'riki-status'
    status.setAttribute('role', 'status')
    const save = button('save', 'Save', () => void this.#save(), 'riki-button riki-button-primary')
    save.disabled = true
    const cancel = button('cancel', 'Cancel', () => void this.close())
    bar.append(cancel, save)
    head.append(status)
    const slot = document.querySelector<HTMLElement>('header .actions')
    if (slot) {
      for (const el of slot.children) {
        if (el instanceof HTMLElement && !el.hidden) {
          el.hidden = true
          el.setAttribute(HIDDEN_WHILE_EDITING, '')
        }
      }
      slot.append(bar)
    } else {
      head.append(bar)
    }
    this.#bar = bar
    const root = document.createElement('div')
    root.className = 'riki-editor-root'
    container.append(head, root)
    this.#article.hidden = true
    this.#article.after(container)
    this.#container = container
    this.#status = status
    this.#saveButton = save

    const editor = await makeEditor({ root, markdown, sourceFile: this.#target.path, editable: () => this.#editable })
      .config(configureLinkTooltip)
      .use(linkTooltipPlugin)
      .create()
    this.#editor = editor
    this.#toolbar = buildToolbar(editor)
    head.prepend(this.#toolbar.element)
  }

  #markdown(): string {
    if (!this.#editor) throw new Error('no editor mounted')
    return this.#editor.action(getMarkdown())
  }

  #setGuard(guard: GuardState): void {
    this.#guard = guard
    this.#editable = guard === 'passed' || guard === 'skipped'
    this.#toolbar?.setEnabled(this.#editable)
    this.#editor?.action((ctx) => {
      const view = ctx.get(editorViewCtx)
      // ProseMirror reads `editable` on every state update; push one so it re-reads it now.
      view.updateState(view.state)
      if (this.#editable) view.focus()
    })
    this.#refreshSave()
  }

  #refreshSave(): void {
    if (this.#saveButton) this.#saveButton.disabled = !saveAllowed(this.#guard, this.#saving)
  }

  /** Step 4 and 5: save; 200 re-renders, 409 keeps the text and offers the latest version. */
  async #save(): Promise<void> {
    const page = this.#page
    if (!page || !saveAllowed(this.#guard, this.#saving)) return
    this.#saving = true
    this.#refreshSave()
    this.#say('Saving...')
    const result = await savePage({ path: page.path, baseOid: page['base-oid'], body: this.#markdown() })
    this.#saving = false
    switch (result.kind) {
      case 'saved':
        try {
          await this.#hooks.rerender()
        } catch (err) {
          this.#say(`Saved, but the page could not be refreshed (${String(err)}); reload it.`)
        }
        return
      case 'conflict':
        this.#conflict(result.message)
        break
      case 'error':
        this.#say(`Not saved: ${result.message}${result.retrySafe ? ' (safe to retry)' : ''}.`)
        break
    }
    this.#refreshSave()
  }

  #conflict(message: string): void {
    this.#say(`Not saved: ${message}. Your text is still here.`)
    const latest = button('load-latest', 'Load latest', () => void this.#loadLatest())
    this.#status?.append(' ', latest)
  }

  async #loadLatest(): Promise<void> {
    if (!window.confirm('Discard your edits and load the latest version of this page?')) return
    await this.#unmount()
    this.#guard = 'pending'
    this.#editable = false
    await this.open()
  }

  #say(text: string): void {
    if (this.#status) this.#status.textContent = text
  }

  /** The guard refused: say why, link the file on GitHub, and leave Save off for good. */
  #refuse(text: string): void {
    this.#say(text)
    const link = sourceLink(this.#target.source)
    if (link && this.#status) this.#status.append(' ', link)
  }

  /** A notice above the article when no editor mounts at all. */
  #notice(text: string): void {
    document.querySelector('.riki-notice')?.remove()
    const notice = document.createElement('div')
    notice.className = 'riki-notice'
    notice.setAttribute('role', 'status')
    notice.textContent = text
    const link = sourceLink(this.#target.source)
    if (link) notice.append(' ', link)
    this.#article.before(notice)
  }
}

function button(control: string, label: string, onClick: () => void, className = 'riki-button'): HTMLButtonElement {
  const el = document.createElement('button')
  el.type = 'button'
  el.className = className
  el.dataset['control'] = control
  el.textContent = label
  el.addEventListener('click', onClick)
  return el
}

function sourceLink(source: string | null): HTMLAnchorElement | null {
  if (!source) return null
  const link = document.createElement('a')
  link.className = 'riki-source'
  link.href = source
  link.textContent = 'Edit it on GitHub'
  link.rel = 'noopener'
  return link
}
