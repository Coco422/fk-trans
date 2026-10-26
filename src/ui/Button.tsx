import type { JSX } from "solid-js";
import { cx } from "./cx";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-accent text-on-accent hover:opacity-90 border border-transparent",
  secondary: "bg-surface text-ink border border-line hover:bg-raised",
  ghost: "text-muted hover:text-ink hover:bg-raised border border-transparent",
  danger: "bg-danger-soft text-danger border border-transparent hover:opacity-90",
};

const SIZES: Record<Size, string> = {
  sm: "h-7 px-2.5 text-xs",
  md: "h-8 px-3 text-sm",
};

export default function Button(props: {
  variant?: Variant;
  size?: Size;
  disabled?: boolean;
  title?: string;
  type?: "button" | "submit";
  onClick?: (event: MouseEvent) => void;
  children: JSX.Element;
}) {
  return (
    <button
      type={props.type ?? "button"}
      title={props.title}
      disabled={props.disabled}
      onClick={props.onClick}
      class={cx(
        "inline-flex items-center justify-center gap-1.5 rounded-control font-medium transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40",
        "disabled:cursor-not-allowed disabled:opacity-45",
        props.disabled ? "" : "cursor-pointer",
        VARIANTS[props.variant ?? "secondary"],
        SIZES[props.size ?? "md"]
      )}
    >
      {props.children}
    </button>
  );
}
