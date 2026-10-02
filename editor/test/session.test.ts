import { describe, expect, it } from 'vitest'
import { guardStateFor, saveAllowed } from '../src/session'

describe('saveAllowed', () => {
  it('allows save after the guard passed or was skipped', () => {
    expect(saveAllowed('passed', false)).toBe(true)
    expect(saveAllowed('skipped', false)).toBe(true)
  })

  it('never allows save while the guard is pending or after it refused', () => {
    expect(saveAllowed('pending', false)).toBe(false)
    expect(saveAllowed('refused', false)).toBe(false)
  })

  it('blocks a second save while one is in flight', () => {
    expect(saveAllowed('passed', true)).toBe(false)
  })
})

describe('guardStateFor', () => {
  it('maps verdicts to guard states', () => {
    expect(guardStateFor({ identical: true })).toBe('passed')
    expect(guardStateFor({ identical: false, reason: 'x' })).toBe('refused')
  })
})
