//! Query building, execution, and snippet extraction.

use rusqlite::{Connection, params};

use crate::domain::text::{is_cjk, segment_for_index};
use crate::error::Result;

use super::{EntityKind, SearchHit};

/// How much text a result card shows around a match.
const SNIPPET_CHARS: usize = 180;

/// Builds an FTS5 MATCH expression from what the user typed.
///
/// Every token is quoted, which is what stops FTS5 syntax — `AND`, `OR`,
/// `NEAR`, `*`, `:`, `^` — being interpreted from manuscript search terms. A
/// writer searching for `OR` wants the word, not a boolean.
///
/// The final token gets a prefix wildcard so results appear while typing.
/// Returns `None` when nothing searchable survives, which callers treat as an
/// empty result rather than running a query that matches everything.
pub fn build_match(raw: &str) -> Option<String> {
    let segmented = segment_for_index(raw);
    let tokens: Vec<&str> = segmented
        .split_whitespace()
        .filter(|token| token.chars().any(|c| c.is_alphanumeric() || is_cjk(c)))
        .collect();

    if tokens.is_empty() {
        return None;
    }

    let last = tokens.len() - 1;
    let expression = tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            let quoted = token.replace('"', "\"\"");
            // A single CJK character is already a whole word; adding a prefix
            // wildcard to it would match a large fraction of the manuscript.
            let wildcard = index == last && token.chars().count() > 1 && !token.chars().all(is_cjk);
            if wildcard {
                format!("\"{quoted}\"*")
            } else {
                format!("\"{quoted}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ");

    Some(expression)
}

/// The words to mark inside a snippet.
fn highlight_terms(raw: &str) -> Vec<String> {
    segment_for_index(raw)
        .split_whitespace()
        .filter(|token| token.chars().any(|c| c.is_alphanumeric() || is_cjk(c)))
        .map(|token| token.to_lowercase())
        .collect()
}

/// Extracts a readable window around the first match, with ranges to mark.
///
/// Works on the original text, not the segmented form, so CJK comes back
/// without spaces inserted between its characters.
pub fn snippet_around(text: &str, terms: &[String], max_chars: usize) -> (String, Vec<(i64, i64)>) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (String::new(), Vec::new());
    }

    // Lowercased one character at a time, so the two vectors stay index-aligned.
    // `str::to_lowercase` can change the character count — 'İ' becomes two —
    // which would silently shift every highlight after it.
    let lowered: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();

    let matches = merge_overlaps(find_matches(&lowered, terms));

    // Centre the window on the first match, so the reader can see why it hit.
    let anchor = matches.first().map(|(start, _)| *start).unwrap_or(0);
    let mut from = anchor.saturating_sub(max_chars / 2);
    let mut to = (from + max_chars).min(chars.len());
    // Prefer a full window to a short one when the match is near the end.
    from = to.saturating_sub(max_chars);

    // Grow outward to word boundaries so a snippet does not begin mid-word,
    // bounded so a run of text without spaces cannot drag the window open.
    const SLACK: usize = 24;
    let floor = from.saturating_sub(SLACK);
    while from > floor && !chars[from - 1].is_whitespace() {
        from -= 1;
    }
    let ceiling = (to + SLACK).min(chars.len());
    while to < ceiling && !chars[to].is_whitespace() {
        to += 1;
    }

    // Shrink past any whitespace at the edges *before* offsets are computed,
    // so nothing has to be trimmed afterwards and shifted for.
    while from < to && chars[from].is_whitespace() {
        from += 1;
    }
    while to > from && chars[to - 1].is_whitespace() {
        to -= 1;
    }

    let mut snippet: String = chars[from..to].iter().collect();
    let elided_start = from > 0;
    if to < chars.len() {
        snippet.push('…');
    }
    if elided_start {
        snippet.insert(0, '…');
    }

    let shift = from as i64 - i64::from(elided_start);
    let highlights = matches
        .into_iter()
        .filter(|(start, end)| *start >= from && *end <= to)
        .map(|(start, end)| (start as i64 - shift, end as i64 - shift))
        .collect();

    (snippet, highlights)
}

fn find_matches(lowered: &[char], terms: &[String]) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    for term in terms {
        let needle: Vec<char> = term.chars().collect();
        if needle.is_empty() || needle.len() > lowered.len() {
            continue;
        }
        for start in 0..=(lowered.len() - needle.len()) {
            if lowered[start..start + needle.len()] == needle[..] {
                matches.push((start, start + needle.len()));
            }
        }
    }
    matches.sort_unstable();
    matches
}

/// Collapses overlapping ranges, so two terms matching the same words produce
/// one mark rather than nested ones the frontend would have to reconcile.
fn merge_overlaps(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Runs a search across the library.
///
/// `volume_id` narrows to one manuscript, which is what the workspace's own
/// search does; the Library searches everything.
pub fn search(
    conn: &Connection,
    raw_query: &str,
    volume_id: Option<&str>,
    limit: i64,
) -> Result<Vec<SearchHit>> {
    let Some(expression) = build_match(raw_query) else {
        return Ok(Vec::new());
    };
    let terms = highlight_terms(raw_query);

    // bm25 weights: a match in a title is worth far more than one in a body,
    // because a Page called "The Crows" is almost certainly what someone
    // searching for "crows" wanted.
    let sql = "
        SELECT s.entity_kind, s.entity_id, s.volume_id,
               bm25(search_index, 8.0, 1.0) AS score
          FROM search_index s
         WHERE search_index MATCH ?1
           AND (?2 IS NULL OR s.volume_id = ?2)
         ORDER BY score
         LIMIT ?3";

    let mut statement = conn.prepare(sql)?;
    let raw_hits = statement
        .query_map(params![expression, volume_id, limit], |row| {
            Ok((
                EntityKind::parse(&row.get::<_, String>(0)?),
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, f64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut hits = Vec::with_capacity(raw_hits.len());
    for (kind, entity_id, volume, score) in raw_hits {
        // The index stores segmented text; everything shown to the reader is
        // fetched from the source row so it reads as it was written.
        if let Some(hit) = hydrate(conn, kind, &entity_id, &volume, score, &terms)? {
            hits.push(hit);
        }
    }

    Ok(hits)
}

/// Turns an index row into a result card, reading display text from the source.
///
/// Returns `None` when the underlying record has gone — a stale index entry
/// should quietly disappear from results rather than produce a hit that opens
/// nothing.
fn hydrate(
    conn: &Connection,
    kind: EntityKind,
    entity_id: &str,
    volume_id: &str,
    score: f64,
    terms: &[String],
) -> Result<Option<SearchHit>> {
    let hit = match kind {
        EntityKind::Volume => conn
            .query_row(
                "SELECT title, COALESCE(subtitle, '') FROM volumes WHERE id = ?1",
                [entity_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .ok()
            .map(|(title, subtitle)| {
                let (snippet, highlights) = snippet_around(&subtitle, terms, SNIPPET_CHARS);
                SearchHit {
                    kind,
                    entity_id: entity_id.to_string(),
                    volume_id: volume_id.to_string(),
                    path: vec![],
                    title,
                    snippet,
                    highlights,
                    score,
                    page_id: None,
                }
            }),

        EntityKind::Chapter => conn
            .query_row(
                "SELECT c.title, v.title FROM chapters c
                   JOIN volumes v ON v.id = c.volume_id WHERE c.id = ?1",
                [entity_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .ok()
            .map(|(title, volume_title)| SearchHit {
                kind,
                entity_id: entity_id.to_string(),
                volume_id: volume_id.to_string(),
                path: vec![volume_title],
                title,
                snippet: String::new(),
                highlights: vec![],
                score,
                page_id: None,
            }),

        EntityKind::Page => conn
            .query_row(
                "SELECT p.title, p.plain_text, c.title, v.title
                   FROM pages p
                   JOIN chapters c ON c.id = p.chapter_id
                   JOIN volumes v  ON v.id = c.volume_id
                  WHERE p.id = ?1",
                [entity_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .ok()
            .map(|(title, body, chapter_title, volume_title)| {
                let (snippet, highlights) = snippet_around(&body, terms, SNIPPET_CHARS);
                SearchHit {
                    kind,
                    entity_id: entity_id.to_string(),
                    volume_id: volume_id.to_string(),
                    path: vec![volume_title, chapter_title],
                    title,
                    snippet,
                    highlights,
                    score,
                    page_id: Some(entity_id.to_string()),
                }
            }),

        EntityKind::Annotation => conn
            .query_row(
                "SELECT a.body, a.page_id, p.title, c.title, v.title
                   FROM annotations a
                   JOIN pages p    ON p.id = a.page_id
                   JOIN chapters c ON c.id = p.chapter_id
                   JOIN volumes v  ON v.id = c.volume_id
                  WHERE a.id = ?1",
                [entity_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .ok()
            .map(|(body, page_id, page_title, chapter_title, volume_title)| {
                let (snippet, highlights) = snippet_around(&body, terms, SNIPPET_CHARS);
                SearchHit {
                    kind,
                    entity_id: entity_id.to_string(),
                    volume_id: volume_id.to_string(),
                    path: vec![volume_title, chapter_title, page_title],
                    title: "Margin note".to_string(),
                    snippet,
                    highlights,
                    score,
                    page_id: Some(page_id),
                }
            }),

        EntityKind::Ink => conn
            .query_row(
                "SELECT r.recognized_text, a.page_id, p.title, c.title, v.title
                   FROM ink_recognition r
                   JOIN annotations a ON a.id = r.annotation_id
                   JOIN pages p        ON p.id = a.page_id
                   JOIN chapters c     ON c.id = p.chapter_id
                   JOIN volumes v      ON v.id = c.volume_id
                  WHERE r.annotation_id = ?1",
                [entity_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .ok()
            .and_then(
                |(transcript, page_id, page_title, chapter_title, volume_title)| {
                    // An ink note whose transcript was cleared should not produce a
                    // hit that opens nothing.
                    if transcript.is_empty() {
                        return None;
                    }
                    let (snippet, highlights) = snippet_around(&transcript, terms, SNIPPET_CHARS);
                    Some(SearchHit {
                        kind,
                        entity_id: entity_id.to_string(),
                        volume_id: volume_id.to_string(),
                        path: vec![volume_title, chapter_title, page_title],
                        title: "Ink Note".to_string(),
                        snippet,
                        highlights,
                        score,
                        page_id: Some(page_id),
                    })
                },
            ),
    };

    Ok(hit)
}
