import { describe, expect, it } from 'vitest';
import { getSchema } from '@tiptap/core';
import { Node as PMNode } from '@tiptap/pm/model';

import { buildExtensions } from '../src/lib/editor/schema';
import {
  buildOffsetMap,
  posToTextOffset,
  selectionToTextRange,
  textOffsetToPos,
  textRangeToSelection
} from '../src/lib/editor/offsets';
import fixtures from '../fixtures/plain-text.json';

/**
 * The offset map is the bridge between where an annotation *says* it points
 * (a character offset, computed in Rust) and where it is *drawn* (a ProseMirror
 * position). A discrepancy here would attach comments to the wrong words
 * without any error being raised, so these tests check the contract itself
 * rather than the implementation.
 */

const schema = getSchema(buildExtensions('placeholder'));

function parse(json: unknown): PMNode {
  return PMNode.fromJSON(schema, json as never);
}

type Case = { name: string; document: unknown; text: string };
const cases = fixtures.cases as unknown as Case[];

describe('plain-text derivation', () => {
  it('reads a meaningful number of shared fixtures', () => {
    expect(cases.length).toBeGreaterThanOrEqual(10);
  });

  // The same file drives the Rust test in domain/text.rs. If either walk
  // changes, one of the two suites fails immediately.
  for (const testCase of cases) {
    it(`matches the backend for: ${testCase.name}`, () => {
      const map = buildOffsetMap(parse(testCase.document));
      expect(map.text).toBe(testCase.text);
    });
  }
});

describe('offset mapping', () => {
  const document = parse({
    type: 'doc',
    content: [
      { type: 'paragraph', content: [{ type: 'text', text: 'The salt road.' }] },
      { type: 'paragraph', content: [{ type: 'text', text: 'Wind and ash.' }] }
    ]
  });
  const map = buildOffsetMap(document);

  it('derives the text the anchors are measured against', () => {
    expect(map.text).toBe('The salt road.\nWind and ash.');
  });

  it('round-trips every offset in the document', () => {
    for (let offset = 0; offset <= map.text.length; offset++) {
      const pos = textOffsetToPos(map, offset);
      const back = posToTextOffset(map, pos);
      // Offsets that land on a block separator resolve to the nearest real
      // caret position, so they come back as the end of the previous run.
      const separator = map.text[offset] === '\n' || offset === map.text.length;
      if (!separator) expect(back, `offset ${offset}`).toBe(offset);
    }
  });

  it('resolves a range to the text it names', () => {
    const from = map.text.indexOf('salt');
    const range = { from, to: from + 'salt'.length };
    const selection = textRangeToSelection(map, range);
    expect(document.textBetween(selection.from, selection.to)).toBe('salt');
  });

  it('resolves a range that crosses a paragraph boundary', () => {
    const from = map.text.indexOf('road');
    const to = map.text.indexOf('Wind') + 'Wind'.length;
    const selection = textRangeToSelection(map, { from, to });
    // textBetween inserts nothing for block boundaries by default, so the
    // words either side must both be present and in order.
    const spanned = document.textBetween(selection.from, selection.to, '\n');
    expect(spanned.startsWith('road')).toBe(true);
    expect(spanned.endsWith('Wind')).toBe(true);
  });

  it('turns a selection back into the offsets an anchor stores', () => {
    const target = 'Wind';
    const from = map.text.indexOf(target);
    const selection = textRangeToSelection(map, { from, to: from + target.length });
    const range = selectionToTextRange(map, selection.from, selection.to);
    expect(map.text.slice(range.from, range.to)).toBe(target);
  });

  it('clamps out-of-range offsets instead of producing invalid positions', () => {
    expect(textOffsetToPos(map, -50)).toBeGreaterThanOrEqual(0);
    const end = textOffsetToPos(map, 10_000);
    expect(end).toBeLessThanOrEqual(document.content.size);
  });

  it('handles an empty document without throwing', () => {
    const empty = buildOffsetMap(parse({ type: 'doc', content: [{ type: 'paragraph' }] }));
    expect(empty.text).toBe('');
    expect(textOffsetToPos(empty, 0)).toBe(0);
    expect(posToTextOffset(empty, 0)).toBe(0);
  });
});

describe('offset mapping with mixed scripts', () => {
  const document = parse({
    type: 'doc',
    content: [
      {
        type: 'paragraph',
        content: [{ type: 'text', text: '手稿属于用户。The manuscript belongs to the user.' }]
      }
    ]
  });
  const map = buildOffsetMap(document);

  it('addresses CJK characters individually', () => {
    const from = map.text.indexOf('属于');
    const selection = textRangeToSelection(map, { from, to: from + 2 });
    expect(document.textBetween(selection.from, selection.to)).toBe('属于');
  });

  it('addresses Latin text that follows CJK', () => {
    const target = 'manuscript';
    const from = map.text.indexOf(target);
    const selection = textRangeToSelection(map, { from, to: from + target.length });
    expect(document.textBetween(selection.from, selection.to)).toBe(target);
  });
});

describe('offset mapping across marks', () => {
  // Marks split a paragraph into several text nodes, which is exactly where a
  // naive "position equals offset plus one" assumption breaks.
  const document = parse({
    type: 'doc',
    content: [
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'The ' },
          { type: 'text', text: 'salt', marks: [{ type: 'italic' }] },
          { type: 'text', text: ' road, and the ' },
          { type: 'text', text: 'carters', marks: [{ type: 'bold' }] },
          { type: 'text', text: ' who walked it.' }
        ]
      }
    ]
  });
  const map = buildOffsetMap(document);

  it('produces one continuous string across the runs', () => {
    expect(map.text).toBe('The salt road, and the carters who walked it.');
  });

  it('resolves a range inside a later run correctly', () => {
    const target = 'carters';
    const from = map.text.indexOf(target);
    const selection = textRangeToSelection(map, { from, to: from + target.length });
    expect(document.textBetween(selection.from, selection.to)).toBe(target);
  });

  it('resolves a range spanning several runs', () => {
    const target = 'salt road, and the carters';
    const from = map.text.indexOf(target);
    const selection = textRangeToSelection(map, { from, to: from + target.length });
    expect(document.textBetween(selection.from, selection.to)).toBe(target);
  });
});
