import type { Id, Timestamp } from './manuscript';

export type AnnotationKind =
  | 'note'
  | 'question'
  | 'suggestion'
  | 'warning'
  | 'reference'
  | 'ai-review'
  | 'ai-suggestion';

export type AnnotationStatus = 'active' | 'resolved' | 'stale';

export type AnnotationAnchor = {
  /** Character offset into the Page's plain text, inclusive. */
  from: number;
  /** Character offset one past the last character. */
  to: number;
  selectedText: string;
  contextBefore: string;
  contextAfter: string;
  baseRevision: number;
  textHash: string;
};

/** Tagged union, mirroring `AnnotationTarget` in Rust. */
export type AnnotationTarget = { kind: 'page' } | ({ kind: 'range' } & AnnotationAnchor);

export type Annotation = {
  id: Id;
  pageId: Id;
  kind: AnnotationKind;
  status: AnnotationStatus;
  body: string;
  target: AnnotationTarget;
  /** Names the AI profile when the note was written by one. */
  authorProfile: string | null;
  createdAt: Timestamp;
  updatedAt: Timestamp;
};

export function anchorOf(annotation: Annotation): AnnotationAnchor | null {
  return annotation.target.kind === 'range' ? annotation.target : null;
}

/**
 * The kinds a person can choose.
 *
 * AI kinds are deliberately absent: they are produced by a review, never picked
 * from a menu, and offering them would invite notes labelled as AI output that
 * no model ever wrote.
 */
export const AUTHORABLE_KINDS: AnnotationKind[] = [
  'note',
  'question',
  'suggestion',
  'warning',
  'reference'
];

export const KIND_LABELS: Record<AnnotationKind, string> = {
  note: 'Note',
  question: 'Question',
  suggestion: 'Suggestion',
  warning: 'Warning',
  reference: 'Reference',
  'ai-review': 'AI review',
  'ai-suggestion': 'AI suggestion'
};

/**
 * The mark that identifies a kind in the Margin.
 *
 * A glyph rather than a colour. Six tinted cards down one column reads as a
 * dashboard; six marginal notes distinguished by a sign in front of them reads
 * as someone's handwriting. Colour is left to say one thing only — whether an
 * anchor has come unstuck — instead of being spent on taxonomy.
 *
 * AI notes take the same marks as their human equivalents, because an AI is an
 * author, not a category. The attribution line under the note says who wrote it.
 */
export const KIND_GLYPHS: Record<AnnotationKind, string> = {
  note: '—',
  question: '?',
  suggestion: '↗',
  warning: '!',
  reference: '§',
  'ai-review': '—',
  'ai-suggestion': '↗'
};
