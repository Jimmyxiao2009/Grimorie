/**
 * The editor schema.
 *
 * Restrained on purpose. Grimoire is for manuscripts, and every node type added
 * here is one more thing that has to survive import, export, search, anchoring,
 * and every future migration. The list below is what long-form prose actually
 * needs; anything else waits until a real manuscript demands it.
 *
 * Notably absent:
 *   - Code blocks. Inline code covers the occasional term; a fenced block in a
 *     novel is a sign the schema, not the writer, went wrong.
 *   - Tables, images, footnotes, embeds. Each is a project of its own.
 */

import StarterKit from '@tiptap/starter-kit';
import CharacterCount from '@tiptap/extension-character-count';
import Placeholder from '@tiptap/extension-placeholder';
import TextAlign from '@tiptap/extension-text-align';
import type { Extensions } from '@tiptap/core';
import { AnnotationHighlights } from './highlights';

export const HEADING_LEVELS = [1, 2, 3] as const;

export function buildExtensions(placeholder: string): Extensions {
  return [
    StarterKit.configure({
      heading: { levels: [...HEADING_LEVELS] },
      // See the note above.
      codeBlock: false,
      link: {
        // A manuscript is being edited, not browsed: clicking a link should
        // place the cursor, not navigate away mid-sentence.
        openOnClick: false,
        autolink: true,
        HTMLAttributes: { rel: 'noopener noreferrer nofollow' }
      }
    }),

    TextAlign.configure({ types: ['heading', 'paragraph'] }),

    Placeholder.configure({
      placeholder,
      // Only on the empty document, not on every empty paragraph — ghost text
      // appearing between finished paragraphs is a distraction.
      showOnlyWhenEditable: true,
      showOnlyCurrent: false
    }),

    // Live counts for the status line. The authoritative numbers still come
    // from the backend on save; these keep the display honest between saves.
    CharacterCount.configure({ limit: null }),

    // Draws annotated ranges. Decorations rather than marks, so annotating a
    // sentence never modifies the document.
    AnnotationHighlights
  ];
}
