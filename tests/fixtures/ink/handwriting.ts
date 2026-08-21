/**
 * The handwriting evaluation fixture set.
 *
 * # Read this before trusting a number produced from these
 *
 * The **ground truth is real** — these are the sentences a writer would put in
 * a margin, in the languages Grimoire is built for. The **stroke geometry is
 * synthetic**: procedurally generated polylines with the right size, line
 * count, spacing and jitter for each sample, but they do not spell anything.
 *
 * That is deliberate and it is a limitation, not a shortcut being hidden. This
 * fixture set was written without a pen digitiser available, and inventing
 * vector data that convincingly renders 这里的转折太突然了 by hand is not
 * possible; pretending otherwise would produce a fixture set that *looks*
 * authoritative and silently invalidates every measurement taken from it.
 *
 * So the samples serve two purposes today:
 *
 * 1. They exercise the harness, the scoring, and the raster budget — all of
 *    which are real logic with real correctness properties.
 * 2. They define the matrix, so that when a pen is available each sample can be
 *    rewritten with captured strokes and the ground truth already agreed.
 *
 * Every sample is marked `provenance: 'synthetic'`, the report carries that
 * flag through, and `formatReport` leads with a warning when a whole run is
 * synthetic. A live run over these samples measures nothing about a model.
 *
 * # Replacing a sample with real handwriting
 *
 * Write the sample's ground truth into a margin ink note, then export the
 * note's strokes and paste them in with `provenance: 'pen-capture'`. See
 * `docs/ink-live-smoke.md`.
 */

import type { EvaluationSample } from '$lib/ink/evaluation';
import type { InkStroke } from '$lib/types/ink';

/** The margin width these samples are normalised against, in logical pixels. */
const SURFACE_WIDTH = 280;

/**
 * A deterministic pseudo-random source.
 *
 * Fixtures must be byte-identical between runs, or a raster-dimension
 * assertion becomes flaky and the whole harness becomes untrustworthy.
 */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    // xorshift32: small, deterministic, and adequate for jitter.
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    return state / 0xffffffff;
  };
}

type LineSpec = {
  /** Where the line starts and ends, as a fraction of the surface width. */
  fromX: number;
  toX: number;
  /** The line's baseline, in surface pixels from the top. */
  y: number;
  /** Roughly how tall the characters are, in surface pixels. */
  height: number;
  /** How many pen-down marks the line is made of. */
  marks: number;
  /** How far the hand wandered. 0 is copperplate, 1 is hurried. */
  messiness: number;
};

/**
 * Builds the strokes for one line of pretend handwriting.
 *
 * Each mark is a short wandering polyline in the band above the baseline, which
 * gives the rasterizer realistic bounds, stroke counts and point density to
 * work with — the properties the raster budget and the meaningfulness check
 * actually depend on.
 */
function line(spec: LineSpec, random: () => number, width: number): InkStroke[] {
  const strokes: InkStroke[] = [];
  const span = spec.toX - spec.fromX;
  const markWidth = span / spec.marks;

  for (let i = 0; i < spec.marks; i++) {
    const startX = spec.fromX + i * markWidth;
    const points: InkStroke['points'] = [];
    const samples = 6;
    for (let s = 0; s <= samples; s++) {
      const t = s / samples;
      const wobble = (random() - 0.5) * spec.messiness * 0.4;
      points.push({
        x: startX + markWidth * (0.15 + 0.7 * t) + wobble * markWidth,
        // A mark rises and falls within the character band.
        y:
          spec.y -
          Math.sin(t * Math.PI) * spec.height * (0.6 + random() * 0.4) +
          (random() - 0.5) * spec.messiness * spec.height * 0.3,
        pressure: 0.4 + random() * 0.35
      });
    }
    strokes.push({
      id: `s-${strokes.length}-${Math.round(startX * 10000)}`,
      tool: 'pen',
      color: 'ink-primary',
      width,
      points,
      createdAt: '2026-01-01T00:00:00Z'
    });
  }

  return strokes;
}

type SampleSpec = {
  id: string;
  groundTruth: string;
  language: string;
  categories: EvaluationSample['categories'];
  seed: number;
  lines: LineSpec[];
  /** Pen width in surface pixels. */
  penWidth?: number;
};

function sample(spec: SampleSpec): EvaluationSample {
  const random = seeded(spec.seed);
  const strokes = spec.lines.flatMap((l) => line(l, random, spec.penWidth ?? 2));
  return {
    id: spec.id,
    groundTruth: spec.groundTruth,
    language: spec.language,
    categories: spec.categories,
    surfaceWidth: SURFACE_WIDTH,
    strokes,
    provenance: 'synthetic'
  };
}

/** One line of ordinary margin handwriting, for reuse across samples. */
function oneLine(y: number, marks: number, height = 18, messiness = 0.3): LineSpec {
  return { fromX: 0.05, toX: 0.9, y, height, marks, messiness };
}

/**
 * The sample matrix.
 *
 * Covers the shapes a margin actually sees: both languages and their mixture,
 * digits and punctuation, one to three lines, small and large hands, tidy and
 * hurried, short marginalia and a longer note.
 */
export const HANDWRITING_SAMPLES: EvaluationSample[] = [
  sample({
    id: 'chinese-short',
    groundTruth: '这里的转折太突然了',
    language: 'zh-CN',
    categories: ['chinese'],
    seed: 1,
    lines: [oneLine(40, 9)]
  }),
  sample({
    id: 'chinese-two-line',
    groundTruth: '这一段的节奏需要放慢\n读者还没准备好',
    language: 'zh-CN',
    categories: ['chinese', 'multiline'],
    seed: 2,
    lines: [oneLine(36, 10), oneLine(70, 7)]
  }),
  sample({
    id: 'chinese-with-digits',
    groundTruth: '第3章太长了，考虑拆成两章',
    language: 'zh-CN',
    categories: ['chinese', 'numeric', 'punctuation'],
    seed: 3,
    lines: [oneLine(40, 13)]
  }),
  sample({
    id: 'english-short',
    groundTruth: 'move this later',
    language: 'en-US',
    categories: ['english'],
    seed: 4,
    lines: [oneLine(40, 3)]
  }),
  sample({
    id: 'english-three-line',
    groundTruth: 'the salt road never\nappears again after\nthis chapter',
    language: 'en-US',
    categories: ['english', 'multiline', 'long'],
    seed: 5,
    lines: [oneLine(32, 4), oneLine(62, 3), oneLine(92, 2)]
  }),
  sample({
    id: 'english-punctuation',
    groundTruth: "Why now? She hasn't earned this — cut it.",
    language: 'en-US',
    categories: ['english', 'punctuation'],
    seed: 6,
    lines: [oneLine(36, 5), oneLine(66, 4)]
  }),
  sample({
    id: 'mixed-languages',
    groundTruth: '这个 pacing 的问题在 chapter 4 最明显',
    language: 'zh-CN',
    categories: ['mixed', 'chinese', 'english', 'numeric'],
    seed: 7,
    lines: [oneLine(36, 8), oneLine(66, 6)]
  }),
  sample({
    id: 'numbers-only',
    groundTruth: '1847, 1852, 1861',
    language: 'en-US',
    categories: ['numeric', 'punctuation'],
    seed: 8,
    lines: [oneLine(40, 6)]
  }),
  sample({
    id: 'small-hand',
    groundTruth: 'check the timeline here',
    language: 'en-US',
    categories: ['english', 'small'],
    seed: 9,
    // A cramped hand: half the character height, packed into less width.
    lines: [{ fromX: 0.05, toX: 0.55, y: 24, height: 9, marks: 5, messiness: 0.25 }],
    penWidth: 1.5
  }),
  sample({
    id: 'large-hand',
    groundTruth: 'NO',
    language: 'en-US',
    categories: ['english', 'large'],
    seed: 10,
    lines: [{ fromX: 0.1, toX: 0.85, y: 70, height: 52, marks: 2, messiness: 0.2 }],
    penWidth: 4
  }),
  sample({
    id: 'messy-hand',
    groundTruth: 'this whole scene is doing nothing',
    language: 'en-US',
    categories: ['english', 'messy', 'long'],
    seed: 11,
    // Written fast: fewer, longer, wilder marks.
    lines: [
      { fromX: 0.03, toX: 0.95, y: 34, height: 20, marks: 4, messiness: 0.9 },
      { fromX: 0.03, toX: 0.88, y: 64, height: 20, marks: 3, messiness: 0.95 }
    ]
  }),
  sample({
    id: 'long-note',
    groundTruth:
      '这里需要一个过渡段。\nThe reader has just been told two things at once,\n然后立刻又换了视角。',
    language: 'zh-CN',
    categories: ['mixed', 'chinese', 'english', 'multiline', 'long'],
    seed: 12,
    lines: [oneLine(30, 10), oneLine(58, 9), oneLine(86, 9)]
  })
];

/** Looks a sample up by id, for a test that needs one in particular. */
export function sampleById(id: string): EvaluationSample {
  const found = HANDWRITING_SAMPLES.find((s) => s.id === id);
  if (!found) throw new Error(`no evaluation sample named ${id}`);
  return found;
}
