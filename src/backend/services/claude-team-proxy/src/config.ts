// Env-driven config. Pure function: process.env -> typed object.
// Throws synchronously on bad input so the failure mode is "can't start"
// rather than "starts then crashes 30s later when something tries to use
// an undefined value".

export interface Config {
  /** claude.ai cookie. Required. */
  sessionKey: string;
  /**
   * Base URL to proxy to. Default "https://claude.ai". Override (e.g.
   * "http://localhost:8080") for integration tests against a mock server
   * without touching real claude.ai. Must NOT have trailing slash; the
   * URL is concatenated with request pathnames that always start with "/".
   */
  upstreamBaseUrl: string;
  /** HTTP server port. */
  port: number;
  /**
   * Whether to run browser headless. Default true. Set HEADLESS=false
   * locally to actually see the browser window while debugging.
   */
  headless: boolean;
  /**
   * Max time to wait for transport.init() (browser launch + Cloudflare
   * clearance). Default 60s.
   */
  startupTimeoutMs: number;
  /**
   * Max time to wait for a single upstream fetch via page.evaluate.
   * Bounded so a stalled upstream cannot leave a request indefinitely
   * in-flight. Default 45s — covers the observed worst-case (claude.ai's
   * /api/claude_code/metrics_aggs/users at ~13s) with generous headroom.
   */
  fetchTimeoutMs: number;
}

export function loadConfig(): Config {
  const sessionKey = process.env.SESSION_KEY;
  if (!sessionKey) {
    throw new Error('SESSION_KEY is required (claude.ai cookie value)');
  }

  const upstreamBaseUrl = parseUrlEnv('UPSTREAM_BASE_URL', 'https://claude.ai');

  const port = parseIntEnv('PORT', 3000);
  if (port < 1 || port > 65535) {
    throw new Error(`PORT out of range: ${port}`);
  }

  const headless = parseBoolEnv('HEADLESS', true);
  const startupTimeoutMs = parseIntEnv('STARTUP_TIMEOUT_MS', 60_000);
  const fetchTimeoutMs = parseIntEnv('FETCH_TIMEOUT_MS', 45_000);

  return { sessionKey, upstreamBaseUrl, port, headless, startupTimeoutMs, fetchTimeoutMs };
}

function parseUrlEnv(name: string, defaultValue: string): string {
  const raw = process.env[name] ?? defaultValue;
  try {
    new URL(raw);
  } catch {
    throw new Error(`${name} must be a valid URL, got: ${raw}`);
  }
  // We concatenate raw + "/path" later, so strip trailing slash to
  // avoid "https://claude.ai//api/...". URL().href adds a trailing /
  // for root paths, so normalise explicitly.
  return raw.replace(/\/+$/, '');
}

function parseIntEnv(name: string, defaultValue: number): number {
  const raw = process.env[name];
  if (raw === undefined || raw === '') return defaultValue;
  const n = Number(raw);
  if (!Number.isInteger(n)) {
    throw new Error(`${name} must be an integer, got: ${raw}`);
  }
  return n;
}

function parseBoolEnv(name: string, defaultValue: boolean): boolean {
  const raw = process.env[name];
  if (raw === undefined || raw === '') return defaultValue;
  if (['1', 'true', 'yes', 'on'].includes(raw.toLowerCase())) return true;
  if (['0', 'false', 'no', 'off'].includes(raw.toLowerCase())) return false;
  throw new Error(`${name} must be a boolean (true/false/1/0), got: ${raw}`);
}
