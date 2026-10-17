import type { JSX } from "solid-js";
import type { Tone } from "./Badge";
import { cx } from "./cx";

const TONES: Record<Exclude<Tone, "neutral">, string> = {
  accent: "border-accent/30 bg-accent-soft text-accent",
  ok: "border-ok/30 bg-ok-soft text-ok",
  warn: "border-warn/30 bg-warn-soft text-warn",
  danger: "border-danger/30 bg-danger-soft text-danger",
};

/**
 * A message about the state of the app rather than about one control — a config
 * file that could not be read, a migration that changed something, a test that
 * is waiting for input. Without these the app looks like it silently ignored
 * what the user did.
 */
export default function Banner(props: {
  tone: Exclude<Tone, "neutral">;
  title: string;
  children?: JSX.Element;
}) {
  return (
    <div class={cx("rounded-card border px-3 py-2 text-xs", TONES[props.tone])}>
      <div class="font-medium">{props.title}</div>
      {props.children}
    </div>
  );
}
