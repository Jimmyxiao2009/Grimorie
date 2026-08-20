/**
 * The bridge between annotation anchors and editor positions.
 *
 * Anchors are stored as character offsets into a Page's plain text, because
 * that is what lets relocation live in Rust where it can be tested without a
 * DOM. The editor, meanwhile, works in ProseMirror positions. This module
 * converts between the two.
 *
 * The walk below must produce *exactly* the same string as
 * `plain_text_from_document` in `src-tauri/src/domain/text.rs`. That equality
 * is load-bearing — if the two ever disagreed, annotations would silently
 * attach to the wrong words. Both are tested against
 * `fixtures/plain-text.json`, and neither may be changed without the other.
 */

import type { Node as PMNode } from '@tiptap/pm/model';

/**
 * Node types that end a line. Only leaf blocks: containers such as lists and
 * block quotes hold children that already emit the newline.
 *
 * Mirrors `LEAF_BLOCK_TYPES` in text.rs.
 */
const LEAF_BLOCKS = new Set([
  'paragraph',
  'heading',
  'codeBlock',
  'code_block',
  'horizontalRule',
  'horizontal_rule'
]);

const HARD_BREAKS = new Set(['hardBreak', 'hard_break']);

/** A run of text, and where it lives in both coordinate systems. */
type Segment = {
  /** Offset of the run's first character in the plain text. */
  textFrom: number;
  /** Offset one past its last character. */
  textTo: number;
  /** ProseMirror position of its first character. */
  pos: number;
};

export type OffsetMap = {
  /** Identical to what the backend derived and stored. */
  text: string;
  segments: Segment[];
};

/**
 * Walks a document, building the plain text and the position mapping together.
 *
 * One pass, so the two cannot drift from each other.
 */
export function buildOffsetMap(doc: PMNode): OffsetMap {
  const segments: Segment[] = [];
  let text = '';

  const walk = (node: PMNode, pos: number): void => {
    if (node.isText) {
      const content = node.text ?? '';
      if (content.length > 0) {
        segments.push({ textFrom: text.length, textTo: text.length + content.length, pos });
        text += content;
      }
      return;
    }

    if (HARD_BREAKS.has(node.type.name)) {
      text += '\n';
      return;
    }

    // Children of a node at `pos` begin at `pos + 1`.
    let childPos = pos + 1;
    node.forEach((child) => {
      walk(child, childPos);
      childPos += child.nodeSize;
    });

    if (LEAF_BLOCKS.has(node.type.name)) text += '\n';
  };

  // The document node itself has no position; its children start at 0.
  walk(doc, -1);

  // The final block's trailing newline sits past every anchorable character,
  // so dropping it cannot shift an offset. Mirrors the Rust trim.
  const trimmed = text.replace(/\n+$/, '');
  return { text: trimmed, segments };
}

/** Binary search for the segment containing (or nearest to) an offset. */
function locate(segments: Segment[], offset: number): number {
  let low = 0;
  let high = segments.length - 1;
  let best = -1;

  while (low <= high) {
    const mid = (low + high) >> 1;
    const segment = segments[mid]!;
    if (offset < segment.textFrom) {
      high = mid - 1;
    } else if (offset >= segment.textTo) {
      best = mid;
      low = mid + 1;
    } else {
      return mid;
    }
  }
  return best;
}

/**
 * Converts a plain-text offset to an editor position.
 *
 * An offset landing on a block separator — a newline that exists in the plain
 * text but is not a character in the document — resolves to the end of the
 * preceding run, which is the nearest real position a caret can occupy.
 */
export function textOffsetToPos(map: OffsetMap, offset: number): number {
  const { segments } = map;
  if (segments.length === 0) return 0;

  const clamped = Math.max(0, Math.min(offset, map.text.length));
  const index = locate(segments, clamped);

  if (index < 0) return segments[0]!.pos;

  const segment = segments[index]!;
  if (clamped >= segment.textTo) {
    // In a gap between runs: sit at the end of this one.
    return segment.pos + (segment.textTo - segment.textFrom);
  }
  return segment.pos + (clamped - segment.textFrom);
}

/** Converts an editor position to a plain-text offset. */
export function posToTextOffset(map: OffsetMap, pos: number): number {
  const { segments } = map;
  if (segments.length === 0) return 0;

  let best = 0;
  for (const segment of segments) {
    const length = segment.textTo - segment.textFrom;
    if (pos < segment.pos) return best;
    if (pos <= segment.pos + length) return segment.textFrom + (pos - segment.pos);
    best = segment.textTo;
  }
  return best;
}

/** A selection expressed in plain-text offsets. */
export type TextRange = { from: number; to: number };

/** Converts an editor selection to the coordinates an anchor is stored in. */
export function selectionToTextRange(map: OffsetMap, from: number, to: number): TextRange {
  const start = posToTextOffset(map, Math.min(from, to));
  const end = posToTextOffset(map, Math.max(from, to));
  return { from: start, to: end };
}

/** Converts a stored anchor range back to editor positions. */
export function textRangeToSelection(map: OffsetMap, range: TextRange): { from: number; to: number } {
  return {
    from: textOffsetToPos(map, range.from),
    to: textOffsetToPos(map, range.to)
  };
}
