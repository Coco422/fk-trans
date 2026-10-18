import type { JSX } from "solid-js";
import { createEffect, Show } from "solid-js";
import { cx } from "./cx";

/** One control's worth of layout: a label, the control, and an explanation. */
export default function Field(props: {
  label: string;
  hint?: string;
  /** Stacks the control under the label instead of beside it. */
  stacked?: boolean;
  children: JSX.Element;
}) {
  return (
    <div
      class={cx(
        "gap-2",
        props.stacked ? "flex flex-col" : "flex items-center justify-between"
      )}
    >
      <div class={props.stacked ? "" : "min-w-0"}>
        <div class="text-sm text-ink">{props.label}</div>
        <Show when={props.hint}>
          <div class="mt-0.5 text-xs text-faint">{props.hint}</div>
        </Show>
      </div>
      <div class={props.stacked ? "" : "shrink-0"}>{props.children}</div>
    </div>
  );
}

const CONTROL =
  "rounded-control border border-line bg-surface px-2.5 text-sm text-ink " +
  "placeholder:text-faint focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/40";

export function TextInput(props: {
  value: string;
  placeholder?: string;
  type?: "text" | "password";
  mono?: boolean;
  onInput: (value: string) => void;
}) {
  return (
    <input
      type={props.type ?? "text"}
      value={props.value}
      placeholder={props.placeholder}
      spellcheck={false}
      autocomplete="off"
      onInput={(event) => props.onInput(event.currentTarget.value)}
      class={cx(CONTROL, "h-8 w-full", props.mono && "font-mono text-xs")}
    />
  );
}

export function TextArea(props: {
  value: string;
  placeholder?: string;
  rows?: number;
  onInput: (value: string) => void;
}) {
  return (
    <textarea
      value={props.value}
      placeholder={props.placeholder}
      rows={props.rows ?? 4}
      spellcheck={false}
      onInput={(event) => props.onInput(event.currentTarget.value)}
      class={cx(CONTROL, "w-full resize-y py-2 leading-relaxed")}
    />
  );
}

export function Select<T extends string>(props: {
  value: T;
  options: ReadonlyArray<{ value: T; label: string }>;
  disabled?: boolean;
  onChange: (value: T) => void;
}) {
  let element: HTMLSelectElement | undefined;

  /*
   * A `<select>` cannot hold a value that has no matching `<option>` yet, and it
   * does not recover when they arrive: it falls back to showing the first one.
   * That is not a hypothetical — the language lists here are fetched, so for one
   * frame the target-language select had no options, and afterwards it displayed
   * 英语 while the config said `zh` and every translation came out Chinese.
   *
   * Reading the options inside the effect is what makes this re-run once they
   * are there.
   */
  createEffect(() => {
    const wanted = props.value;
    void props.options.length;
    if (element && element.value !== wanted) {
      element.value = wanted;
    }
  });

  return (
    <select
      ref={element}
      disabled={props.disabled}
      onChange={(event) => props.onChange(event.currentTarget.value as T)}
      class={cx(CONTROL, "h-8 cursor-pointer pr-1")}
    >
      {props.options.map((option) => (
        <option value={option.value}>{option.label}</option>
      ))}
    </select>
  );
}
