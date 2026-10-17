import { cx } from "./cx";

export default function Toggle(props: {
  checked: boolean;
  disabled?: boolean;
  /** Read out by screen readers; the visible label is beside it. */
  label: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
      class={cx(
        "relative h-5 w-9 shrink-0 rounded-full border transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40",
        props.disabled ? "cursor-not-allowed opacity-45" : "cursor-pointer",
        props.checked ? "border-transparent bg-accent" : "border-line bg-raised"
      )}
    >
      <span
        class={cx(
          "absolute top-0.5 h-3.5 w-3.5 rounded-full bg-surface shadow-sm transition-all",
          props.checked ? "left-[1.15rem]" : "left-0.5"
        )}
      />
    </button>
  );
}
