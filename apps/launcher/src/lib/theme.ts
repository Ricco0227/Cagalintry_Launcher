import type { Theme } from "./api";

/**
 * Applies the theme to the document.
 *
 * "system" is resolved to a concrete attribute rather than left to CSS, so the
 * stylesheet has exactly one mechanism to reason about: `data-theme` is always
 * either "light" or "dark", never absent.
 */
export function applyTheme(theme: Theme): void {
  const resolved =
    theme === "system"
      ? window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark"
      : theme;

  document.documentElement.dataset["theme"] = resolved;
}

/**
 * Keeps "system" in step with the OS while the app is open.
 * Returns a cleanup function; a no-op for the explicit choices.
 */
export function watchSystemTheme(theme: Theme): () => void {
  if (theme !== "system") return () => {};

  const query = window.matchMedia("(prefers-color-scheme: light)");
  const onChange = () => applyTheme("system");
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}
