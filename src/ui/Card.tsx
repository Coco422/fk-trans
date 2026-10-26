import type { JSX } from "solid-js";
import { Show } from "solid-js";
import { cx } from "./cx";

export default function Card(props: { class?: string; children: JSX.Element }) {
  return (
    <div class={cx("rounded-card border border-line bg-surface", props.class)}>
      {props.children}
    </div>
  );
}

/** A titled block inside a page: heading, optional description, then content. */
export function Section(props: {
  title: string;
  description?: string;
  children: JSX.Element;
}) {
  return (
    <Card class="p-4">
      <div class="mb-3">
        <h2 class="text-sm font-semibold text-ink">{props.title}</h2>
        <Show when={props.description}>
          <p class="mt-0.5 text-xs text-muted">{props.description}</p>
        </Show>
      </div>
      <div class="space-y-3">{props.children}</div>
    </Card>
  );
}
