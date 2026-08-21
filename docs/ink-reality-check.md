# Ink Intelligence — Reality Check

> The goal is not "OCR exists".
> The goal is: write naturally in the Margin, and Grimoire quietly understands it.

This document records an audit of the Ink Intelligence implementation against
real-world behaviour, the fixes that came out of it, and — just as important —
what has *not* been proven yet.

## 1. Baseline

Verified at the start of this pass, on `rebuild/grimoire-next` at `29e5bd4`:

```
cargo test    374 passed
pnpm test      86 passed
git status     clean
```

The reported baseline was accurate.

## 2. What this pass is, and is not

Tests were green and the architecture was sound, but the path had only ever been
exercised through mocks that fed it *consistent* inputs. The audit therefore
looked for the class of defect a mock cannot catch: places where the real
frontend hands the real backend something different from what the tests hand it.

It found several, including one that silently disabled automatic recognition
entirely.

## 3. Audit findings

Ordered by how much they cost the writer.

### P0 — the feature did not work in practice

**F1. Automatic recognition only ever saw the last stroke.**
`InkStore.commitStroke` scheduled recognition with `[stroke]` — the single
stroke just drawn — while the eraser path correctly passed the whole note. Two
consequences, both fatal:

- The raster sent to the model contained one stroke, not the note.
- The snapshot's content hash covered one stroke, while the queue's stale check
  re-read *all* the note's strokes. For any note with more than one stroke the
  two hashes could never match, so every automatic result was discarded as
  stale and the note settled on `stale` forever.

Single-stroke notes worked. Handwriting does not consist of single strokes.
Every existing test passed a self-consistent snapshot, so none of them could see
this.

**F2. Automatic recognition never told the UI it had finished.**
The queue wrote its result to SQLite and emitted nothing. Only the *manual*
command emitted `ink:recognized`, and no frontend code listened for even that.
The Margin's status came from a one-shot fetch when the Page opened, so an
automatically recognised note sat under an optimistic "Recognizing…" until the
writer navigated away and back.

### P1 — recognition quality

**F3. The raster was never upscaled.** The scale factor was
`min(1, MAX_DIM / longest_side)` — capped at 1, with no floor. A one-line note
in a 280px margin produced roughly a 250×90px image. That is far below what a
vision model needs for handwriting, and worse for CJK, where character detail
is the whole signal.

**F4. Highlighter strokes were rasterised as opaque black.** Every stroke was
drawn with the same `#111111` at its full stored width, so a 10px highlighter
became a thick black bar straight through the handwriting it was meant to mark.
The SVG renderer layers highlighters *behind* pens; the rasteriser drew them in
array order, on top.

**F5. The recognition language setting did nothing.** `LanguageHint` was parsed
from settings, placed on `InkRecognitionRequest`, and then ignored by
`VisionRecognizer`, which built its prompt from two constants.

**F6. An unsuitable model produced an opaque error.** No capability check and no
error mapping, so choosing a text-only model surfaced whatever JSON the provider
returned.

### P2 — robustness

**F7. Lost pointer capture could wedge the surface.** `InkSurface` handled
`pointerup` and `pointercancel` but not `lostpointercapture`. Any path that
dropped capture without one of those two events left `drawingPointerId` set —
and because `onPointerDown` returns early when it is non-null, the surface would
then refuse *every* subsequent stroke until the component was recreated.

**F8. The eraser took pointer capture and never released it.**

**F9. Pressure was never captured.** `renderWidth` had a considered pressure
model; `InkSurface` never read `event.pressure`, so it was dead code and every
stroke rendered at its base width.

**F10. Automatic recognition ignored the concurrency limit.** `recognize_now`
acquired a semaphore permit; `schedule_auto` — the path that actually runs in
normal use — called `run_recognition` directly. `MAX_CONCURRENCY` bounded only
the manual path.

**F11. A failure with no transcript showed no reason.** The error paragraph was
nested inside the transcript panel, which only renders when `recognizedText` is
non-empty. A note that failed on its first recognition showed the words
"Recognition failed" and nothing else, anywhere.

### P3 — measurement

**F12. There was no way to measure accuracy.** No fixtures, no CER, no latency
record — so prompt and raster changes could only be judged by impression.

## 4. Raster strategy

Chosen after the F3/F4 fixes, and documented here because it is the parameter
most likely to be re-tuned:

```
padding          24px (unchanged)
minimum longest side   768px   — upscale small notes into legibility
maximum longest side  2048px   — bound cost and request size
minimum short side     64px    — a single low line still gets vertical room
background       opaque #ffffff, never the live theme
pen ink          #111111
highlighter      #c8c8c8 wash, drawn first, behind the pen strokes
```

The floor matters more than the ceiling. Margin handwriting is physically small;
without upscaling, the model's input was a thumbnail.

## 5. Evaluation harness

`tests/ink-recognition-eval.test.ts` runs the deterministic half: fixtures of
synthetic vector handwriting in `tests/fixtures/ink/`, scored with Character
Error Rate. CER is computed over Unicode code points, never whitespace tokens,
so Chinese is measured correctly; WER is reported for Latin-script fixtures
only, where word boundaries are real.

The harness scores a *recognizer function*, so the same fixtures can be run
against a mock (deterministic, in CI) or a live provider (manual, with
credentials). See `docs/ink-live-smoke.md` for the live path.

## 6. What has NOT been validated

Stated plainly, because the temptation to imply otherwise is the whole risk of a
document like this.

**No physical hardware was used.** This pass ran in a terminal with no pen
digitiser and no interactive session. Nothing below has been observed on real
hardware:

- Surface Pen input of any kind — pressure, tilt, barrel button, eraser end
- Palm rejection with a hand actually resting on the glass
- WebView2's real Pointer Event sequencing, including coalescing behaviour
- Windows display scaling at 100/125/150/175/200%
- Portrait orientation and the Margin overlay on a Surface Go-class device
- Perceived latency and whether the 1.5s debounce feels right while writing

The pointer fixes in this pass are reasoned from the Pointer Events spec and
covered by unit tests. They are not hardware-validated.

**No live vision model was called.** No provider credential was available to
this session, and none was requested — using the writer's stored API key
without being asked is not a validation step. So:

- Real recognition accuracy is unmeasured. No CER against a real model exists.
- Real latency is unmeasured.
- The prompt has not been tested against real handwriting, only reasoned about.
- Real provider failure shapes (401/429/malformed output) are covered by unit
  tests against stubs, not against a real endpoint.

The live recognition path is **not proven**. The fixes above make it
*capable* of working; they do not demonstrate that it does.

## 7. How to run the live check

See `docs/ink-live-smoke.md`. It needs a configured, vision-capable
OpenAI-compatible provider and is opt-in via an environment variable, so it
never runs in CI and never spends money by accident.

## 8. Recommended next iteration

1. Run the live smoke on real hardware with a real pen and record CER per
   fixture category. Nothing else on this list is worth doing first.
2. Tune the raster floor against measured CER rather than the reasoning in §4.
3. Revisit the 1.5s debounce only after watching someone write with it.
4. Audit the `y`-is-absolute-pixels coordinate model against real DPI changes;
   it is sound under width changes, but vertical drift under font-scale or
   theme changes has not been observed either way.
