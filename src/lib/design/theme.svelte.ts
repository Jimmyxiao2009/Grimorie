/**
 * Theme and manuscript-appearance control.
 *
 * SQLite is the source of truth for these preferences. `localStorage` is used
 * only as a *cache* of the last resolved theme, read synchronously before the
 * first paint so the window does not flash the wrong colours while settings
 * load over IPC. If the cache and the database ever disagree, the database
 * wins.
 *
 * DOM writes happen in explicit functions rather than a hidden effect, so it is
 * always obvious what changed the document and when.
 */

export const THEME_CHOICES = ['system', 'paper', 'light', 'dark', 'night'] as const;
export type ThemeChoice = (typeof THEME_CHOICES)[number];

/** A theme that actually exists as a token block. `system` is never one. */
export type ResolvedTheme = Exclude<ThemeChoice, 'system'>;

export const MANUSCRIPT_FACES = ['serif', 'sans', 'mono'] as const;
export type ManuscriptFace = (typeof MANUSCRIPT_FACES)[number];

export type ManuscriptTypography = {
  face: ManuscriptFace;
  /** Body size in rem. */
  size: number;
  /** Unitless line height. */
  leading: number;
  /** Space between paragraphs, in em of the body size. */
  paragraphGap: number;
  /** Measure — the manuscript column width — in rem. */
  measure: number;
};

export const DEFAULT_TYPOGRAPHY: ManuscriptTypography = {
  face: 'serif',
  size: 1.125,
  leading: 1.7,
  paragraphGap: 0.85,
  measure: 36
};

const CACHE_KEY = 'grimoire.resolved-theme';

/**
 * `system` resolves to Grimoire's own reading themes rather than to the plainer
 * Light and Dark. Paper by day and Night after dark is the identity of the app;
 * Light and Dark remain selectable for anyone who wants something more neutral.
 */
function resolve(choice: ThemeChoice, prefersDark: boolean): ResolvedTheme {
  if (choice !== 'system') return choice;
  return prefersDark ? 'night' : 'paper';
}

function isThemeChoice(value: unknown): value is ThemeChoice {
  return typeof value === 'string' && (THEME_CHOICES as readonly string[]).includes(value);
}

class Appearance {
  choice = $state<ThemeChoice>('system');
  prefersDark = $state(false);
  typography = $state<ManuscriptTypography>({ ...DEFAULT_TYPOGRAPHY });

  readonly resolved = $derived(resolve(this.choice, this.prefersDark));

  /** Wired once at startup. Returns a teardown for the media-query listener. */
  start(): () => void {
    if (typeof window === 'undefined') return () => {};

    const query = window.matchMedia('(prefers-color-scheme: dark)');
    this.prefersDark = query.matches;

    const onChange = (event: MediaQueryListEvent) => {
      this.prefersDark = event.matches;
      this.paint();
    };
    query.addEventListener('change', onChange);

    this.paint();
    return () => query.removeEventListener('change', onChange);
  }

  setChoice(choice: ThemeChoice): void {
    this.choice = choice;
    this.paint();
  }

  setTypography(typography: ManuscriptTypography): void {
    this.typography = typography;
    this.paint();
  }

  /** Writes the current state to the document. The only place that does. */
  paint(): void {
    if (typeof document === 'undefined') return;

    const theme = this.resolved;
    const root = document.documentElement;
    root.dataset['theme'] = theme;

    const t = this.typography;
    const style = root.style;
    style.setProperty('--font-manuscript', `var(--font-${t.face})`);
    style.setProperty('--manuscript-size', `${t.size}rem`);
    style.setProperty('--manuscript-leading', `${t.leading}`);
    style.setProperty('--manuscript-paragraph-gap', `${t.paragraphGap}em`);
    style.setProperty('--measure-editor', `${t.measure}rem`);

    try {
      localStorage.setItem(CACHE_KEY, theme);
    } catch {
      // Private mode or a wiped profile. The cache is an optimisation; losing
      // it costs a brief flash on next launch and nothing else.
    }
  }
}

export const appearance = new Appearance();

/**
 * Applies the cached theme synchronously, before Svelte mounts. Called from the
 * entry point so the very first frame is already the right colour.
 */
export function applyCachedTheme(): void {
  if (typeof document === 'undefined') return;
  let cached: string | null = null;
  try {
    cached = localStorage.getItem(CACHE_KEY);
  } catch {
    cached = null;
  }
  const theme = isThemeChoice(cached) && cached !== 'system' ? cached : null;
  document.documentElement.dataset['theme'] =
    theme ?? (window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'night' : 'paper');
}

/** Exported for tests. */
export const _internal = { resolve, isThemeChoice, CACHE_KEY };
