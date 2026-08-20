// Vitest setup. Kept deliberately thin: tests exercise real logic, and any stub
// added here must be justified by an environment gap, never by convenience.

// jsdom does not implement matchMedia, which the theme layer queries.
if (typeof window !== 'undefined' && !window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false
  })) as unknown as typeof window.matchMedia;
}
