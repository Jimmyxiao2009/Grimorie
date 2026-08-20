import { invoke } from './ipc';

export type SearchEntityKind = 'volume' | 'chapter' | 'page' | 'annotation';

export type SearchHit = {
  kind: SearchEntityKind;
  entityId: string;
  volumeId: string;
  /** "The Salt Road › Chapter I", for the result card. */
  path: string[];
  title: string;
  snippet: string;
  /**
   * Character ranges within `snippet` to mark.
   *
   * Ranges rather than markup: a manuscript is never interpreted as HTML, so
   * text containing angle brackets cannot become an injection.
   */
  highlights: [number, number][];
  score: number;
  /** The Page to open, for page and annotation hits. */
  pageId: string | null;
};

export function searchLibrary(
  query: string,
  volumeId?: string | null,
  limit?: number
): Promise<SearchHit[]> {
  return invoke('search_query', {
    query,
    volumeId: volumeId ?? null,
    limit: limit ?? null
  });
}

/** Rebuilds the index from the manuscript. Offered in Settings as a repair. */
export function rebuildSearchIndex(): Promise<number> {
  return invoke('search_rebuild');
}
