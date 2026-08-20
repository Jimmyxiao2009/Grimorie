/**
 * Draws annotated ranges in the manuscript.
 *
 * These are ProseMirror *decorations*, not marks — a distinction that matters.
 * A mark would be part of the document, so annotating a sentence would modify
 * the text, dirty the Page, and show up in exports. A decoration is drawn over
 * an unchanged document and leaves it untouched.
 *
 * The plugin holds a plain list of ranges and rebuilds its decorations from the
 * live document. Rebuilding costs nothing at manuscript sizes and avoids
 * maintaining a mapped DecorationSet, which is where off-by-one errors live.
 */

import { Extension } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import type { EditorView } from '@tiptap/pm/view';
import { Decoration, DecorationSet } from '@tiptap/pm/view';

export type HighlightRange = {
  id: string;
  /** ProseMirror positions, already resolved from the stored anchor. */
  from: number;
  to: number;
  kind: string;
  stale: boolean;
  focused: boolean;
};

export const highlightsKey = new PluginKey<HighlightRange[]>('grimoire-annotation-highlights');

/** Meta key used to push a new set of ranges into the plugin. */
const SET_RANGES = 'grimoire:set-highlight-ranges';

export const AnnotationHighlights = Extension.create({
  name: 'annotationHighlights',

  addProseMirrorPlugins() {
    return [
      new Plugin<HighlightRange[]>({
        key: highlightsKey,

        state: {
          init: () => [],
          apply(transaction, previous) {
            return (transaction.getMeta(SET_RANGES) as HighlightRange[] | undefined) ?? previous;
          }
        },

        props: {
          decorations(state) {
            const ranges = highlightsKey.getState(state) ?? [];
            if (ranges.length === 0) return DecorationSet.empty;

            const size = state.doc.content.size;
            const decorations = ranges
              // A range that no longer fits the document is dropped rather than
              // clamped. A highlight over the wrong words is worse than none,
              // and the backend has already judged such an anchor stale.
              .filter((range) => range.from >= 0 && range.to > range.from && range.to <= size)
              .map((range) =>
                Decoration.inline(range.from, range.to, {
                  class: [
                    'annotated',
                    `annotated-${range.kind}`,
                    range.stale ? 'annotated-stale' : '',
                    range.focused ? 'annotated-focused' : ''
                  ]
                    .filter(Boolean)
                    .join(' '),
                  'data-annotation': range.id
                })
              );

            return DecorationSet.create(state.doc, decorations);
          }
        }
      })
    ];
  }
});

/**
 * Replaces the highlighted ranges.
 *
 * Dispatched as a transaction with no document change and `addToHistory: false`,
 * so refreshing highlights never marks the Page unsaved and Ctrl+Z after
 * opening the Margin still undoes the writer's last edit.
 */
export function setHighlightRanges(view: EditorView | null, ranges: HighlightRange[]): void {
  if (!view) return;
  const transaction = view.state.tr.setMeta(SET_RANGES, ranges);
  transaction.setMeta('addToHistory', false);
  view.dispatch(transaction);
}
