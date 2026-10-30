const CODES = [
  'notFound',
  'permissionDenied',
  'invalid',
  'busy',
  'unsupported',
  'launch',
  'internal',
  /** The host speaks an SDK version this theme was not built for. */
  'incompatible'
] as const;

export type ErrorCode = (typeof CODES)[number];

function isCode(code: unknown): code is ErrorCode {
  return CODES.includes(code as ErrorCode);
}

/** Every failed call rejects with one of these: branch on `code`, show `message`. */
export class RomDexError extends Error {
  readonly code: ErrorCode;

  constructor(code: ErrorCode, message: string) {
    super(message);
    this.name = 'RomDexError';
    this.code = code;
  }

  /** Wraps whatever a backend rejected with. */
  static from(cause: unknown): RomDexError {
    if (cause instanceof RomDexError) return cause;
    if (typeof cause === 'object' && cause !== null && 'code' in cause && 'message' in cause) {
      const { code, message } = cause;
      if (typeof message === 'string') {
        // A code from a newer host than this SDK knows is still an error, just not a typed one.
        return new RomDexError(isCode(code) ? code : 'internal', message);
      }
    }
    return new RomDexError('internal', cause instanceof Error ? cause.message : String(cause));
  }
}
