# Ink — Live Recognition Smoke Test

Everything else in the recognition test suite runs against stubs. That proves
the code handles the shapes it is given; it proves nothing about whether a real
model can read real handwriting.

This is the one check that talks to a provider. It is `#[ignore]`d and
environment-gated, so it never runs in CI and never spends money by accident.

**As of this writing it has never been run.** No provider credential was
available to the session that wrote it. See §6 of
[ink-reality-check.md](ink-reality-check.md).

## What it proves, and what it does not

Exercises, end to end:

- the real prompt, including the language hint
- a real multimodal request through the real transport
- a real model reading a real handwriting image
- the structured-output parsing, against whatever the model actually returns
- the provider error mapping, if it fails

It does **not** cover persistence, the debounce, stale rejection, or the queue —
those have deterministic tests and do not need a live model.

## 1. Get a handwriting image

The image must be what Grimoire would actually send: cropped to the strokes,
padded, dark ink on an opaque white background, longest side between 1024 and
2048px. See `src/lib/ink/rasterize.ts` for the current budget.

Two ways to get one:

- **From the app.** Write a note in the Margin, then take the raster from the
  recognition request. This is the faithful option: it is the exact bitmap the
  rasterizer produces at the DPI and margin width you are actually using.
- **From a photograph.** Write on paper, photograph it, crop, and convert to
  greyscale on white. Good enough for a first check of a model's capability;
  not a substitute for testing the rasterizer's own output.

Note what the handwriting says — you will want to score it.

## 2. Run it

```bash
GRIMOIRE_LIVE_BASE_URL="https://api.example.com/v1" GRIMOIRE_LIVE_MODEL="your-vision-model" GRIMOIRE_LIVE_API_KEY="sk-..." GRIMOIRE_LIVE_PNG="/path/to/handwriting.png" cargo test --manifest-path src-tauri/Cargo.toml --lib live_recognition -- --ignored --nocapture
```

`--nocapture` matters: the transcript and latency are printed, not asserted,
because only someone who knows what the image says can judge the result.

Optional:

| Variable | Effect |
| --- | --- |
| `GRIMOIRE_LIVE_LANGUAGE` | A BCP-47 tag (`zh-CN`, `en-US`) or `auto`. Exercises the language hint. Defaults to `auto`. |
| `GRIMOIRE_LIVE_EXPECT` | The text the handwriting actually says. Turns the run into a pass/fail: the test computes CER and fails above 0.25. |

### Do not put your key in shell history

The key is read from the environment so it never becomes a command-line
argument. If your shell records history, set it from a file or a secret manager
rather than typing it inline.

## 3. Read the result

```
--- live recognition ---
model      your-vision-model
latency    1840ms
language   Some("zh-CN")
transcript 这里的转折太突然了
--- end ---
```

Things worth checking beyond "is it right":

- **Did it transcribe, or interpret?** A model that answers the question the
  note poses, or rewrites the sentence it criticises, is not a recognizer. The
  prompt forbids this explicitly; if it happens anyway, tighten
  `TRANSCRIBE_SYSTEM` in `src-tauri/src/ai/ink_recognition.rs`.
- **Did it preserve the language?** Mixed Chinese and English must both survive.
  A transcript translated into one language is a prompt failure.
- **Did it invent?** Illegible words should come back as `⍰`, not as a plausible
  guess.
- **Latency.** Above about three seconds, background recognition stops feeling
  like something that happens quietly while you write.

## 4. Measuring more than one sample

For a real accuracy number, use the harness rather than this test: the fixture
matrix is in `tests/fixtures/ink/handwriting.ts` and the scoring is in
`src/lib/ink/evaluation.ts`.

**The fixtures' stroke geometry is currently synthetic** — the ground truth is
real, but the strokes do not spell anything, so a run over them measures the
harness and not the model. Every sample is marked `provenance: 'synthetic'` and
the report leads with a warning to that effect.

To make them meaningful, replace samples with captured strokes: write each
sample's ground truth into a Margin ink note with a real pen, take the note's
strokes, and paste them in with `provenance: 'pen-capture'`. The matrix and the
ground truth are already agreed, so this is transcription work, not design work.

## 5. When the model refuses

A model that cannot accept images is detected from the refusal and reported as:

```
<model> cannot read handwriting. Choose a vision-capable model for
handwriting recognition in AI settings.
```

If a live run produces a generic provider error instead, the refusal wording did
not match — add the phrasing to `mentions_unsupported_image` in
`src-tauri/src/ai/recognizers.rs` and cover it with a test.
