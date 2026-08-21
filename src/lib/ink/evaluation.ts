/**
 * Handwriting recognition evaluation.
 *
 * Recognition quality cannot be judged by looking at one transcript and feeling
 * good about it. A prompt change, a raster change, or a different model has to
 * be answerable with a number, against the same samples, every time — otherwise
 * "the recognition got better" is an opinion.
 *
 * # Character Error Rate is the primary metric
 *
 * CER is edit distance over *Unicode code points*, divided by the length of the
 * ground truth. It is the right primary metric here because Grimoire's
 * handwriting is substantially Chinese, and Chinese has no word delimiters:
 * splitting on whitespace would score a whole line as one token, so a single
 * wrong character would read as 100% error. Word Error Rate is reported too,
 * but only for samples whose script actually has word boundaries — reporting it
 * for Chinese would be a number that looks meaningful and is not.
 *
 * # What this module does not do
 *
 * It does not render, call a provider, or know what a recognizer is. It takes
 * samples and a function that turns one into a transcript, so the same fixtures
 * can be scored against a mock (deterministic, in CI) or a real model (manual,
 * with credentials). See `docs/ink-live-smoke.md`.
 */

import type { InkStroke } from '$lib/types/ink';

/** Which part of the sample matrix a fixture covers, for grouped reporting. */
export type EvaluationCategory =
  | 'chinese'
  | 'english'
  | 'mixed'
  | 'numeric'
  | 'punctuation'
  | 'multiline'
  | 'small'
  | 'large'
  | 'messy'
  | 'long';

/** One handwriting sample with the text it is known to say. */
export type EvaluationSample = {
  id: string;
  /** What is actually written, exactly. The scoring baseline. */
  groundTruth: string;
  /** BCP-47 tag of the sample's dominant language. */
  language: string;
  categories: EvaluationCategory[];
  /** The surface width the stroke coordinates were normalised against. */
  surfaceWidth: number;
  strokes: InkStroke[];
  /**
   * Whether the stroke geometry came from a real pen.
   *
   * Synthetic geometry is fine for exercising the harness and the scoring, and
   * useless for judging a model: a real transcript against fake handwriting
   * measures nothing. A live run must report this, so a good-looking CER over
   * synthetic samples can never be mistaken for evidence.
   */
  provenance: 'pen-capture' | 'synthetic';
};

/** One sample's result. */
export type SampleResult = {
  sampleId: string;
  groundTruth: string;
  transcript: string | null;
  /** Character Error Rate, or null when recognition failed outright. */
  cer: number | null;
  /** Word Error Rate — only for scripts with word boundaries. */
  wer: number | null;
  language: string;
  categories: EvaluationCategory[];
  provenance: EvaluationSample['provenance'];
  /** Milliseconds from handing off the sample to receiving a transcript. */
  latencyMs: number;
  /** The dimensions the sample was rendered at, when the runner reports them. */
  raster: { width: number; height: number } | null;
  error: string | null;
};

/** The aggregate of a whole run. */
export type EvaluationReport = {
  results: SampleResult[];
  /** Samples that produced a transcript, over samples attempted. */
  completed: number;
  attempted: number;
  /** Mean CER across completed samples, or null when none completed. */
  meanCer: number | null;
  /** Median and worst latency, in milliseconds. */
  medianLatencyMs: number | null;
  worstLatencyMs: number | null;
  /** Mean CER per category, for finding which kind of handwriting fails. */
  byCategory: Record<string, { count: number; meanCer: number | null }>;
  /** True when every sample used synthetic geometry — see {@link EvaluationSample}. */
  syntheticOnly: boolean;
};

/** What a runner returns for one sample. */
export type RecognitionOutcome = {
  transcript: string | null;
  raster?: { width: number; height: number } | null;
  error?: string | null;
};

/** The function under evaluation: one sample in, one transcript out. */
export type SampleRecognizer = (sample: EvaluationSample) => Promise<RecognitionOutcome>;

/**
 * Splits text into Unicode code points.
 *
 * `String.prototype.split('')` splits into UTF-16 code *units*, which cuts
 * astral characters in half and would make the error rate for anything outside
 * the BMP nonsense. Iterating the string yields code points.
 */
export function characters(text: string): string[] {
  return [...text];
}

/**
 * Normalises text for comparison: trims, and collapses runs of whitespace to a
 * single space.
 *
 * Line breaks are meaningful in a margin note and the prompt asks for them to
 * be preserved, but a model that renders a deliberate line break as a space
 * should not be scored as having got every following character wrong. So the
 * comparison ignores whitespace *shape* while still counting whitespace that
 * is present or missing between words.
 */
export function normalizeForScoring(text: string): string {
  return text.trim().replace(/\s+/g, ' ');
}

/**
 * Levenshtein distance between two token sequences.
 *
 * Two rows rather than a full matrix — the sequences here are short, but the
 * full matrix buys nothing since the alignment itself is never needed.
 */
export function editDistance(a: readonly string[], b: readonly string[]): number {
  if (a.length === 0) return b.length;
  if (b.length === 0) return a.length;

  let previous = Array.from({ length: b.length + 1 }, (_, i) => i);
  let current = new Array<number>(b.length + 1);

  for (let i = 1; i <= a.length; i++) {
    current[0] = i;
    for (let j = 1; j <= b.length; j++) {
      const substitution = previous[j - 1]! + (a[i - 1] === b[j - 1] ? 0 : 1);
      const deletion = previous[j]! + 1;
      const insertion = current[j - 1]! + 1;
      current[j] = Math.min(substitution, deletion, insertion);
    }
    [previous, current] = [current, previous];
  }

  return previous[b.length]!;
}

/**
 * Character Error Rate: edit distance over code points, divided by the ground
 * truth's length.
 *
 * Not clamped to 1. A recognizer that hallucinates three lines where one was
 * written has a CER above 1, and flattening that to "100% wrong" would hide
 * how wrong it was — which is exactly the failure worth seeing.
 *
 * An empty ground truth scores 0 when the transcript is also empty and 1 when
 * it is not, rather than dividing by zero.
 */
export function characterErrorRate(groundTruth: string, transcript: string): number {
  const reference = characters(normalizeForScoring(groundTruth));
  const hypothesis = characters(normalizeForScoring(transcript));
  if (reference.length === 0) return hypothesis.length === 0 ? 0 : 1;
  return editDistance(reference, hypothesis) / reference.length;
}

/**
 * Whether a language has word boundaries a whitespace split can find.
 *
 * Chinese, Japanese and Thai do not. Reporting WER for them would produce a
 * number that looks comparable to the English one and is not.
 */
export function hasWordBoundaries(language: string): boolean {
  const base = language.toLowerCase().split('-')[0];
  return base !== 'zh' && base !== 'ja' && base !== 'th';
}

/**
 * Word Error Rate, for space-delimited scripts only. Returns null for a
 * language where whitespace does not delimit words.
 */
export function wordErrorRate(
  groundTruth: string,
  transcript: string,
  language: string
): number | null {
  if (!hasWordBoundaries(language)) return null;
  const reference = normalizeForScoring(groundTruth).split(' ').filter(Boolean);
  const hypothesis = normalizeForScoring(transcript).split(' ').filter(Boolean);
  if (reference.length === 0) return hypothesis.length === 0 ? 0 : 1;
  return editDistance(reference, hypothesis) / reference.length;
}

/**
 * Runs every sample through a recognizer and scores the results.
 *
 * Samples run in sequence, not in parallel: a live run against a real provider
 * should not open a dozen connections at once, and latency measured under
 * self-inflicted contention would not be the latency a writer experiences.
 *
 * A sample that throws is recorded as a failure and the run continues — one
 * unreadable sample must not cost the whole report.
 */
export async function evaluate(
  samples: readonly EvaluationSample[],
  recognize: SampleRecognizer
): Promise<EvaluationReport> {
  const results: SampleResult[] = [];

  for (const sample of samples) {
    const started = Date.now();
    let outcome: RecognitionOutcome;
    try {
      outcome = await recognize(sample);
    } catch (error) {
      outcome = {
        transcript: null,
        error: error instanceof Error ? error.message : String(error)
      };
    }
    const latencyMs = Date.now() - started;
    const transcript = outcome.transcript;

    results.push({
      sampleId: sample.id,
      groundTruth: sample.groundTruth,
      transcript,
      cer: transcript === null ? null : characterErrorRate(sample.groundTruth, transcript),
      wer:
        transcript === null
          ? null
          : wordErrorRate(sample.groundTruth, transcript, sample.language),
      language: sample.language,
      categories: sample.categories,
      provenance: sample.provenance,
      latencyMs,
      raster: outcome.raster ?? null,
      error: outcome.error ?? null
    });
  }

  return summarize(results, samples);
}

/** Aggregates per-sample results into the report. */
function summarize(
  results: SampleResult[],
  samples: readonly EvaluationSample[]
): EvaluationReport {
  const completed = results.filter((r) => r.cer !== null);
  const latencies = results.map((r) => r.latencyMs).sort((a, b) => a - b);

  const byCategory: Record<string, { count: number; meanCer: number | null }> = {};
  for (const result of results) {
    for (const category of result.categories) {
      const bucket = (byCategory[category] ??= { count: 0, meanCer: null });
      bucket.count += 1;
    }
  }
  for (const category of Object.keys(byCategory)) {
    const scored = completed.filter((r) => r.categories.includes(category as EvaluationCategory));
    byCategory[category]!.meanCer = scored.length > 0 ? mean(scored.map((r) => r.cer!)) : null;
  }

  return {
    results,
    completed: completed.length,
    attempted: results.length,
    meanCer: completed.length > 0 ? mean(completed.map((r) => r.cer!)) : null,
    medianLatencyMs: latencies.length > 0 ? median(latencies) : null,
    worstLatencyMs: latencies.length > 0 ? latencies[latencies.length - 1]! : null,
    byCategory,
    syntheticOnly: samples.length > 0 && samples.every((s) => s.provenance === 'synthetic')
  };
}

function mean(values: number[]): number {
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

/** The median of an already-sorted array. */
function median(sorted: number[]): number {
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1]! + sorted[middle]!) / 2
    : sorted[middle]!;
}

/**
 * Renders a report as plain text, for a live run's console output.
 *
 * Leads with the provenance warning when it applies, because a reader skimming
 * a table of low error rates needs to know before the numbers whether they mean
 * anything.
 */
export function formatReport(report: EvaluationReport): string {
  const lines: string[] = [];

  if (report.syntheticOnly) {
    lines.push(
      'WARNING: every sample used synthetic stroke geometry. These numbers',
      'exercise the harness; they say nothing about real handwriting.',
      ''
    );
  }

  lines.push(`samples    ${report.completed}/${report.attempted} recognised`);
  lines.push(`mean CER   ${format(report.meanCer)}`);
  lines.push(`latency    median ${report.medianLatencyMs ?? '-'}ms, worst ${report.worstLatencyMs ?? '-'}ms`);
  lines.push('');

  for (const [category, stats] of Object.entries(report.byCategory).sort()) {
    lines.push(`  ${category.padEnd(12)} n=${String(stats.count).padEnd(3)} CER ${format(stats.meanCer)}`);
  }
  lines.push('');

  for (const result of report.results) {
    const score = result.cer === null ? 'FAILED' : `CER ${format(result.cer)}`;
    lines.push(`  ${result.sampleId.padEnd(24)} ${score.padEnd(12)} ${result.latencyMs}ms`);
    if (result.error) lines.push(`      ${result.error}`);
    else if (result.cer !== null && result.cer > 0) {
      lines.push(`      expected: ${result.groundTruth}`);
      lines.push(`      got:      ${result.transcript}`);
    }
  }

  return lines.join('\n');
}

function format(value: number | null): string {
  return value === null ? '-' : value.toFixed(3);
}
