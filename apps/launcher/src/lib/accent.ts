/**
 * Per-instance colour, derived from the instance id.
 *
 * The design is artwork-led: an instance's page and card take their accent from
 * the instance rather than the app, so the library doesn't read as one flat
 * wall of violet. Until instances carry real cover art (pack icons, Phase 7),
 * the hue is hashed from the id — stable across renders and restarts, and
 * different enough between instances to tell them apart at a glance.
 */

function hueFor(id: string): number {
  let hash = 0;
  for (let i = 0; i < id.length; i += 1) {
    hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  }
  return hash % 360;
}

/** Background for an instance's cover area. */
export function coverStyle(id: string): React.CSSProperties {
  const hue = hueFor(id);
  return {
    backgroundImage: `linear-gradient(140deg,
      oklch(0.62 0.17 ${hue}) 0%,
      oklch(0.48 0.15 ${(hue + 40) % 360}) 55%,
      oklch(0.33 0.10 ${(hue + 75) % 360}) 100%)`,
  };
}

/**
 * Overrides `--accent` for a subtree.
 *
 * Every component reads accent through the token rather than a literal colour,
 * so setting it on one wrapper re-tints the buttons, focus rings and highlights
 * beneath it without any of them knowing an instance exists.
 */
export function accentStyle(id: string): React.CSSProperties {
  const hue = hueFor(id);
  return {
    "--accent": `oklch(0.62 0.17 ${hue})`,
    "--accent-hover": `oklch(0.68 0.17 ${hue})`,
    "--accent-active": `oklch(0.56 0.17 ${hue})`,
    "--accent-soft": `oklch(0.62 0.17 ${hue} / 0.15)`,
    "--accent-ring": `oklch(0.62 0.17 ${hue} / 0.45)`,
  } as React.CSSProperties;
}
