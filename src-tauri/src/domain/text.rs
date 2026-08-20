//! Text derivation.
//!
//! Everything the app knows about a Page beyond its editor document is computed
//! here: the plain-text projection, the statistics, and the form that goes into
//! the search index. All of it is pure, so all of it is testable without a
//! database or a DOM, and all of it runs on the Rust side so the numbers cannot
//! drift between what was displayed and what was stored.

use serde_json::Value;

/// The longest a title may be. Long enough for a real chapter heading, short
/// enough that it cannot be used to smuggle a paragraph into a tree row.
pub const MAX_TITLE_LEN: usize = 200;

/// Node types that end a line in the plain-text projection.
///
/// Only *leaf* blocks — the ones that directly hold text. Containers such as
/// lists, list items, and block quotes are deliberately absent: their children
/// already emit the newline, and having containers emit one too would produce
/// runs of blank lines that then had to be collapsed afterwards.
///
/// Avoiding that collapse is not tidiness. The frontend maps plain-text offsets
/// back to editor positions to place annotations, and a post-processing pass
/// that removes characters would have to be mirrored there exactly. Emitting
/// the right string in one pass makes the two implementations trivially
/// comparable — see `fixtures/plain-text.json`.
const LEAF_BLOCK_TYPES: &[&str] = &[
    "paragraph",
    "heading",
    "codeBlock",
    "code_block",
    "horizontalRule",
    "horizontal_rule",
];

/// True for characters that stand alone as words: CJK ideographs, kana, and
/// Hangul. These scripts do not separate words with spaces, so a run of them
/// would otherwise be counted — and indexed — as a single enormous token.
pub fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0x20000..=0x2FA1F // CJK Extensions B–F and Compatibility Supplement
    )
}

/// Flattens a ProseMirror document to plain text.
///
/// The document JSON stays canonical; this projection exists for search,
/// previews, statistics, and AI context. It is written in the same transaction
/// as the document it came from, so the two cannot disagree.
pub fn plain_text_from_document(doc: &Value) -> String {
    let mut out = String::new();
    walk(doc, &mut out);
    // The only post-processing: the final block's trailing newline. It sits
    // past every anchorable character, so trimming it cannot shift an offset.
    out.trim_end_matches('\n').to_string()
}

fn walk(node: &Value, out: &mut String) {
    if let Some(text) = node.get("text").and_then(Value::as_str) {
        out.push_str(text);
        return;
    }

    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
    if node_type == "hardBreak" || node_type == "hard_break" {
        out.push('\n');
        return;
    }

    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            walk(child, out);
        }
    }

    if LEAF_BLOCK_TYPES.contains(&node_type) {
        out.push('\n');
    }
}

/// Counts words the way a writer would.
///
/// A run of letters and digits is one word, with apostrophes and hyphens held
/// inside it so "don't" and "well-known" each count once. Every CJK character
/// counts as one word, because in those scripts that is the closest honest
/// equivalent and treating a sentence as a single word would be useless.
pub fn count_words(text: &str) -> i64 {
    let mut words = 0i64;
    let mut in_word = false;

    for ch in text.chars() {
        if is_cjk(ch) {
            words += 1;
            in_word = false;
        } else if ch.is_alphanumeric() {
            if !in_word {
                words += 1;
                in_word = true;
            }
        } else if in_word && matches!(ch, '\'' | '\u{2019}' | '-' | '\u{2010}' | '_') {
            // Stays inside the current word.
        } else {
            in_word = false;
        }
    }

    words
}

/// Counts characters, including spaces — which is what a manuscript brief means
/// by a character count.
pub fn count_characters(text: &str) -> i64 {
    text.chars().count() as i64
}

/// Prepares text for the FTS5 index.
///
/// SQLite's `unicode61` tokenizer classifies CJK characters as letters, so an
/// unbroken run of them becomes one enormous token and searching for a two
/// character word inside it finds nothing. This was measured before the schema
/// was written: `trigram` does not fix it either, because a two character query
/// cannot form a trigram.
///
/// Separating CJK characters at index time — and applying the identical
/// transformation to queries — makes them individually addressable while
/// leaving space-separated scripts completely untouched.
pub fn segment_for_index(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut previous_was_cjk = false;

    for ch in text.chars() {
        let cjk = is_cjk(ch);
        if (cjk || previous_was_cjk)
            && !out.is_empty()
            && !out.ends_with(' ')
            && !ch.is_whitespace()
        {
            out.push(' ');
        }
        out.push(ch);
        previous_was_cjk = cjk;
    }

    out
}

/// Trims a title, collapses internal whitespace, and caps its length.
///
/// Returns an empty string if nothing survives, which callers translate into
/// their own default ("Untitled Page" and so on) rather than having one imposed
/// here.
pub fn normalise_title(raw: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_TITLE_LEN {
        return collapsed;
    }
    collapsed.chars().take(MAX_TITLE_LEN).collect()
}

/// A short single-line preview, for tree rows and search results.
pub fn preview(text: &str, max_chars: usize) -> String {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        return single_line;
    }
    let truncated: String = single_line.chars().take(max_chars).collect();
    // Prefer to break at a word boundary, but never lose more than a quarter of
    // the preview chasing one.
    match truncated.rfind(' ') {
        Some(at) if at > max_chars * 3 / 4 => format!("{}…", &truncated[..at]),
        _ => format!("{truncated}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(content: Value) -> Value {
        json!({ "type": "doc", "content": content })
    }

    fn para(text: &str) -> Value {
        json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
    }

    #[test]
    fn flattens_paragraphs_with_line_breaks_between_them() {
        let document = doc(json!([para("First."), para("Second.")]));
        assert_eq!(plain_text_from_document(&document), "First.\nSecond.");
    }

    #[test]
    fn keeps_marked_text_and_drops_the_marks() {
        let document = doc(json!([{
            "type": "paragraph",
            "content": [
                { "type": "text", "text": "The " },
                { "type": "text", "text": "salt", "marks": [{ "type": "italic" }] },
                { "type": "text", "text": " road." }
            ]
        }]));
        assert_eq!(plain_text_from_document(&document), "The salt road.");
    }

    #[test]
    fn hard_breaks_become_newlines() {
        let document = doc(json!([{
            "type": "paragraph",
            "content": [
                { "type": "text", "text": "one" },
                { "type": "hardBreak" },
                { "type": "text", "text": "two" }
            ]
        }]));
        assert_eq!(plain_text_from_document(&document), "one\ntwo");
    }

    #[test]
    fn container_blocks_do_not_add_separators_of_their_own() {
        let document = doc(json!([{
            "type": "bulletList",
            "content": [
                { "type": "listItem", "content": [para("alpha")] },
                { "type": "listItem", "content": [para("beta")] }
            ]
        }]));
        // Exactly one newline between the items. No collapse pass is involved,
        // which is what lets the frontend mirror this walk exactly.
        assert_eq!(plain_text_from_document(&document), "alpha\nbeta");
    }

    /// The contract the frontend's offset map is held to.
    ///
    /// Annotation anchors are character offsets into this text, computed here
    /// and resolved to editor positions in the frontend. If the two walks ever
    /// disagreed, annotations would attach to the wrong words — so both sides
    /// are tested against the same file.
    #[test]
    fn matches_the_shared_plain_text_fixtures() {
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            document: Value,
            text: String,
        }
        #[derive(serde::Deserialize)]
        struct Fixtures {
            cases: Vec<Case>,
        }

        let raw = include_str!("../../../fixtures/plain-text.json");
        let fixtures: Fixtures = serde_json::from_str(raw).expect("fixtures parse");
        assert!(fixtures.cases.len() >= 10, "fixtures look truncated");

        for case in fixtures.cases {
            assert_eq!(
                plain_text_from_document(&case.document),
                case.text,
                "fixture: {}",
                case.name
            );
        }
    }

    #[test]
    fn an_empty_document_flattens_to_nothing() {
        assert_eq!(plain_text_from_document(&doc(json!([]))), "");
        assert_eq!(plain_text_from_document(&doc(json!([para("")]))), "");
    }

    #[test]
    fn unknown_node_types_still_yield_their_text() {
        // A future schema addition must not silently drop content from search.
        let document = doc(json!([{
            "type": "someFutureNode",
            "content": [{ "type": "text", "text": "still indexed" }]
        }]));
        assert_eq!(plain_text_from_document(&document), "still indexed");
    }

    #[test]
    fn counts_english_words() {
        assert_eq!(count_words("The road had been salt once"), 6);
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("   \n  "), 0);
    }

    #[test]
    fn holds_apostrophes_and_hyphens_inside_words() {
        assert_eq!(count_words("don't"), 1);
        assert_eq!(count_words("don\u{2019}t"), 1);
        assert_eq!(count_words("well-known"), 1);
        assert_eq!(count_words("a well-known secret"), 3);
    }

    #[test]
    fn punctuation_alone_is_not_a_word() {
        assert_eq!(count_words("— ... !?"), 0);
        assert_eq!(count_words("Yes — really."), 2);
    }

    #[test]
    fn counts_each_cjk_character_as_a_word() {
        assert_eq!(count_words("手稿属于用户"), 6);
        // Mixed scripts add up rather than one swallowing the other:
        // 手 + 稿 + belongs + to + 用 + 户.
        assert_eq!(count_words("手稿 belongs to 用户"), 6);
    }

    #[test]
    fn counts_characters_including_spaces() {
        assert_eq!(count_characters("abc def"), 7);
        assert_eq!(count_characters("手稿"), 2);
        // Counts scalar values, not bytes.
        assert_eq!(count_characters("é"), 1);
    }

    #[test]
    fn segmentation_separates_cjk_and_leaves_latin_alone() {
        // Separators go between characters, never in front of the first one —
        // a leading space would show up in every indexed value for no reason.
        assert_eq!(segment_for_index("手稿属于用户"), "手 稿 属 于 用 户");
        assert_eq!(segment_for_index("the salt road"), "the salt road");
    }

    #[test]
    fn segmentation_handles_mixed_text_without_doubling_spaces() {
        let segmented = segment_for_index("第三章 The Salt Road");
        assert!(!segmented.contains("  "), "got {segmented:?}");
        assert!(segmented.contains("第 三 章"));
        assert!(segmented.contains("The Salt Road"));
    }

    #[test]
    fn titles_are_trimmed_and_collapsed() {
        assert_eq!(normalise_title("  The   Salt \n Road "), "The Salt Road");
        assert_eq!(normalise_title(""), "");
        assert_eq!(normalise_title("   "), "");
    }

    #[test]
    fn titles_are_capped_by_characters_not_bytes() {
        let long = "手".repeat(MAX_TITLE_LEN + 50);
        let capped = normalise_title(&long);
        assert_eq!(capped.chars().count(), MAX_TITLE_LEN);
    }

    #[test]
    fn previews_break_on_a_word_where_they_can() {
        let text = "The road had been salt once, or so the carters said";
        let short = preview(text, 20);
        assert!(short.ends_with('…'));
        assert!(short.chars().count() <= 21);
        assert!(!short.contains("  "));
    }

    #[test]
    fn short_text_is_returned_whole_without_an_ellipsis() {
        assert_eq!(preview("Chapter III", 40), "Chapter III");
    }

    #[test]
    fn previews_never_split_a_multibyte_character() {
        // Slicing by bytes would panic here.
        let text = "手稿属于用户手稿属于用户手稿属于用户";
        let short = preview(text, 5);
        assert!(short.chars().count() <= 6);
    }
}
