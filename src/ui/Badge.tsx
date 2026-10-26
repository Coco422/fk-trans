import { cx } from "./cx";

export type Tone = "neutral" | "accent" | "ok" | "warn" | "danger";

const TONES: Record<Tone, string> = {
  neutral: "bg-raised text-muted",
  accent: "bg-accent-soft text-accent",
  ok: "bg-ok-soft text-ok",
  warn: "bg-warn-soft text-warn",
  danger: "bg-danger-soft text-danger",
};

export default function Badge(props: { tone?: Tone; children: string }) {
  return (
    <span
      class={cx(
        "inline-flex items-center rounded px-1.5 py-0.5 text-[11px] font-medium whitespace-nowrap",
        TONES[props.tone ?? "neutral"]
      )}
    >
      {props.children}
    </span>
  );
}
