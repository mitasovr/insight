// Minimal JSON-line logger. One record per line so k8s log collectors
// (fluentbit, loki) can parse without extra config.
//
// Format: {"ts":"2026-05-26T...","level":"info","msg":"...","field":"..."}
//
// No deps. ~20 lines. If we ever need log levels filtering / sampling /
// correlation IDs / etc — swap for pino. For now this is enough.

type LogLevel = 'info' | 'warn' | 'error';
type LogFields = Record<string, unknown>;

function emit(level: LogLevel, msg: string, fields: LogFields): void {
  const record = {
    ts: new Date().toISOString(),
    level,
    msg,
    ...fields,
  };
  // stdout is line-buffered in non-TTY mode (k8s captures stdout)
  process.stdout.write(JSON.stringify(record) + '\n');
}

export const log = {
  info: (msg: string, fields: LogFields = {}) => emit('info', msg, fields),
  warn: (msg: string, fields: LogFields = {}) => emit('warn', msg, fields),
  error: (msg: string, fields: LogFields = {}) => emit('error', msg, fields),
};
