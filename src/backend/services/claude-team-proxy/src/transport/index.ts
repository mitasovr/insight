// AuthedTransport contract — the seam between "HTTP server" and "actual
// way of talking to claude.ai".
//
// MVP: only PlaywrightTransport exists. The interface is overshaped on
// purpose so future swaps (curl_cffi, CloakBrowser CDP, mock for tests)
// drop in without server.ts changes.
//
// Design rules:
//   1. fetch() does NOT throw on non-2xx HTTP. Pass status verbatim;
//      let caller distinguish "upstream said no" from "upstream
//      unreachable" (the latter throws).
//   2. init() may take tens of seconds (browser launch + CF challenge).
//      isReady() is the fast sync check used by /health.
//   3. Transport is single-use re: identity. One transport = one
//      sessionKey = one logged-in identity. Multi-tenant = multiple
//      transports = multiple pods (k8s replicas).
//   4. URL is full URL (https://...). Transport does not prepend a base.
//      Keeps the seam free of "knowledge of claude.ai".

import { createPlaywrightTransport } from './playwright.js';

export interface TransportResponse {
  /** HTTP status code from upstream (1xx-5xx). */
  status: number;
  /** Response body as UTF-8 string. */
  body: string;
  /**
   * Lowercased response headers. Sparse — implementations may not have
   * all upstream headers if their underlying API does not expose them
   * (page.evaluate(fetch) does not).
   */
  headers: Record<string, string>;
}

export interface FetchOptions {
  /** HTTP method. MVP only supports GET. */
  method?: 'GET';
  /** Additional request headers merged with transport defaults. */
  headers?: Record<string, string>;
}

/**
 * Authenticated transport to claude.ai. Hides ALL details of how auth
 * is established (cookie, browser, TLS fingerprint, CF clearance, etc).
 *
 * Lifecycle: init() once at startup, then fetch() N times, then close()
 * at shutdown. init() may take 10-60s (CF challenge), fetch() should be
 * fast (~hundreds of ms).
 */
export interface AuthedTransport {
  /**
   * Establish authenticated session with upstream. Idempotent if called
   * on already-initialised transport (no-op). Throws on auth failure or
   * timeout — caller should treat the transport as dead.
   */
  init(): Promise<void>;
  /**
   * Perform an authenticated request. Returns the response verbatim;
   * non-2xx statuses are NOT thrown.
   * Throws ONLY when the request could not complete (browser crashed,
   * network error, transport-side timeout). In that case caller should
   * return HTTP 502/504 to its own client.
   */
  fetch(url: string, opts?: FetchOptions): Promise<TransportResponse>;
  /**
   * Synchronous: true if init() has successfully finished, false otherwise.
   * Used by /health probe — must be cheap (no I/O).
   */
  isReady(): boolean;
  /**
   * Graceful shutdown. Releases all resources (browser processes,
   * sockets). Safe to call on un-init'd transport.
   */
  close(): Promise<void>;
  /**
   * Identifier for debug/health output, e.g. 'playwright', 'mock'.
   * Surfaced in /health response.
   */
  readonly kind: string;
  /**
   * The base URL this transport is bound to (e.g. "https://claude.ai").
   * Used by the HTTP server layer to build absolute upstream URLs from
   * incoming request paths. Means the server never has to know the
   * upstream hostname — it just asks the transport.
   */
  readonly upstreamBaseUrl: string;
}

export interface TransportFactoryConfig {
  kind?: 'playwright';
  sessionKey: string;
  upstreamBaseUrl: string;
  headless: boolean;
  startupTimeoutMs: number;
  fetchTimeoutMs: number;
}

/**
 * Factory. MVP returns playwright; future may switch on env var.
 */
export function createTransport(config: TransportFactoryConfig): AuthedTransport {
  const kind = config.kind ?? 'playwright';
  switch (kind) {
    case 'playwright':
      return createPlaywrightTransport({
        sessionKey: config.sessionKey,
        upstreamBaseUrl: config.upstreamBaseUrl,
        headless: config.headless,
        startupTimeoutMs: config.startupTimeoutMs,
        fetchTimeoutMs: config.fetchTimeoutMs,
      });
    default:
      throw new Error(`Unknown transport kind: ${kind}`);
  }
}
