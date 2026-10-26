import { createSignal, For, Show } from "solid-js";
import type { ActionConfig } from "../api/types";
import { describeError, store } from "../store";
import Badge from "../ui/Badge";
import Button from "../ui/Button";
import Card from "../ui/Card";
import Field, { Select, TextArea } from "../ui/Field";
import { actionLabel, providerLabel } from "./labels";

type Phase = "idle" | "dirty" | "saving" | "saved" | "error";

/** The value meaning "use whichever service is translating". */
const FOLLOW_ACTIVE = "";

/**
 * Settings for the four things the popup can ask about a selection beyond
 * translating it.
 *
 * They are instructions to a model rather than translations, so each one has its
 * own prompt and can be pointed at its own service — a big local model for
 * explaining and a cheap hosted one for summarizing, say.
 */
export default function ActionsCard() {
  const [draft, setDraft] = createSignal<ActionConfig[] | null>(null);
  const [phase, setPhase] = createSignal<Phase>("idle");
  const [error, setError] = createSignal("");

  /** The shipped list until something is edited, so a save is always explicit. */
  function current(): ActionConfig[] {
    return draft() ?? store.config()?.actions ?? [];
  }

  function edit(id: string, patch: Partial<ActionConfig>) {
    setDraft(
      current().map((action) => (action.id === id ? { ...action, ...patch } : action))
    );
    setPhase("dirty");
    setError("");
  }

  function providerOptions() {
    return [
      { value: FOLLOW_ACTIVE, label: "跟随当前翻译服务" },
      ...(store.config()?.providers ?? []).map((provider) => ({
        value: provider.name,
        label: providerLabel(provider.name),
      })),
    ];
  }

  async function save() {
    setPhase("saving");
    try {
      await store.saveConfig({ actions: current() });
      // The store handed the saved list back, so the draft would only be a
      // second copy of it from here on.
      setDraft(null);
      setPhase("saved");
    } catch (reason) {
      setPhase("error");
      setError(describeError(reason));
    }
  }

  return (
    <Show when={store.config()}>
      <Card class="p-4">
        <div class="mb-1 flex items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <h2 class="text-sm font-semibold text-ink">浮层上的额外动作</h2>
            <Show when={phase() === "dirty"}>
              <Badge tone="warn">未保存</Badge>
            </Show>
            <Show when={phase() === "saved"}>
              <Badge tone="ok">已保存</Badge>
            </Show>
          </div>
          <Button
            size="sm"
            variant="primary"
            disabled={phase() !== "dirty" && phase() !== "error"}
            onClick={() => void save()}
          >
            保存
          </Button>
        </div>
        <p class="mb-4 text-xs text-muted">
          解释 / 总结 / 润色 / 词典发给哪个服务、用什么提示词。提示词里可以用{" "}
          <code class="font-mono">{"{text}"}</code>（选中的文字）、
          <code class="font-mono">{"{from}"}</code>、
          <code class="font-mono">{"{to}"}</code>，必须包含{" "}
          <code class="font-mono">{"{text}"}</code>。
        </p>

        <div class="space-y-4">
          <For each={current()}>
            {(action) => (
              <div class="space-y-2 border-t border-line pt-3 first:border-t-0 first:pt-0">
                <div class="flex items-center justify-between gap-2">
                  <span class="text-sm text-ink">{actionLabel(action.id)}</span>
                  <Select
                    value={action.provider ?? FOLLOW_ACTIVE}
                    options={providerOptions()}
                    onChange={(value) =>
                      edit(action.id, {
                        provider: value === FOLLOW_ACTIVE ? null : value,
                      })
                    }
                  />
                </div>
                <Field label="" stacked>
                  <TextArea
                    value={action.prompt}
                    rows={3}
                    onInput={(value) => edit(action.id, { prompt: value })}
                  />
                </Field>
              </div>
            )}
          </For>
        </div>

        <Show when={error()}>
          <p class="mt-3 text-xs break-words text-danger">{error()}</p>
        </Show>
      </Card>
    </Show>
  );
}
