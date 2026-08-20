//! Applying a suggested edit to a document.
//!
//! This is the only code in Grimoire that writes a model's words into a
//! manuscript, so it is written to refuse rather than to cope.
//!
//! Two guarantees:
//!
//! 1. **The text is re-checked first.** [`validate`] compares what the
//!    suggestion expected to replace against what the Page holds now. If the
//!    words have changed, the suggestion is stale and nothing is written.
//! 2. **The splice is exact.** [`splice`] replaces precisely the character
//!    range that was validated, keeping the formatting of the text it replaces,
//!    and refuses any range it cannot express faithfully.
//!
//! A range spanning more than one block is refused. Merging paragraphs to fit a
//! replacement would restructure the manuscript in a way the writer did not ask
//! for, and telling them so is better than guessing.

use serde_json::{Map, Value};

use crate::domain::annotation::AnnotationAnchor;
use crate::domain::annotation::{Relocation, hash, relocate};
use crate::domain::text::plain_text_from_document;
use crate::error::{AppError, ErrorCode, Result};

/// Node types that end a line — the same set the plain-text walk uses.
const LEAF_BLOCKS: &[&str] = &[
    "paragraph",
    "heading",
    "codeBlock",
    "code_block",
    "horizontalRule",
    "horizontal_rule",
];

/// The outcome of checking a suggestion against the Page as it stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Validation {
    /// Safe to apply, at these character offsets in the current plain text.
    Ready { from: i64, to: i64 },
    /// The text has changed. Nothing should be written.
    Stale { reason: String },
}

/// Re-checks a suggestion against the Page's current text.
///
/// The stored offsets are tried first. If the text has moved but is still
/// present and unambiguous, the suggestion travels with it — editing a
/// paragraph above a suggestion should not invalidate it. If the text itself
/// changed, or several identical passages make the target ambiguous, the answer
/// is stale.
pub fn validate(
    current_text: &str,
    original_text: &str,
    context_hash: &str,
    from: i64,
    to: i64,
) -> Validation {
    if original_text.is_empty() {
        return Validation::Stale {
            reason: "The suggestion has no text to replace.".into(),
        };
    }

    // A mismatched hash means the record itself disagrees with its own text.
    if hash(original_text) != context_hash {
        return Validation::Stale {
            reason: "This suggestion could not be verified, so it was not applied.".into(),
        };
    }

    let chars: Vec<char> = current_text.chars().collect();
    let start = from.clamp(0, chars.len() as i64) as usize;
    let end = to.clamp(start as i64, chars.len() as i64) as usize;
    let at_offsets: String = chars[start..end].iter().collect();

    if at_offsets == original_text {
        return Validation::Ready {
            from: start as i64,
            to: end as i64,
        };
    }

    // The text moved. Reuse the anchor relocation the Margin already relies on,
    // which refuses ambiguous matches rather than guessing.
    let anchor = AnnotationAnchor {
        from,
        to,
        selected_text: original_text.to_string(),
        context_before: String::new(),
        context_after: String::new(),
        base_revision: 0,
        text_hash: context_hash.to_string(),
    };

    match relocate(&anchor, current_text) {
        Relocation::Unchanged => Validation::Ready { from, to },
        Relocation::Moved { from, to, .. } => Validation::Ready { from, to },
        Relocation::Lost => Validation::Stale {
            reason: "The text this suggestion was written for has changed, so it was not \
                     applied. Your manuscript has not been altered."
                .into(),
        },
    }
}

/// Replaces a character range of a document's plain text with new text.
///
/// The replacement inherits the marks of the text it begins in, so tightening a
/// sentence inside an italic passage stays italic.
pub fn splice(document: &Value, from: i64, to: i64, replacement: &str) -> Result<Value> {
    if to <= from {
        return Err(AppError::invalid("That suggestion covers no text."));
    }

    let mut cursor = Cursor {
        offset: 0,
        from: from as usize,
        to: to as usize,
        replacement,
        blocks_touched: 0,
        done: false,
    };

    let spliced = walk(document, &mut cursor);

    if cursor.blocks_touched == 0 {
        return Err(AppError::new(
            ErrorCode::Stale,
            "Grimoire couldn't find that text to replace, so nothing was changed.",
        ));
    }
    if cursor.blocks_touched > 1 {
        return Err(AppError::new(
            ErrorCode::Conflict,
            "This suggestion spans more than one paragraph, so Grimoire didn't apply it \
             automatically. The suggested text is in the Margin if you want it.",
        ));
    }

    Ok(spliced)
}

struct Cursor<'a> {
    offset: usize,
    from: usize,
    to: usize,
    replacement: &'a str,
    blocks_touched: usize,
    done: bool,
}

/// Rebuilds a node, splicing the target range where it falls.
///
/// Mirrors the plain-text walk exactly: text nodes advance the cursor by their
/// length, hard breaks by one, and leaf blocks by one at their end. If the two
/// ever diverged, a suggestion would replace the wrong words.
fn walk(node: &Value, cursor: &mut Cursor<'_>) -> Value {
    if let Some(text) = node.get("text").and_then(Value::as_str) {
        let start = cursor.offset;
        let length = text.chars().count();
        let end = start + length;
        cursor.offset = end;

        // No overlap with the range being replaced.
        if end <= cursor.from || start >= cursor.to {
            return node.clone();
        }

        let overlap_start = cursor.from.max(start) - start;
        let overlap_end = cursor.to.min(end) - start;

        let chars: Vec<char> = text.chars().collect();
        let prefix: String = chars[..overlap_start].iter().collect();
        let suffix: String = chars[overlap_end..].iter().collect();

        // The replacement lands in the first node it overlaps; later nodes in
        // the range contribute only their untouched tails.
        let middle = if cursor.done {
            String::new()
        } else {
            cursor.done = true;
            cursor.replacement.to_string()
        };

        let rebuilt = format!("{prefix}{middle}{suffix}");
        if rebuilt.is_empty() {
            // An empty text node is invalid in ProseMirror.
            return Value::Null;
        }

        let mut object = node.as_object().cloned().unwrap_or_default();
        object.insert("text".into(), Value::String(rebuilt));
        return Value::Object(object);
    }

    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");

    if node_type == "hardBreak" || node_type == "hard_break" {
        cursor.offset += 1;
        return node.clone();
    }

    let block_start = cursor.offset;
    let mut object: Map<String, Value> = node.as_object().cloned().unwrap_or_default();

    if let Some(children) = node.get("content").and_then(Value::as_array) {
        let rebuilt: Vec<Value> = children
            .iter()
            .map(|child| walk(child, cursor))
            // Text nodes emptied by the splice are dropped rather than left
            // behind as invalid empty nodes.
            .filter(|child| !child.is_null())
            .collect();

        if rebuilt.is_empty() {
            object.remove("content");
        } else {
            object.insert("content".into(), Value::Array(rebuilt));
        }
    }

    if LEAF_BLOCKS.contains(&node_type) {
        let block_end = cursor.offset;
        // Does the replaced range reach into this block's text?
        if cursor.from < block_end && cursor.to > block_start {
            cursor.blocks_touched += 1;
        }
        cursor.offset += 1; // the newline this block contributes
    }

    Value::Object(object)
}

/// Applies a validated suggestion to a document, returning the new document.
pub fn apply(document: &Value, from: i64, to: i64, replacement: &str) -> Result<Value> {
    let spliced = splice(document, from, to, replacement)?;

    // A last check that the splice did what it claimed. Cheap, and the failure
    // it guards against — a walk that drifted out of step with the plain-text
    // derivation — would otherwise be silent.
    let before = plain_text_from_document(document).chars().count();
    let after = plain_text_from_document(&spliced).chars().count();
    let expected = before as i64 - (to - from) + replacement.chars().count() as i64;
    if after as i64 != expected {
        return Err(AppError::internal(format!(
            "splice produced {after} characters, expected {expected}"
        )));
    }

    Ok(spliced)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn paragraph(text: &str) -> Value {
        json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
    }

    fn doc(blocks: Vec<Value>) -> Value {
        json!({ "type": "doc", "content": blocks })
    }

    fn offsets(text: &str, phrase: &str) -> (i64, i64) {
        let start = text
            .find(phrase)
            .map(|b| text[..b].chars().count())
            .unwrap();
        (start as i64, (start + phrase.chars().count()) as i64)
    }

    // --- Validation ---------------------------------------------------------

    #[test]
    fn unchanged_text_validates_at_its_original_offsets() {
        let text = "The road had been salt once.";
        let (from, to) = offsets(text, "salt");
        let result = validate(text, "salt", &hash("salt"), from, to);
        assert_eq!(result, Validation::Ready { from, to });
    }

    #[test]
    fn a_suggestion_travels_when_text_above_it_grows() {
        // Editing a paragraph above a suggestion must not invalidate it.
        let original = "The road had been salt once.";
        let edited = format!("A new opening. {original}");
        let (from, to) = offsets(original, "salt");

        match validate(&edited, "salt", &hash("salt"), from, to) {
            Validation::Ready { from, to } => {
                let found: String = edited
                    .chars()
                    .skip(from as usize)
                    .take((to - from) as usize)
                    .collect();
                assert_eq!(found, "salt");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn changed_text_is_stale_and_says_the_manuscript_is_untouched() {
        let (from, to) = offsets("The road had been salt once.", "salt");
        let result = validate(
            "The road had been sand once.",
            "salt",
            &hash("salt"),
            from,
            to,
        );
        match result {
            Validation::Stale { reason } => {
                assert!(reason.contains("has not been altered"), "{reason}");
            }
            other => panic!("expected Stale, got {other:?}"),
        }
    }

    #[test]
    fn a_record_that_disagrees_with_itself_is_refused() {
        // A hash that does not match its own original_text means the row is
        // corrupt; applying it would be writing something unverified.
        let result = validate("anything", "salt", "not-the-real-hash", 0, 4);
        assert!(matches!(result, Validation::Stale { .. }));
    }

    #[test]
    fn an_ambiguous_target_is_refused_rather_than_guessed() {
        let text = "salt salt salt salt";
        // Offsets that no longer hold the text, and four identical candidates.
        let result = validate(text, "salt", &hash("salt"), 40, 44);
        assert!(matches!(result, Validation::Stale { .. }), "{result:?}");
    }

    // --- Splicing -----------------------------------------------------------

    #[test]
    fn a_phrase_is_replaced_in_place() {
        let document = doc(vec![paragraph("The road had been salt once.")]);
        let (from, to) = offsets("The road had been salt once.", "salt");

        let result = apply(&document, from, to, "brine").unwrap();
        assert_eq!(
            plain_text_from_document(&result),
            "The road had been brine once."
        );
    }

    #[test]
    fn the_replacement_keeps_the_formatting_of_what_it_replaces() {
        let document = doc(vec![json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": "The " },
                { "type": "text", "text": "salt", "marks": [{ "type": "italic" }] },
                { "type": "text", "text": " road." }
            ]
        })]);

        let result = apply(&document, 4, 8, "brine").unwrap();
        assert_eq!(plain_text_from_document(&result), "The brine road.");

        let marked = &result["content"][0]["content"][1];
        assert_eq!(marked["text"], "brine");
        assert_eq!(marked["marks"][0]["type"], "italic");
    }

    #[test]
    fn a_replacement_spanning_several_text_nodes_produces_one_run() {
        let document = doc(vec![json!({
            "type": "paragraph",
            "content": [
                { "type": "text", "text": "one " },
                { "type": "text", "text": "two", "marks": [{ "type": "bold" }] },
                { "type": "text", "text": " three" }
            ]
        })]);

        // Replace "two three" — crossing a node boundary.
        let result = apply(&document, 4, 13, "TWO THREE").unwrap();
        assert_eq!(plain_text_from_document(&result), "one TWO THREE");
    }

    #[test]
    fn other_paragraphs_are_untouched() {
        let document = doc(vec![
            paragraph("First paragraph."),
            paragraph("The road had been salt once."),
            paragraph("Third paragraph."),
        ]);
        let text = plain_text_from_document(&document);
        let (from, to) = offsets(&text, "salt");

        let result = apply(&document, from, to, "brine").unwrap();
        let after = plain_text_from_document(&result);
        assert!(after.starts_with("First paragraph."));
        assert!(after.ends_with("Third paragraph."));
        assert!(after.contains("brine"));
    }

    #[test]
    fn a_range_crossing_a_paragraph_boundary_is_refused() {
        // Merging paragraphs to fit a replacement would restructure the
        // manuscript in a way nobody asked for.
        let document = doc(vec![paragraph("First one."), paragraph("Second one.")]);
        let text = plain_text_from_document(&document);
        let (from, _) = offsets(&text, "one.");
        let to = text.chars().count() as i64;

        let error = apply(&document, from, to, "merged").unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(
            error.message.contains("more than one paragraph"),
            "{}",
            error.message
        );
        assert!(error.message.contains("Margin"), "{}", error.message);
    }

    #[test]
    fn a_range_past_the_end_of_the_document_is_refused() {
        let document = doc(vec![paragraph("Short.")]);
        assert!(apply(&document, 500, 600, "x").is_err());
    }

    #[test]
    fn an_empty_range_is_refused() {
        let document = doc(vec![paragraph("Short.")]);
        assert!(apply(&document, 2, 2, "x").is_err());
    }

    #[test]
    fn replacing_a_whole_paragraph_leaves_a_valid_document() {
        let document = doc(vec![paragraph("Replace me entirely.")]);
        let result = apply(&document, 0, 20, "New text.").unwrap();
        assert_eq!(plain_text_from_document(&result), "New text.");
        assert_eq!(result["content"][0]["type"], "paragraph");
    }

    #[test]
    fn cjk_is_replaced_by_character_not_by_byte() {
        let document = doc(vec![paragraph("手稿属于用户。")]);
        let result = apply(&document, 2, 4, "来自").unwrap();
        assert_eq!(plain_text_from_document(&result), "手稿来自用户。");
    }

    #[test]
    fn a_replacement_inside_a_heading_stays_a_heading() {
        let document = doc(vec![
            json!({
                "type": "heading",
                "attrs": { "level": 2 },
                "content": [{ "type": "text", "text": "Chapter III" }]
            }),
            paragraph("Body."),
        ]);

        let result = apply(&document, 8, 11, "IV").unwrap();
        assert_eq!(result["content"][0]["type"], "heading");
        assert_eq!(result["content"][0]["attrs"]["level"], 2);
        assert_eq!(plain_text_from_document(&result), "Chapter IV\nBody.");
    }

    #[test]
    fn a_replacement_inside_a_list_item_keeps_the_list() {
        let document = doc(vec![json!({
            "type": "bulletList",
            "content": [
                { "type": "listItem", "content": [paragraph("alpha")] },
                { "type": "listItem", "content": [paragraph("beta")] }
            ]
        })]);

        let result = apply(&document, 0, 5, "ALPHA").unwrap();
        assert_eq!(result["content"][0]["type"], "bulletList");
        assert_eq!(plain_text_from_document(&result), "ALPHA\nbeta");
    }

    #[test]
    fn the_length_check_catches_a_splice_that_drifted() {
        // Exercised indirectly by every apply test above: if the walk fell out
        // of step with the plain-text derivation, the arithmetic would not add
        // up and apply would refuse rather than silently corrupt the Page.
        let document = doc(vec![paragraph("The road had been salt once.")]);
        let (from, to) = offsets("The road had been salt once.", "salt");
        let result = apply(&document, from, to, "a much longer replacement indeed").unwrap();
        let expected = "The road had been a much longer replacement indeed once.";
        assert_eq!(plain_text_from_document(&result), expected);
    }
}
