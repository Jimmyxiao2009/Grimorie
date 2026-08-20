import { describe, expect, it } from 'vitest';
import { IpcError, invoke, isDesktop } from '../src/lib/services/ipc';

describe('ipc', () => {
  it('reports that the backend is absent outside the desktop host', () => {
    // jsdom is not Tauri, so this must be false — if it ever reported true the
    // frontend would try to call a backend that cannot answer.
    expect(isDesktop).toBe(false);
  });

  it('fails loudly rather than inventing a result when the backend is absent', async () => {
    await expect(invoke('volumes_list')).rejects.toBeInstanceOf(IpcError);
  });

  it('names the failing command on the error', async () => {
    const error = await invoke('volumes_list').catch((err: unknown) => err);
    expect(error).toBeInstanceOf(IpcError);
    expect((error as IpcError).command).toBe('volumes_list');
  });
});
