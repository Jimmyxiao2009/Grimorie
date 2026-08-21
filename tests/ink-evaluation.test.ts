/**
 * Evaluation harness tests.
 *
 * The scoring is the part that has to be right: if CER is wrong, every
 * conclusion drawn from a live run is wrong in a way nobody would notice. So
 * the metrics are tested directly and hard, with particular attention to
 * Chinese — where a whitespace-based implementation would look fine on English
 * and be meaningless on the language most of these margins are written in.
 */

import { describe, expect, it } from 'vitest';

import {
  characterErrorRate,
  characters,
  editDistance,
  evaluate,
  formatReport,
  hasWordBoundaries,
  normalizeForScoring,
  wordErrorRate,
  type EvaluationSample
} from '$lib/ink/evaluation';
import { recognitionScale, strokeBounds } from '$lib/ink/rasterize';
import { HANDWRITING_SAMPLES, sampleById } from './fixtures/ink/handwriting';

describe('character splitting', () => {
  it('splits Chinese into characters, not bytes', () => {
    expect(characters('转折')).toEqual(['转', '折']);
  });

  it('keeps an astral character whole', () => {
    // Splitting on UTF-16 code units would cut this into two lone surrogates
    // and double its length, inflating every error rate that touched it.
    expect(characters('a🖋b')).toEqual(['a', '🖋', 'b']);
  });
});

describe('editDistance', () => {
  it('is zero for identical sequences', () => {
    expect(editDistance(['a', 'b', 'c'], ['a', 'b', 'c'])).toBe(0);
  });

  it('counts a substitution as one', () => {
    expect(editDistance(['a', 'b', 'c'], ['a', 'x', 'c'])).toBe(1);
  });

  it('counts insertions and deletions', () => {
    expect(editDistance(['a', 'b'], ['a', 'b', 'c'])).toBe(1);
    expect(editDistance(['a', 'b', 'c'], ['a', 'c'])).toBe(1);
  });

  it('handles an empty side', () => {
    expect(editDistance([], ['a', 'b'])).toBe(2);
    expect(editDistance(['a', 'b'], [])).toBe(2);
    expect(editDistance([], [])).toBe(0);
  });

  it('matches a known distance', () => {
    expect(editDistance(characters('kitten'), characters('sitting'))).toBe(3);
  });
});

describe('normalizeForScoring', () => {
  it('collapses whitespace so a line break is not scored as many errors', () => {
    // The prompt asks for line breaks to be preserved, but a model that
    // renders one as a space should lose a character, not the rest of the line.
    expect(normalizeForScoring('one\ntwo')).toBe('one two');
    expect(normalizeForScoring('  padded \n\n  text  ')).toBe('padded text');
  });
});

describe('characterErrorRate', () => {
  it('is zero for an exact transcript', () => {
    expect(characterErrorRate('这里的转折太突然了', '这里的转折太突然了')).toBe(0);
  });

  it('scores one wrong Chinese character as one character of error', () => {
    // The heart of why CER is the primary metric. A whitespace-token metric
    // would score this whole line as a single wrong token — 100% error for one
    // wrong character.
    const cer = characterErrorRate('这里的转折太突然了', '这里的转折太突然啦');
    expect(cer).toBeCloseTo(1 / 9, 5);
  });

  it('scores an English substitution proportionally', () => {
    expect(characterErrorRate('move this', 'move thas')).toBeCloseTo(1 / 9, 5);
  });

  it('is not clamped when a recognizer hallucinates', () => {
    // A model that invents three lines where one was written is worse than
    // "100% wrong", and flattening it to 1 would hide exactly that.
    const cer = characterErrorRate('no', 'no, and here is a great deal more text');
    expect(cer).toBeGreaterThan(1);
  });

  it('treats a missing transcript against empty truth as no error', () => {
    expect(characterErrorRate('', '')).toBe(0);
    expect(characterErrorRate('', 'invented')).toBe(1);
  });

  it('ignores line-break shape but not missing words', () => {
    expect(characterErrorRate('one\ntwo', 'one two')).toBe(0);
    expect(characterErrorRate('one two', 'onetwo')).toBeGreaterThan(0);
  });
});

describe('word error rate', () => {
  it('is reported for space-delimited languages', () => {
    expect(hasWordBoundaries('en-US')).toBe(true);
    expect(wordErrorRate('move this later', 'move that later', 'en-US')).toBeCloseTo(1 / 3, 5);
  });

  it('is withheld for languages with no word delimiters', () => {
    // Reporting WER for Chinese would produce a number that looks comparable
    // to the English one and is not.
    expect(hasWordBoundaries('zh-CN')).toBe(false);
    expect(hasWordBoundaries('ja')).toBe(false);
    expect(wordErrorRate('这里的转折', '这里的转机', 'zh-CN')).toBeNull();
  });
});

describe('the fixture matrix', () => {
  it('covers every part of the sample matrix', () => {
    const covered = new Set(HANDWRITING_SAMPLES.flatMap((s) => s.categories));
    for (const category of [
      'chinese',
      'english',
      'mixed',
      'numeric',
      'punctuation',
      'multiline',
      'small',
      'large',
      'messy',
      'long'
    ]) {
      expect(covered.has(category as never), `no sample covers ${category}`).toBe(true);
    }
  });

  it('declares its geometry synthetic, so no run can imply otherwise', () => {
    // If this ever fails it is because someone added a pen-captured sample,
    // which is the goal — but the report's warning logic depends on the flag
    // being honest, so it is asserted rather than assumed.
    expect(HANDWRITING_SAMPLES.every((s) => s.provenance === 'synthetic')).toBe(true);
  });

  it('is deterministic, so raster assertions cannot go flaky', () => {
    const first = sampleById('chinese-short').strokes[0]!.points[0]!;
    const again = sampleById('chinese-short').strokes[0]!.points[0]!;
    expect(first).toEqual(again);
  });

  it('produces samples the raster budget treats as real handwriting', () => {
    for (const sample of HANDWRITING_SAMPLES) {
      const bounds = strokeBounds(sample.strokes, sample.surfaceWidth);
      expect(bounds, `${sample.id} has no bounds`).not.toBeNull();

      const width = bounds!.maxX - bounds!.minX;
      const height = bounds!.maxY - bounds!.minY;
      const scale = recognitionScale(width, height);
      const longest = Math.max(width, height) * scale;

      // Every sample lands inside the raster budget: magnified enough to read,
      // never past the cap. The small-hand sample is the one that would have
      // been sent as a thumbnail before the floor existed.
      expect(longest, `${sample.id} renders too small`).toBeGreaterThanOrEqual(1023);
      expect(longest, `${sample.id} renders too large`).toBeLessThanOrEqual(2049);
    }
  });
});

describe('evaluate', () => {
  const twoSamples: EvaluationSample[] = [
    sampleById('chinese-short'),
    sampleById('english-short')
  ];

  it('scores a perfect recognizer at zero error', async () => {
    const report = await evaluate(twoSamples, async (sample) => ({
      transcript: sample.groundTruth,
      raster: { width: 1024, height: 300 }
    }));

    expect(report.attempted).toBe(2);
    expect(report.completed).toBe(2);
    expect(report.meanCer).toBe(0);
    expect(report.results[0]!.raster).toEqual({ width: 1024, height: 300 });
  });

  it('discriminates a degraded recognizer', async () => {
    // The property that makes the harness worth having: a worse transcript
    // must produce a worse number.
    const perfect = await evaluate(twoSamples, async (s) => ({ transcript: s.groundTruth }));
    const degraded = await evaluate(twoSamples, async (s) => ({
      // Drop the last character of every sample.
      transcript: [...s.groundTruth].slice(0, -1).join('')
    }));

    expect(degraded.meanCer!).toBeGreaterThan(perfect.meanCer!);
  });

  it('records a failure without losing the rest of the run', async () => {
    const report = await evaluate(twoSamples, async (sample) => {
      if (sample.id === 'chinese-short') throw new Error('the provider refused');
      return { transcript: sample.groundTruth };
    });

    expect(report.attempted).toBe(2);
    expect(report.completed).toBe(1);
    expect(report.results[0]!.cer).toBeNull();
    expect(report.results[0]!.error).toBe('the provider refused');
    // The mean is over what completed, not silently counting the failure as
    // perfect.
    expect(report.meanCer).toBe(0);
  });

  it('withholds WER for a Chinese sample but reports it for English', async () => {
    const report = await evaluate(twoSamples, async (s) => ({ transcript: s.groundTruth }));
    const chinese = report.results.find((r) => r.sampleId === 'chinese-short')!;
    const english = report.results.find((r) => r.sampleId === 'english-short')!;
    expect(chinese.wer).toBeNull();
    expect(english.wer).toBe(0);
  });

  it('breaks the score down by category', async () => {
    const report = await evaluate(HANDWRITING_SAMPLES, async (sample) => ({
      // Chinese comes back perfect; everything else loses a character.
      transcript: sample.categories.includes('chinese')
        ? sample.groundTruth
        : [...sample.groundTruth].slice(0, -1).join('')
    }));

    expect(report.byCategory['chinese']!.meanCer).toBe(0);
    expect(report.byCategory['english']!.meanCer).toBeGreaterThan(0);
  });

  it('measures latency per sample', async () => {
    const report = await evaluate([twoSamples[0]!], async (s) => {
      await new Promise((resolve) => setTimeout(resolve, 20));
      return { transcript: s.groundTruth };
    });
    expect(report.results[0]!.latencyMs).toBeGreaterThanOrEqual(15);
    expect(report.medianLatencyMs).not.toBeNull();
  });
});

describe('formatReport', () => {
  it('leads with a warning when every sample is synthetic', async () => {
    const report = await evaluate(HANDWRITING_SAMPLES, async (s) => ({
      transcript: s.groundTruth
    }));
    const text = formatReport(report);

    // A reader skimming a table of zeroes has to be told, before the numbers,
    // that they mean nothing about real handwriting.
    expect(text.startsWith('WARNING')).toBe(true);
    expect(text).toContain('synthetic stroke geometry');
  });

  it('shows the mismatch for a sample that scored badly', async () => {
    const report = await evaluate([sampleById('english-short')], async () => ({
      transcript: 'something else entirely'
    }));
    const text = formatReport(report);
    expect(text).toContain('expected: move this later');
    expect(text).toContain('got:      something else entirely');
  });
});
