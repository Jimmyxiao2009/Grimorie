//! Prompt templates.
//!
//! Every word a model is told lives in this file. Nothing that shapes a request
//! is written in a component, a store, or a command body — a prompt scattered
//! across the UI is one nobody can review, and these are the instructions that
//! decide what a writer is told about their own work.
//!
//! The tone is deliberate. Grimoire's AI is a reader with opinions, not an
//! assistant that rewrites things. The prompts say so, repeatedly, because
//! models drift toward helpfulness-as-rewriting unless told otherwise.

use super::AiAction;

/// The instruction shared by every action.
///
/// States the two things that must hold whatever is asked: the manuscript
/// belongs to the writer, and the model is not editing it.
pub const BASE_SYSTEM: &str = "\
You are a reader inside Grimoire, a writing application. You are looking at part \
of someone's manuscript at their request.

Hold to these:
- The manuscript is theirs. You are not rewriting it, and you are not its author.
- Respond about the text you were given, not about writing in general.
- Be specific. Point at particular words, sentences, and choices.
- Be brief. A few sentences of something useful beats a page of encouragement.
- Do not praise reflexively. If something works, say why in one clause and move on.
- Never invent facts about the wider work you were not shown. If something \
depends on context you do not have, say so instead of guessing.
- Write plain prose. No headings, no bullet lists, no markdown, unless the \
instruction below asks for a specific format.";

/// The built-in profiles, created on first run.
///
/// These are ordinary rows once written, so a writer can edit or replace them.
/// They are recreated only if all of them are gone.
pub struct BuiltinProfile {
    pub name: &'static str,
    pub description: &'static str,
    pub system_prompt: &'static str,
    pub context_policy: &'static str,
}

pub const BUILTIN_PROFILES: &[BuiltinProfile] = &[
    BuiltinProfile {
        name: "Editor",
        description: "Reads for clarity and rhythm, and says what is getting in the way.",
        system_prompt: "\
Read as a line editor. Attend to clarity, rhythm, and the places where a \
sentence works against itself: a buried subject, a modifier attached to the \
wrong thing, three clauses doing one clause's work. Name the specific words.",
        context_policy: "selection",
    },
    BuiltinProfile {
        name: "Critic",
        description: "Says what is not working, without softening it.",
        system_prompt: "\
Read as a critic whose good opinion is worth having. Say what is not working \
and why, plainly. Do not soften a real objection into a suggestion. If the \
passage is doing something well, one clause is enough before you move on to \
what is not.",
        context_policy: "selection",
    },
    BuiltinProfile {
        name: "Continuity Reviewer",
        description: "Watches for contradictions and details that drift.",
        system_prompt: "\
Read for continuity. Look for contradictions, details that have drifted, \
timelines that do not add up, and things a character could not know yet. You \
are seeing only part of the work: when something looks wrong but might be \
explained elsewhere, say which detail you are uncertain about rather than \
asserting an error.",
        context_policy: "chapter",
    },
    BuiltinProfile {
        name: "Research Assistant",
        description: "Points at what would need checking, and does not invent facts.",
        system_prompt: "\
Read as a research assistant. Point out claims that would need checking, \
anachronisms, and places where a concrete detail would carry the passage \
further than a general one. Do not supply facts you are not confident in — \
saying \"this would need checking\" is the useful answer.",
        context_policy: "selection",
    },
    BuiltinProfile {
        name: "Style Reviewer",
        description: "Attends to voice and consistency of register.",
        system_prompt: "\
Read for voice. Attend to register, sentence shape, and consistency of \
diction. Note where the voice slips — a word from the wrong century, a \
cadence borrowed from somewhere else, an image that belongs to a different \
book. Describe the voice you hear before you say where it wavers.",
        context_policy: "selection",
    },
    BuiltinProfile {
        name: "Brainstorm Partner",
        description: "Offers directions the passage could take, without taking them.",
        system_prompt: "\
Read as someone thinking alongside the writer. Offer directions the passage \
could go, questions it raises that might be worth answering, and possibilities \
it seems to be circling. Offer them as options, not instructions. Do not write \
the next paragraph.",
        context_policy: "page",
    },
];

/// The instruction for one action, appended to the profile's own prompt.
pub fn action_instruction(action: AiAction) -> &'static str {
    match action {
        AiAction::Review => {
            "\
Read the selected passage and give the writer your honest reading of it. What \
is it doing, what is it doing well, and what is getting in the way. Two or \
three short paragraphs at most."
        }

        AiAction::Critique => {
            "\
Critique the selected passage. Say what does not work and why. Be direct; the \
writer asked. Do not end with reassurance."
        }

        AiAction::Tighten => {
            "\
Tighten the selected passage. Keep the writer's voice, the register, and \
every fact — remove only what is doing no work.

Reply in exactly this form and nothing else:

REPLACEMENT
<the tightened passage, and only the passage>
END REPLACEMENT
NOTE
<one or two sentences on what you removed and why>

If the passage is already tight, omit the REPLACEMENT block entirely and say so \
in the NOTE."
        }

        AiAction::Continuity => {
            "\
Check the selected passage against the surrounding text for continuity: \
contradictions, drifting details, timing that does not work, things a \
character could not yet know. List only what you actually found. If you found \
nothing, say so in one sentence."
        }

        AiAction::Questions => {
            "\
Ask the writer the questions this passage raises — the ones a careful reader \
would want answered, and the ones the passage seems to be avoiding. Three to \
five questions, each a single sentence. No preamble."
        }

        AiAction::Alternatives => {
            "\
Offer one alternative version of the selected passage that takes a different \
approach — a different emphasis, order, or distance. Do not merely reword it.

Reply in exactly this form and nothing else:

REPLACEMENT
<the alternative passage, and only the passage>
END REPLACEMENT
NOTE
<one or two sentences on what the alternative does differently>"
        }
    }
}

/// A short label for the Margin note an action produces.
pub fn action_label(action: AiAction) -> &'static str {
    match action {
        AiAction::Review => "Review",
        AiAction::Critique => "Critique",
        AiAction::Tighten => "Tighten",
        AiAction::Continuity => "Continuity",
        AiAction::Questions => "Questions",
        AiAction::Alternatives => "Alternatives",
    }
}

/// Splits a reply into proposed replacement text and the accompanying note.
///
/// Models do not always honour a format exactly, so a reply without the markers
/// is treated as a note with no replacement rather than as an error. The cost of
/// being wrong here is asymmetric: a missing suggestion is a mild
/// disappointment, whereas mistaking commentary for replacement text would put
/// the model's prose into the manuscript.
pub fn split_replacement(reply: &str) -> (Option<String>, String) {
    const OPEN: &str = "REPLACEMENT";
    const CLOSE: &str = "END REPLACEMENT";
    const NOTE: &str = "NOTE";

    let Some(open_at) = reply.find(OPEN) else {
        return (None, reply.trim().to_string());
    };
    let after_open = open_at + OPEN.len();

    let Some(close_offset) = reply[after_open..].find(CLOSE) else {
        return (None, reply.trim().to_string());
    };
    let close_at = after_open + close_offset;

    let replacement = reply[after_open..close_at].trim().to_string();

    let rest = &reply[close_at + CLOSE.len()..];
    let note = match rest.find(NOTE) {
        Some(note_at) => rest[note_at + NOTE.len()..].trim(),
        None => rest.trim(),
    };

    // Anything before the marker is preamble the model was asked not to write;
    // it is kept rather than discarded, because throwing away model output the
    // writer might want to read is worse than a slightly untidy note.
    let preamble = reply[..open_at].trim();
    let note = if preamble.is_empty() {
        note.to_string()
    } else if note.is_empty() {
        preamble.to_string()
    } else {
        format!("{preamble}\n\n{note}")
    };

    if replacement.is_empty() {
        return (None, note);
    }
    (Some(replacement), note)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_an_instruction_and_a_label() {
        for action in AiAction::ALL {
            assert!(!action_instruction(action).trim().is_empty());
            assert!(!action_label(action).is_empty());
        }
    }

    #[test]
    fn the_base_prompt_states_who_owns_the_manuscript() {
        assert!(BASE_SYSTEM.contains("manuscript is theirs"));
        assert!(BASE_SYSTEM.contains("not rewriting it"));
    }

    #[test]
    fn editing_actions_ask_for_the_replacement_format() {
        for action in AiAction::ALL.into_iter().filter(AiAction::proposes_an_edit) {
            let instruction = action_instruction(action);
            assert!(instruction.contains("REPLACEMENT"), "{action:?}");
            assert!(instruction.contains("END REPLACEMENT"), "{action:?}");
        }
    }

    #[test]
    fn every_builtin_profile_is_complete() {
        assert!(BUILTIN_PROFILES.len() >= 6);
        for profile in BUILTIN_PROFILES {
            assert!(!profile.name.is_empty());
            assert!(!profile.description.is_empty());
            assert!(profile.system_prompt.len() > 40);
            assert!(["selection", "page", "chapter"].contains(&profile.context_policy));
        }
    }

    #[test]
    fn a_well_formed_reply_splits_into_replacement_and_note() {
        let reply =
            "REPLACEMENT\nThe road had been salt.\nEND REPLACEMENT\nNOTE\nRemoved the hedge.";
        let (replacement, note) = split_replacement(reply);
        assert_eq!(replacement.as_deref(), Some("The road had been salt."));
        assert_eq!(note, "Removed the hedge.");
    }

    #[test]
    fn a_reply_with_no_markers_is_all_note_and_proposes_nothing() {
        // The safe direction: commentary must never be mistaken for text to
        // put into the manuscript.
        let reply = "This is already about as tight as it can get.";
        let (replacement, note) = split_replacement(reply);
        assert!(replacement.is_none());
        assert_eq!(note, reply);
    }

    #[test]
    fn an_unterminated_replacement_block_proposes_nothing() {
        let reply = "REPLACEMENT\nHalf an answer and then the stream stopped";
        let (replacement, note) = split_replacement(reply);
        assert!(
            replacement.is_none(),
            "a truncated reply must not become an edit"
        );
        assert!(note.contains("Half an answer"));
    }

    #[test]
    fn an_empty_replacement_block_proposes_nothing() {
        let reply = "REPLACEMENT\n\nEND REPLACEMENT\nNOTE\nAlready tight.";
        let (replacement, note) = split_replacement(reply);
        assert!(replacement.is_none());
        assert_eq!(note, "Already tight.");
    }

    #[test]
    fn preamble_the_model_was_asked_not_to_write_is_kept() {
        let reply =
            "Happy to help!\nREPLACEMENT\nTight text.\nEND REPLACEMENT\nNOTE\nCut two words.";
        let (replacement, note) = split_replacement(reply);
        assert_eq!(replacement.as_deref(), Some("Tight text."));
        assert!(note.contains("Happy to help!"));
        assert!(note.contains("Cut two words."));
    }

    #[test]
    fn a_missing_note_section_still_yields_the_replacement() {
        let reply = "REPLACEMENT\nJust the text.\nEND REPLACEMENT";
        let (replacement, note) = split_replacement(reply);
        assert_eq!(replacement.as_deref(), Some("Just the text."));
        assert_eq!(note, "");
    }
}
