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

/** Raised when a backend call fails, carrying the message the backend chose. */
export class IpcError extends Error {
  readonly command: string;

  constructor(command: string, message: string) {
    super(message);
    this.name = 'IpcError';
    this.command = command;
  }
}

/**
 * Calls a backend command.
 *
 * Errors from Rust arrive as strings; they are already written for a reader
 * (see `AppError`), so they are surfaced rather than replaced with a generic
 * message.
 */
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop) {
    throw new IpcError(
      command,
      'Grimoire’s backend is not available. This build is running outside the desktop app.'
    );
  }
  try {
    return await tauriInvoke<T>(command, args);
  } catch (raw) {
    throw new IpcError(command, typeof raw === 'string' ? raw : String(raw));
  }
}
