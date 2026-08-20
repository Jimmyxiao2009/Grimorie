import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Page, ProseMirrorDocument } from '../src/lib/types/manuscript';

/**
 * The save queue is where a bug destroys a manuscript rather than merely
 * annoying someone, so these tests are about the failure modes, not the happy
 * path: saves addressed to the wrong Page, saves racing each other, and text
 * discarded when a save fails.
 */

const savePage = vi.fn<(id: string, document: ProseMirrorDocument) => Promise<Page>>();

vi.mock('$lib/services/manuscript', () => ({
  savePage: (id: string, document: ProseMirrorDocument) => savePage(id, document)
}));

const { Autosave } = await import('../src/lib/editor/autosave.svelte');

function doc(text: string): ProseMirrorDocument {
  return {
    type: 'doc',
    content: [{ type: 'paragraph', content: [{ type: 'text', text }] }]
  };
}

function savedPage(id: string): Page {
  return {
    id,
    chapterId: 'chapter',
    title: 'A Page',
    document: doc(''),
    plainText: '',
    position: 0,
    revisionNumber: 2,
    wordCount: 0,
    characterCount: 0,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z'
  };
}

/**
 * Lets awaited promises settle.
 *
 * Deliberately not `setTimeout`: the timers are faked, so a timeout-based wait
 * would never resolve. Draining microtasks is what is actually needed here.
 */
const settle = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};

let autosave: InstanceType<typeof Autosave>;

beforeEach(() => {
  vi.useFakeTimers();
  savePage.mockReset();
  savePage.mockImplementation(async (id) => savedPage(id));
  autosave = new Autosave();
  autosave.setDebounce(200);
});

afterEach(() => {
  vi.useRealTimers();
});

describe('autosave', () => {
  it('does not touch the backend on the keystroke itself', () => {
    autosave.schedule('page-1', doc('a'));
    expect(savePage).not.toHaveBeenCalled();
    expect(autosave.state).toBe('unsaved');
  });

  it('saves once after typing pauses, not once per keystroke', async () => {
    autosave.schedule('page-1', doc('a'));
    autosave.schedule('page-1', doc('ab'));
    autosave.schedule('page-1', doc('abc'));

    await vi.advanceTimersByTimeAsync(250);
    await settle();

    expect(savePage).toHaveBeenCalledTimes(1);
    expect(savePage.mock.calls[0]?.[1]).toEqual(doc('abc'));
    expect(autosave.state).toBe('saved');
  });

  it('addresses every save to the Page the text came from', async () => {
    // The writer types in one Page and immediately switches to another.
    autosave.schedule('page-1', doc('first page text'));
    autosave.schedule('page-2', doc('second page text'));

    await vi.advanceTimersByTimeAsync(250);
    await settle();

    // Whatever is written must never be page-1's text against page-2's id.
    for (const [id, document] of savePage.mock.calls) {
      const text = document.content?.[0]?.content?.[0]?.text ?? '';
      expect(text.startsWith(id === 'page-1' ? 'first' : 'second')).toBe(true);
    }
  });

  it('flushes pending work immediately when asked', async () => {
    autosave.schedule('page-1', doc('unsaved words'));
    await autosave.flush();

    expect(savePage).toHaveBeenCalledTimes(1);
    expect(savePage).toHaveBeenCalledWith('page-1', doc('unsaved words'));
    expect(autosave.state).toBe('saved');
  });

  it('flushing with nothing pending is harmless', async () => {
    await autosave.flush();
    expect(savePage).not.toHaveBeenCalled();
    expect(autosave.state).toBe('saved');
  });

  it('queues a change made while a save is in flight rather than racing it', async () => {
    let release: (page: Page) => void = () => {};
    savePage.mockImplementationOnce(
      () => new Promise<Page>((resolve) => (release = resolve))
    );

    autosave.schedule('page-1', doc('first'));
    await vi.advanceTimersByTimeAsync(250);
    expect(autosave.state).toBe('saving');

    // More typing arrives before the first save comes back.
    autosave.schedule('page-1', doc('second'));
    release(savedPage('page-1'));
    await vi.advanceTimersByTimeAsync(250);
    await settle();

    expect(savePage).toHaveBeenCalledTimes(2);
    expect(savePage.mock.calls[1]?.[1]).toEqual(doc('second'));
    expect(autosave.state).toBe('saved');
  });

  it('keeps the text queued when a save fails', async () => {
    savePage.mockRejectedValueOnce(new Error('Grimoire couldn’t save this Page.'));

    autosave.schedule('page-1', doc('precious words'));
    await vi.advanceTimersByTimeAsync(250);
    await settle();

    expect(autosave.state).toBe('failed');
    expect(autosave.failure).toContain('couldn’t save');

    // The text was not discarded: retrying sends the same document.
    await autosave.retry();
    expect(savePage).toHaveBeenLastCalledWith('page-1', doc('precious words'));
    expect(autosave.state).toBe('saved');
  });

  it('a failed older save never overwrites newer text', async () => {
    let reject: (error: Error) => void = () => {};
    savePage.mockImplementationOnce(() => new Promise<Page>((_, r) => (reject = r)));

    autosave.schedule('page-1', doc('old'));
    await vi.advanceTimersByTimeAsync(250);

    // The writer keeps typing while the save is failing.
    autosave.schedule('page-1', doc('new'));
    reject(new Error('network gone'));
    await settle();

    await autosave.retry();
    await settle();

    // The retry must carry the newer text, not resurrect the older document.
    expect(savePage.mock.calls.at(-1)?.[1]).toEqual(doc('new'));
  });

  it('reports each saved Page so counts can be adopted', async () => {
    const seen: Page[] = [];
    autosave.onsaved = (page) => seen.push(page);

    autosave.schedule('page-1', doc('a'));
    await autosave.flush();

    expect(seen).toHaveLength(1);
    expect(seen[0]?.id).toBe('page-1');
  });

  it('forgets queued work for a Page that no longer exists', async () => {
    autosave.schedule('page-1', doc('about to be deleted'));
    autosave.forget('page-1');

    await vi.advanceTimersByTimeAsync(250);
    await settle();

    expect(savePage).not.toHaveBeenCalled();
    expect(autosave.state).toBe('saved');
  });

  it('clamps the debounce to a band that is neither frantic nor forgetful', () => {
    autosave.setDebounce(1);
    autosave.schedule('page-1', doc('a'));
    vi.advanceTimersByTime(150);
    expect(savePage).not.toHaveBeenCalled(); // floor is 200ms

    autosave.setDebounce(10 ** 9);
    autosave.schedule('page-1', doc('b'));
    vi.advanceTimersByTime(5_100);
    expect(savePage).toHaveBeenCalled(); // ceiling is 5s
  });
});
