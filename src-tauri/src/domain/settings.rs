//! Application settings.
//!
//! Every field carries a `serde` default, so a settings row written by an older
//! build loads cleanly into a newer one and gains the new fields at their
//! defaults. That is deliberate: adding a preference should never require a
//! schema migration, and must never be able to fail a user's launch.
//!
//! Values are validated on the way in rather than trusted, because these end up
//! as CSS custom properties and as request parameters.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    // --- Appearance ---
    pub theme: String,
    pub manuscript_face: String,
    pub manuscript_size: f64,
    pub manuscript_leading: f64,
    pub manuscript_paragraph_gap: f64,
    pub manuscript_measure: f64,

    // --- Editor ---
    /// How long typing must pause before a save is queued.
    pub autosave_debounce_ms: i64,
    /// How long a continuous editing run may go without a checkpoint revision.
    pub revision_interval_seconds: i64,
    pub spellcheck: bool,

    // --- AI ---
    /// AI is off until the user configures it. Nothing is sent anywhere before.
    pub ai_enabled: bool,
    /// Ceiling on how much manuscript text may accompany one AI request.
    pub ai_context_budget_chars: i64,

    // --- Ink intelligence ---
    /// Whether handwriting is recognised automatically after the writer pauses.
    /// Off by default: recognition sends ink to a remote model, and that must be
    /// an explicit choice rather than a silent one.
    pub ink_auto_recognition: bool,
    /// A specific model to use for handwriting recognition, overriding the
    /// configured provider's model. Empty means "use the provider's model"; set
    /// it when the writer has a vision-specific model distinct from the one used
    /// for manuscript-reading actions.
    pub ink_recognition_model: String,
    /// A language hint for recognition: `auto` (the default) or a BCP-47 tag.
    pub ink_recognition_language: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            manuscript_face: "serif".into(),
            manuscript_size: 1.125,
            manuscript_leading: 1.7,
            manuscript_paragraph_gap: 0.85,
            manuscript_measure: 36.0,

            autosave_debounce_ms: 900,
            revision_interval_seconds: 300,
            spellcheck: true,

            ai_enabled: false,
            ai_context_budget_chars: 8_000,

            ink_auto_recognition: false,
            ink_recognition_model: String::new(),
            ink_recognition_language: "auto".into(),
        }
    }
}

const THEMES: [&str; 5] = ["system", "paper", "light", "dark", "night"];
const FACES: [&str; 3] = ["serif", "sans", "mono"];

impl AppSettings {
    /// Clamps every value into a range the UI can actually render.
    ///
    /// Applied on load as well as on save, so a hand-edited or corrupted row
    /// produces a usable app rather than an unreadable one.
    pub fn sanitised(mut self) -> Self {
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        if !FACES.contains(&self.manuscript_face.as_str()) {
            self.manuscript_face = "serif".into();
        }

        self.manuscript_size = clamp(self.manuscript_size, 0.875, 1.75);
        self.manuscript_leading = clamp(self.manuscript_leading, 1.2, 2.4);
        self.manuscript_paragraph_gap = clamp(self.manuscript_paragraph_gap, 0.0, 2.0);
        self.manuscript_measure = clamp(self.manuscript_measure, 24.0, 60.0);

        // A debounce below ~200ms writes on nearly every keystroke; above a few
        // seconds it stops feeling like autosave.
        self.autosave_debounce_ms = self.autosave_debounce_ms.clamp(200, 5_000);
        self.revision_interval_seconds = self.revision_interval_seconds.clamp(60, 3_600);
        self.ai_context_budget_chars = self.ai_context_budget_chars.clamp(500, 200_000);

        // A language hint is either `auto` or a short tag; trim it so a stray
        // space does not turn "auto" into something the recognizer cannot parse.
        self.ink_recognition_language = self.ink_recognition_language.trim().to_string();
        if self.ink_recognition_language.is_empty() {
            self.ink_recognition_language = "auto".into();
        }
        self.ink_recognition_model = self.ink_recognition_model.trim().to_string();

        self
    }
}

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value.is_nan() {
        low
    } else {
        value.clamp(low, high)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_is_off_until_the_user_turns_it_on() {
        assert!(!AppSettings::default().ai_enabled);
    }

    #[test]
    fn defaults_survive_their_own_sanitiser() {
        let defaults = AppSettings::default();
        assert_eq!(defaults.clone().sanitised(), defaults);
    }

    #[test]
    fn unknown_themes_and_faces_fall_back() {
        let settings = AppSettings {
            theme: "neon".into(),
            manuscript_face: "comic".into(),
            ..Default::default()
        }
        .sanitised();
        assert_eq!(settings.theme, "system");
        assert_eq!(settings.manuscript_face, "serif");
    }

    #[test]
    fn absurd_typography_is_clamped_to_something_readable() {
        let settings = AppSettings {
            manuscript_size: 400.0,
            manuscript_leading: 0.1,
            manuscript_measure: 1_000.0,
            ..Default::default()
        }
        .sanitised();

        assert!(settings.manuscript_size <= 1.75);
        assert!(settings.manuscript_leading >= 1.2);
        assert!(settings.manuscript_measure <= 60.0);
    }

    #[test]
    fn nan_does_not_survive() {
        let settings = AppSettings {
            manuscript_size: f64::NAN,
            ..Default::default()
        }
        .sanitised();
        assert!(settings.manuscript_size.is_finite());
    }

    #[test]
    fn autosave_debounce_stays_in_a_sane_band() {
        assert_eq!(
            AppSettings {
                autosave_debounce_ms: 1,
                ..Default::default()
            }
            .sanitised()
            .autosave_debounce_ms,
            200
        );
        assert_eq!(
            AppSettings {
                autosave_debounce_ms: 900_000,
                ..Default::default()
            }
            .sanitised()
            .autosave_debounce_ms,
            5_000
        );
    }

    #[test]
    fn a_row_written_by_an_older_build_loads_and_gains_new_fields() {
        // Only two keys, as an early version might have stored.
        let stored = r#"{"theme":"night","manuscriptSize":1.25}"#;
        let settings: AppSettings = serde_json::from_str(stored).unwrap();
        assert_eq!(settings.theme, "night");
        assert_eq!(settings.manuscript_size, 1.25);
        // Everything else arrives at its default rather than failing the load.
        assert_eq!(settings.manuscript_face, "serif");
        assert!(!settings.ai_enabled);
    }

    #[test]
    fn unknown_keys_from_a_newer_build_do_not_break_the_load() {
        let stored = r#"{"theme":"paper","somethingFromTheFuture":true}"#;
        let settings: AppSettings = serde_json::from_str(stored).unwrap();
        assert_eq!(settings.theme, "paper");
    }

    #[test]
    fn settings_use_camel_case_on_the_wire() {
        let json = serde_json::to_value(AppSettings::default()).unwrap();
        assert!(json.get("manuscriptSize").is_some());
        assert!(json.get("aiEnabled").is_some());
    }
}
