/**
 * The manuscript contract.
 *
 * These mirror the `serde` output of the Rust domain types. They are written by
 * hand rather than generated: the surface is small, and a hand-written contract
 * is one both sides can read. Any drift shows up immediately as a type error at
 * a service boundary rather than as a silent `undefined` at runtime.
 */

/** An opaque identifier. Never parsed or constructed in the frontend. */
export type Id = string;

/** RFC3339 UTC, as produced by the backend. */
export type Timestamp = string;

export type CoverTint = 'ink' | 'clay' | 'moss' | 'slate' | 'wine' | 'ochre' | 'plum' | 'sea';

export type Volume = {
  id: Id;
  title: string;
  subtitle: string | null;
  description: string | null;
  tint: CoverTint;
  createdAt: Timestamp;
  updatedAt: Timestamp;
  lastOpenedAt: Timestamp | null;
  archivedAt: Timestamp | null;
};

/** A Volume plus the counts the Library shelf shows. */
export type VolumeSummary = Volume & {
  chapterCount: number;
  pageCount: number;
  wordCount: number;
};

export type Chapter = {
  id: Id;
  volumeId: Id;
  title: string;
  position: number;
  createdAt: Timestamp;
  updatedAt: Timestamp;
};

/** What the navigation tree needs. Deliberately carries no document. */
export type PageSummary = {
  id: Id;
  chapterId: Id;
  title: string;
  position: number;
  wordCount: number;
  preview: string;
  updatedAt: Timestamp;
};

/** A Page with its editor document. Loaded only when a Page is opened. */
export type Page = {
  id: Id;
  chapterId: Id;
  title: string;
  /** ProseMirror document JSON — the canonical representation. */
  document: ProseMirrorDocument;
  plainText: string;
  position: number;
  revisionNumber: number;
  wordCount: number;
  characterCount: number;
  createdAt: Timestamp;
  updatedAt: Timestamp;
};

export type ProseMirrorNode = {
  type: string;
  attrs?: Record<string, unknown>;
  content?: ProseMirrorNode[];
  marks?: { type: string; attrs?: Record<string, unknown> }[];
  text?: string;
};

export type ProseMirrorDocument = ProseMirrorNode & { type: 'doc' };

export type ChapterOutline = Chapter & { pages: PageSummary[] };

export type Outline = {
  volume: Volume;
  chapters: ChapterOutline[];
};

export type Shelf = 'active' | 'archived' | 'all';

export type AppSettings = {
  theme: string;
  manuscriptFace: string;
  manuscriptSize: number;
  manuscriptLeading: number;
  manuscriptParagraphGap: number;
  manuscriptMeasure: number;
  autosaveDebounceMs: number;
  revisionIntervalSeconds: number;
  spellcheck: boolean;
  aiEnabled: boolean;
  aiContextBudgetChars: number;
};
