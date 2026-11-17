const CODES = [
  'notFound',
  'permissionDenied',
  'invalid',
  'busy',
  'unsupported',
  'launch',
  /** A service could not be reached and nothing was cached. */
  'network',
  /** A feature needs setting up first; the message says how. */
  'notConfigured',
  /** A game's system needs a BIOS that is missing; `bios:missing` says which. Since 1.39.0. */
  'biosMissing',
  'internal',
  /** The host speaks an SDK version this theme was not built for. */
  'incompatible'
] as const;

export type ErrorCode = (typeof CODES)[number];

function isCode(code: unknown): code is ErrorCode {
  return CODES.includes(code as ErrorCode);
}

/** Every failed call rejects with one of these: branch on `code`, show `message`. */
export class FormaError extends Error {
  readonly code: ErrorCode;

  constructor(code: ErrorCode, message: string) {
    super(message);
    this.name = 'FormaError';
    this.code = code;
  }

  /** Wraps whatever a backend rejected with. */
  static from(cause: unknown): FormaError {
    if (cause instanceof FormaError) return cause;
    if (typeof cause === 'object' && cause !== null && 'code' in cause && 'message' in cause) {
      const { code, message } = cause;
      if (typeof message === 'string') {
        // A code from a newer host than this SDK knows is still an error, just not a typed one.
        return new FormaError(isCode(code) ? code : 'internal', message);
      }
    }
    return new FormaError('internal', cause instanceof Error ? cause.message : String(cause));
  }
}
