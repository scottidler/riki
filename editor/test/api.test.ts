import { describe, expect, it, vi } from 'vitest'
import { checkRoundTrip, loadPage, savePage } from '../src/api'

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
const request = { path: 'a.md', baseOid: 'abc', serialized: 'body\n' }

describe('checkRoundTrip fails closed', () => {
  it('passes only on identical: true', async () => {
    const fetchImpl = vi.fn(async () => json(200, { identical: true, 'first-diff-line': null }))
    expect(await checkRoundTrip(request, { fetchImpl })).toEqual({ identical: true })
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/_riki/api/roundtrip')
    expect(init.headers).toEqual({ 'Content-Type': 'application/json' })
    expect(JSON.parse(String(init.body))).toEqual({ path: 'a.md', 'base-oid': 'abc', serialized: 'body\n' })
  })

  it('refuses identical: false and names the line', async () => {
    const verdict = await checkRoundTrip(request, { fetchImpl: async () => json(200, { identical: false, 'first-diff-line': 3 }) })
    expect(verdict).toEqual({ identical: false, reason: 'the editor cannot reproduce this page exactly (first difference at line 3)' })
  })

  it('refuses an HTTP 500', async () => {
    const verdict = await checkRoundTrip(request, { fetchImpl: async () => new Response('boom', { status: 500 }) })
    expect(verdict.identical).toBe(false)
    if (!verdict.identical) expect(verdict.reason).toContain('HTTP 500')
  })

  it('refuses a network error', async () => {
    const verdict = await checkRoundTrip(request, { fetchImpl: async () => Promise.reject(new TypeError('offline')) })
    expect(verdict.identical).toBe(false)
  })

  it('refuses a malformed answer', async () => {
    const verdict = await checkRoundTrip(request, { fetchImpl: async () => json(200, { identical: 'yes' }) })
    expect(verdict.identical).toBe(false)
  })

  it('refuses when no answer arrives before the deadline', async () => {
    const never: typeof fetch = (_url, init) =>
      new Promise((_resolve, reject) => init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError'))))
    const verdict = await checkRoundTrip(request, { fetchImpl: never, timeoutMs: 20 })
    expect(verdict).toEqual({ identical: false, reason: 'the round-trip check did not answer within 0.02s' })
  })
})

describe('loadPage', () => {
  it('returns the page JSON', async () => {
    const page = { path: 'a.md', 'base-oid': null, body: '', editable: true, reason: null }
    const fetchImpl = vi.fn(async (_url: string) => json(200, page))
    expect(await loadPage('a b.md', fetchImpl as unknown as typeof fetch)).toEqual({ ok: true, page })
    expect(fetchImpl.mock.calls[0]?.[0]).toBe('/_riki/api/page?path=a%20b.md')
  })

  it('accepts a non-editable page with body null', async () => {
    const page = { path: 'a.md', 'base-oid': 'x', body: null, editable: false, reason: 'contains \\r' }
    expect(await loadPage('a.md', async () => json(200, page))).toEqual({ ok: true, page })
  })

  it('reports an HTTP error with the server message', async () => {
    const result = await loadPage('a.md', async () => json(400, { error: 'path must end in .md' }))
    expect(result).toEqual({ ok: false, reason: 'could not load the page: HTTP 400: path must end in .md' })
  })

  it('refuses a malformed body', async () => {
    expect((await loadPage('a.md', async () => json(200, { path: 'a.md' }))).ok).toBe(false)
  })
})

describe('savePage', () => {
  const save = { path: 'a.md', baseOid: null, body: 'x\n' }

  it('maps 200 to saved and sends the save request shape', async () => {
    const fetchImpl = vi.fn(async (_url: string, _init: RequestInit) => json(200, { commit: 'c0ffee', 'content-present': false }))
    expect(await savePage(save, fetchImpl as unknown as typeof fetch)).toEqual({ kind: 'saved', commit: 'c0ffee' })
    const init = fetchImpl.mock.calls[0]?.[1] ?? {}
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ path: 'a.md', 'base-oid': null, body: 'x\n' })
  })

  it('maps 409 to conflict', async () => {
    const result = await savePage(save, async () => json(409, { error: 'the page changed since it was loaded', 'current-body': 'y' }))
    expect(result).toEqual({ kind: 'conflict', message: 'the page changed since it was loaded' })
  })

  it('maps 503 retry-safe to a retryable error', async () => {
    const result = await savePage(save, async () => json(503, { error: 'push outcome unknown', 'retry-safe': true }))
    expect(result).toEqual({ kind: 'error', message: 'push outcome unknown', retrySafe: true })
  })

  it('maps a 502 to a non-retryable error', async () => {
    expect(await savePage(save, async () => json(502, { error: 'denied' }))).toEqual({ kind: 'error', message: 'denied', retrySafe: false })
  })
})
