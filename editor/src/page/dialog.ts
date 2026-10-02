// A small modal for the page script: a title, a body, an error line, Cancel and a confirm
// button. Plain elements (no <dialog>, so it also works under jsdom); Escape and the backdrop
// close it, Enter in a field submits.

export interface DialogSpec {
  title: string
  body: HTMLElement
  confirmLabel: string
  /** Runs on confirm. Return an error message to keep the dialog open, or null when done. */
  onConfirm: () => Promise<string | null>
  /** Add `riki-danger` to the confirm button. */
  danger?: boolean
}

export interface OpenDialog {
  close(): void
}

export function openDialog(spec: DialogSpec): OpenDialog {
  const backdrop = document.createElement('div')
  backdrop.className = 'riki-dialog-backdrop'
  const box = document.createElement('form')
  box.className = 'riki-dialog'
  box.setAttribute('role', 'dialog')
  box.setAttribute('aria-modal', 'true')
  box.setAttribute('aria-label', spec.title)
  const heading = document.createElement('h2')
  heading.textContent = spec.title
  const error = document.createElement('p')
  error.className = 'riki-dialog-error'
  error.setAttribute('role', 'alert')
  error.hidden = true
  const actions = document.createElement('div')
  actions.className = 'riki-dialog-actions'
  const cancel = document.createElement('button')
  cancel.type = 'button'
  cancel.className = 'riki-button'
  cancel.dataset['control'] = 'dialog-cancel'
  cancel.textContent = 'Cancel'
  const confirm = document.createElement('button')
  confirm.type = 'submit'
  confirm.className = `riki-button riki-button-primary${spec.danger ? ' riki-danger' : ''}`
  confirm.dataset['control'] = 'dialog-confirm'
  confirm.textContent = spec.confirmLabel
  actions.append(cancel, confirm)
  box.append(heading, spec.body, error, actions)
  backdrop.append(box)

  const close = (): void => {
    document.removeEventListener('keydown', onKey, true)
    backdrop.remove()
  }
  const onKey = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    close()
  }
  cancel.addEventListener('click', close)
  backdrop.addEventListener('mousedown', (event) => {
    if (event.target === backdrop) close()
  })
  box.addEventListener('submit', (event) => {
    event.preventDefault()
    if (confirm.disabled) return
    confirm.disabled = true
    error.hidden = true
    void spec.onConfirm().then((message) => {
      if (message === null) {
        close()
        return
      }
      confirm.disabled = false
      error.textContent = message
      error.hidden = false
    })
  })
  document.addEventListener('keydown', onKey, true)
  document.body.append(backdrop)
  box.querySelector<HTMLElement>('input, button')?.focus()
  return { close }
}

/** A labelled text field. */
export function field(label: string, name: string, value = ''): { row: HTMLElement; input: HTMLInputElement } {
  const row = document.createElement('label')
  row.className = 'riki-field'
  const text = document.createElement('span')
  text.textContent = label
  const input = document.createElement('input')
  input.type = 'text'
  input.name = name
  input.value = value
  input.autocomplete = 'off'
  input.spellcheck = false
  row.append(text, input)
  return { row, input }
}
