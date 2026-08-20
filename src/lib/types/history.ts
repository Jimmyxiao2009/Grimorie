import type { Id, ProseMirrorDocument, Timestamp } from './manuscript';

/** Why a snapshot exists. Mirrors `RevisionReason` in Rust. */
export type RevisionReason = 'checkpoint' | 'before-restore' | 'before-ai' | 'before-import';

/** A history row. Carries no document — the list must stay cheap to draw. */
export type RevisionSummary = {
  id: Id;
  revisionNumber: number;
  wordCount: number;
  /** Words gained or lost against the revision immediately before it. */
  wordDelta: number;
  reason: RevisionReason;
  preview: string;
  createdAt: Timestamp;
};

export type Revision = {
  id: Id;
  pageId: Id;
  revisionNumber: number;
  document: ProseMirrorDocument;
  plainText: string;
  wordCount: number;
  reason: RevisionReason;
  createdAt: Timestamp;
};

/** Text that was typed but never committed, offered back after a crash. */
export type RecoverableDraft = {
  pageId: Id;
  pageTitle: string;
  volumeTitle: string;
  document: ProseMirrorDocument;
  preview: string;
  wordCount: number;
  /** What the Page currently holds, so the two can be compared. */
  savedPreview: string;
  savedWordCount: number;
  capturedAt: Timestamp;
};

export const REVISION_REASON_LABELS: Record<RevisionReason, string> = {
  checkpoint: 'While writing',
  'before-restore': 'Before restoring',
  'before-ai': 'Before an AI edit',
  'before-import': 'Before importing'
};
