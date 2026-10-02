// riki's page API, typed. Every call returns data; nothing here touches the DOM.

export const PAGE_API = '/_riki/api/page'
export const ROUNDTRIP_API = '/_riki/api/roundtrip'
/** The round-trip guard's deadline (design doc, Editor flow step 3). */
export const GUARD_TIMEOUT_MS = 5000

type Fetch = typeof fetch

/** `GET /_riki/api/page`: front matter is never sent; `body` is null when not editable. */
export interface PageJson {
  path: string
  'base-oid': string | null
  body: string | null
  editable: boolean
  reason: string | null
}

export type LoadResult = { ok: true; page: PageJson } | { ok: false; reason: string }

export async function loadPage(path: string, fetchImpl: Fetch = fetch): Promise<LoadResult> {
  let response: Response
  try {
    response = await fetchImpl(`${PAGE_API}?path=${encodeURIComponent(path)}`, { headers: { Accept: 'application/json' } })
  } catch (err) {
    return { ok: false, reason: `could not load the page: ${String(err)}` }
  }
  if (!response.ok) return { ok: false, reason: `could not load the page: ${await errorText(response)}` }
  const page = (await response.json().catch(() => null)) as PageJson | null
  if (!isPageJson(page)) return { ok: false, reason: 'could not load the page: malformed response' }
  return { ok: true, page }
}

function isPageJson(value: unknown): value is PageJson {
  if (typeof value !== 'object' || value === null) return false
  const v = value as Record<string, unknown>
  return (
    typeof v['path'] === 'string' &&
    (typeof v['base-oid'] === 'string' || v['base-oid'] === null) &&
    (typeof v['body'] === 'string' || v['body'] === null) &&
    typeof v['editable'] === 'boolean'
  )
}

/** The guard's answer. Anything but a well-formed `identical: true` is a refusal. */
export type GuardVerdict = { identical: true } | { identical: false; reason: string }

export interface RoundTripRequest {
  path: string
  baseOid: string
  serialized: string
}

/** `POST /_riki/api/roundtrip`, failing closed: `identical: false`, an HTTP error, a network
 *  error, a malformed answer, or no answer within `timeoutMs` all refuse. */
export async function checkRoundTrip(
  request: RoundTripRequest,
  { timeoutMs = GUARD_TIMEOUT_MS, fetchImpl = fetch }: { timeoutMs?: number; fetchImpl?: Fetch } = {},
): Promise<GuardVerdict> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const response = await fetchImpl(ROUNDTRIP_API, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: request.path, 'base-oid': request.baseOid, serialized: request.serialized }),
      signal: controller.signal,
    })
    if (!response.ok) {
      return { identical: false, reason: `the round-trip check failed (${await errorText(response)})` }
    }
    const result = (await response.json()) as { identical?: unknown; 'first-diff-line'?: unknown }
    if (result.identical === true) return { identical: true }
    if (result.identical === false) {
      const line = typeof result['first-diff-line'] === 'number' ? ` (first difference at line ${result['first-diff-line']})` : ''
      return { identical: false, reason: `the editor cannot reproduce this page exactly${line}` }
    }
    return { identical: false, reason: 'the round-trip check answered with a malformed response' }
  } catch (err) {
    if (controller.signal.aborted) {
      return { identical: false, reason: `the round-trip check did not answer within ${timeoutMs / 1000}s` }
    }
    return { identical: false, reason: `the round-trip check failed (${String(err)})` }
  } finally {
    clearTimeout(timer)
  }
}

export interface SaveRequest {
  path: string
  baseOid: string | null
  body: string
}

export type SaveResult =
  | { kind: 'saved'; commit: string | null }
  | { kind: 'conflict'; message: string }
  | { kind: 'error'; message: string; retrySafe: boolean }

/** `POST /_riki/api/page`. A 409 is a conflict (the page moved, or the save would break the
 *  index); everything else that is not a 200 is an error carrying the server's words. */
export async function savePage(request: SaveRequest, fetchImpl: Fetch = fetch): Promise<SaveResult> {
  let response: Response
  try {
    response = await fetchImpl(PAGE_API, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: request.path, 'base-oid': request.baseOid, body: request.body }),
    })
  } catch (err) {
    return { kind: 'error', message: `the save did not reach the server: ${String(err)}`, retrySafe: true }
  }
  const json = (await response.json().catch(() => ({}))) as Record<string, unknown>
  const message = typeof json['error'] === 'string' ? json['error'] : `HTTP ${response.status}`
  if (response.ok) return { kind: 'saved', commit: typeof json['commit'] === 'string' ? json['commit'] : null }
  if (response.status === 409) return { kind: 'conflict', message }
  return { kind: 'error', message, retrySafe: json['retry-safe'] === true }
}

async function errorText(response: Response): Promise<string> {
  const text = await response.text().catch(() => '')
  try {
    const json = JSON.parse(text) as { error?: unknown }
    if (typeof json.error === 'string') return `HTTP ${response.status}: ${json.error}`
  } catch {
    // not JSON; fall through to the raw text
  }
  return text.trim() ? `HTTP ${response.status}: ${text.trim()}` : `HTTP ${response.status}`
}
