/**
 * The one place the light/dark choice is read and applied.
 *
 * Both windows call this: the Settings window offers the toggle, and the popup —
 * a separate webview, with no toggle of its own — has to follow it anyway. They
 * share an origin, so they share the stored value.
 */
export type Theme = "light" | "dark" | "system";

const STORAGE_KEY = "fk-trans-theme";

/** The system preference, which is the starting point before anyone chooses. */
function systemTheme(): "light" | "dark" {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/**
 * Reading storage can throw — in a private window, with site data blocked, or
 * during a preview — and the app has to render either way, so every access is
 * guarded and a failure just means "use the system preference".
 */
export function storedTheme(): Theme {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    if (value === "light" || value === "dark" || value === "system") {
      return value;
    }
  } catch {
    // Fall through to the default.
  }
  return "system";
}

export function applyTheme(theme: Theme) {
  const resolved = theme === "system" ? systemTheme() : theme;
  document.documentElement.classList.toggle("dark", resolved === "dark");
  document.documentElement.style.colorScheme = resolved;
}

export function setTheme(theme: Theme) {
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // The choice lasts for this window and not the next one, which is better
    // than refusing to change it at all.
  }
  applyTheme(theme);
}

/**
 * Apply the stored theme now, and keep following the system while the choice is
 * "system".
 *
 * Returns a function that stops the following, for `onCleanup`.
 */
export function startTheme(): () => void {
  applyTheme(storedTheme());

  const media = window.matchMedia?.("(prefers-color-scheme: dark)");
  if (!media) {
    return () => {};
  }

  const onChange = () => {
    if (storedTheme() === "system") {
      applyTheme("system");
    }
  };

  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}
