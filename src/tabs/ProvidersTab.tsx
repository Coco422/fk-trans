import { createSignal, For, Show } from "solid-js";
import * as api from "../api/commands";
import type { ProviderConfig, ProviderUpdate } from "../api/types";
import { describeError, store } from "../store";
import Badge, { type Tone } from "../ui/Badge";
import Button from "../ui/Button";
import Card from "../ui/Card";
import Field, { TextArea, TextInput } from "../ui/Field";
import ActionsCard from "./ActionsCard";
import { providerLabel } from "./labels";

type Phase =
  | "idle"
  | "dirty"
  | "saving"
  | "saved"
  | "save_error"
  | "testing"
  | "test_success"
  | "test_error";

interface Status {
  phase: Phase;
  message: string;
}

const TONES: Partial<Record<Phase, Tone>> = {
  dirty: "warn",
  saved: "ok",
  test_success: "ok",
  save_error: "danger",
  test_error: "danger",
};

const LABELS: Partial<Record<Phase, string>> = {
  dirty: "未保存",
  saving: "保存中…",
  saved: "已保存",
  save_error: "保存失败",
  testing: "测试中…",
  test_success: "测试通过",
  test_error: "测试失败",
};

/**
 * The draft is a full `ProviderConfig`; `ProviderUpdate` is the subset the
 * command accepts. Both are generated, so this stays honest if either changes.
 */
function asUpdate(name: string, draft: ProviderConfig): ProviderUpdate {
  return {
    name,
    baseUrl: draft.baseUrl,
    apiKey: draft.apiKey,
    model: draft.model,
    systemPrompt: draft.systemPrompt,
    userPrompt: draft.userPrompt,
    extraParams: draft.extraParams,
  };
}

/**
 * Everything a provider editor needs, for one provider.
 *
 * Kept per-provider and created inside the loop rather than lifted into the
 * store: nothing outside this tab can see a draft, and a draft that outlived the
 * tab would be a draft nobody could discard.
 */
function useEditor(provider: () => ProviderConfig) {
  const [draft, setDraft] = createSignal<ProviderConfig | null>(null);
  const [extraParamsText, setExtraParamsText] = createSignal<string | null>(null);
  const [status, setStatus] = createSignal<Status>({ phase: "idle", message: "" });
  const [expanded, setExpanded] = createSignal(false);

  function current(): ProviderConfig {
    return draft() ?? provider();
  }

  function changed(a: ProviderConfig, b: ProviderConfig): boolean {
    return (
      a.baseUrl !== b.baseUrl ||
      a.apiKey !== b.apiKey ||
      a.model !== b.model ||
      a.systemPrompt !== b.systemPrompt ||
      a.userPrompt !== b.userPrompt ||
      JSON.stringify(a.extraParams ?? {}) !== JSON.stringify(b.extraParams ?? {})
    );
  }

  function edit(field: keyof ProviderConfig, value: string | Record<string, unknown>) {
    const next = { ...current(), [field]: value } as ProviderConfig;
    setDraft(next);
    setStatus(
      changed(next, provider())
        ? { phase: "dirty", message: "" }
        : { phase: "idle", message: "" }
    );
  }

  function editExtraParams(raw: string) {
    setExtraParamsText(raw);
    setStatus({ phase: "dirty", message: "" });
    try {
      edit("extraParams", JSON.parse(raw));
    } catch {
      // The raw text is kept as typed. Invalid intermediate JSON while someone
      // is mid-edit is normal, and throwing away what they typed is not.
    }
  }

  /** Write the draft back, once the text is valid JSON. */
  async function save(): Promise<boolean> {
    const raw = extraParamsText();
    if (raw !== null) {
      let parsed: unknown;
      try {
        parsed = JSON.parse(raw);
      } catch {
        setStatus({ phase: "save_error", message: "附加参数不是合法的 JSON。" });
        return false;
      }
      edit("extraParams", parsed as Record<string, unknown>);
      setExtraParamsText(null);
    }

    const next = current();
    if (!changed(next, provider())) {
      setDraft(null);
      setStatus({ phase: "saved", message: "" });
      return true;
    }

    setStatus({ phase: "saving", message: "" });
    try {
      await api.updateProvider(asUpdate(provider().name, next));
      await store.reloadConfig();
      setDraft(null);
      setStatus({ phase: "saved", message: "" });
      return true;
    } catch (error) {
      setStatus({ phase: "save_error", message: describeError(error) });
      return false;
    }
  }

  async function test() {
    // Saved first, deliberately: a test that ran against the old settings would
    // pass and leave the user believing the new ones work.
    if (!(await save())) {
      return;
    }

    setStatus({ phase: "testing", message: "" });
    try {
      const result = await api.testProvider(provider().name);
      setStatus({ phase: "test_success", message: result });
    } catch (error) {
      setStatus({ phase: "test_error", message: describeError(error) });
    }
  }

  return {
    current,
    status,
    expanded,
    toggleExpanded: () => setExpanded((open) => !open),
    edit,
    extraParamsValue: () =>
      extraParamsText() ?? JSON.stringify(current().extraParams ?? {}, null, 2),
    editExtraParams,
    busy: () => status().phase === "saving" || status().phase === "testing",
    save,
    test,
  };
}

export default function ProvidersTab() {
  return (
    <Show when={store.config()}>
      {(cfg) => (
        <div class="space-y-4">
          <p class="text-xs text-muted">
            每家服务各填各的。改动要先「保存」才会生效，「测试」会先保存再试一次翻译。
          </p>

          {/*
            Iterated by name, not by the provider object. `For` reconciles by
            identity, and every reload of the config hands back new objects — so
            iterating the objects would tear down and rebuild every editor on
            each save, taking its draft and its "saved" badge with it.
          */}
          <For each={cfg().providers.map((provider) => provider.name)}>
            {(name) => {
              // The names iterated above are derived from this same array, so the
              // lookup cannot come up empty; a miss would be a bug in the
              // derivation, not a state to render around.
              // biome-ignore lint/style/noNonNullAssertion: name comes from this array
              const provider = () => cfg().providers.find((c) => c.name === name)!;
              const editor = useEditor(provider);
              const isActive = () => cfg().activeProvider === name;

              return (
                <Card class="p-4">
                  <div class="mb-3 flex items-center justify-between gap-2">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-ink">
                        {providerLabel(name)}
                      </span>
                      <Show when={isActive()}>
                        <Badge tone="accent">当前使用</Badge>
                      </Show>
                      <Show when={LABELS[editor.status().phase]}>
                        <Badge tone={TONES[editor.status().phase] ?? "neutral"}>
                          {LABELS[editor.status().phase] ?? ""}
                        </Badge>
                      </Show>
                    </div>
                    <div class="flex items-center gap-1.5">
                      <Button
                        size="sm"
                        disabled={editor.busy()}
                        onClick={() => void editor.test()}
                      >
                        测试
                      </Button>
                      <Button
                        size="sm"
                        variant="primary"
                        disabled={editor.busy()}
                        onClick={() => void editor.save()}
                      >
                        保存
                      </Button>
                    </div>
                  </div>

                  <div class="space-y-2">
                    {/* Optional for a local endpoint: a key is only demanded
                        where one could be used, which is decided from the URL
                        rather than from the provider's name. */}
                    <TextInput
                      type="password"
                      value={editor.current().apiKey}
                      placeholder="API Key（本地服务可以留空）"
                      mono
                      onInput={(value) => editor.edit("apiKey", value)}
                    />

                    <div class="flex gap-2">
                      <div class="min-w-0 flex-1">
                        <TextInput
                          value={editor.current().baseUrl}
                          placeholder="Base URL"
                          mono
                          onInput={(value) => editor.edit("baseUrl", value)}
                        />
                      </div>
                      <div class="w-44 shrink-0">
                        <TextInput
                          value={editor.current().model}
                          placeholder="Model"
                          mono
                          onInput={(value) => editor.edit("model", value)}
                        />
                      </div>
                    </div>
                  </div>

                  <button
                    type="button"
                    class="mt-3 cursor-pointer text-xs text-faint hover:text-muted"
                    onClick={() => editor.toggleExpanded()}
                  >
                    {editor.expanded() ? "▾ 收起高级设置" : "▸ 高级设置"}
                  </button>

                  <Show when={editor.expanded()}>
                    <div class="mt-3 space-y-3 border-t border-line pt-3">
                      <Field label="System Prompt" hint="留空则用默认提示词。" stacked>
                        <TextArea
                          value={editor.current().systemPrompt}
                          rows={3}
                          onInput={(value) => editor.edit("systemPrompt", value)}
                        />
                      </Field>
                      <Field
                        label="User Prompt"
                        hint="可用占位符：{from}、{to}、{text}。"
                        stacked
                      >
                        <TextArea
                          value={editor.current().userPrompt}
                          rows={3}
                          onInput={(value) => editor.edit("userPrompt", value)}
                        />
                      </Field>
                      <Field
                        label="附加参数"
                        hint="发给服务商的额外字段，JSON。例如 Ollama 的 reasoning_effort。"
                        stacked
                      >
                        <TextArea
                          value={editor.extraParamsValue()}
                          rows={4}
                          onInput={(value) => editor.editExtraParams(value)}
                        />
                      </Field>
                    </div>
                  </Show>

                  <Show when={editor.status().message}>
                    <p class="mt-3 text-xs break-words text-muted">
                      {editor.status().message}
                    </p>
                  </Show>
                </Card>
              );
            }}
          </For>

          <ActionsCard />
        </div>
      )}
    </Show>
  );
}
