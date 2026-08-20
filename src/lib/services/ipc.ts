import { invoke as tauriInvoke } from '@tauri-apps/api/core';

/**
 * True when running inside the Tauri host rather than a plain browser tab.
 *
 * `pnpm dev` in a browser is useful for pure layout work, so the frontend must
 * not hard-crash without a backend — but it must also never invent data to fill
 * the gap. Callers get an explicit failure instead.
 */
export const isDesktop: boolean =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Mirrors `ErrorCode` in src-tauri/src/error.rs. */
export type ErrorCode =
  | 'not-found'
  | 'conflict'
  | 'invalid-input'
  | 'stale'
  | 'database'
  | 'migration'
  | 'io'
  | 'network'
  | 'provider'
  | 'credential'
  | 'cancelled'
  | 'internal'
  | 'unavailable';

type BackendError = { code: ErrorCode; message: string; detail?: string };

function isBackendError(value: unknown): value is BackendError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as BackendError).code === 'string' &&
    typeof (value as BackendError).message === 'string'
  );
}

/**
 * A failure from the backend.
 *
 * The message is already written for a reader — the Rust side owns that
 * wording — so the UI shows it as-is rather than substituting something
 * generic. `code` is what the UI branches on.
 */
export class IpcError extends Error {
  readonly command: string;
  readonly code: ErrorCode;
  readonly detail: string | undefined;

  constructor(command: string, code: ErrorCode, message: string, detail?: string) {
    super(message);
    this.name = 'IpcError';
    this.command = command;
    this.code = code;
    this.detail = detail;
  }

  /** True when the thing being acted on has gone — usually a stale UI. */
  get isMissing(): boolean {
    return this.code === 'not-found';
  }
}

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop) {
    throw new IpcError(
      command,
      'unavailable',
      'Grimoire’s backend is not available. This build is running outside the desktop app.'
    );
  }

  try {
    return await tauriInvoke<T>(command, args);
  } catch (raw) {
    if (isBackendError(raw)) {
      throw new IpcError(command, raw.code, raw.message, raw.detail);
    }
    // Transport-level failures — a command that does not exist, a serialisation
    // mismatch — never reach the Rust error type, so they are reported as
    // internal rather than dressed up as something the user did wrong.
    throw new IpcError(
      command,
      'internal',
      'Grimoire hit an unexpected problem. Your work has not been changed.',
      typeof raw === 'string' ? raw : String(raw)
    );
  }
}

/** Renders any thrown value as a sentence suitable for the user. */
export function describeError(error: unknown): string {
  if (error instanceof IpcError) return error.message;
  if (error instanceof Error && error.message) return error.message;
  return 'Grimoire hit an unexpected problem. Your work has not been changed.';
}
