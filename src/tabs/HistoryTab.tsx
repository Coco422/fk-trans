import { For, onMount, Show } from "solid-js";
import { store } from "../store";
import Button from "../ui/Button";
import Card from "../ui/Card";
import { languageLabel } from "./languages";

function formatSeconds(value: number): string {
  return new Date(value * 1000).toLocaleString();
}

export default function HistoryTab() {
  // Loaded on mount rather than on every translation: history is something the
  // user comes here to look at, not something they watch. The tab is mounted and
  // unmounted by the shell, so this is exactly once per visit.
  onMount(() => void store.loadHistory());

  return (
    <div class="space-y-4">
      <div class="flex items-center justify-between">
        <p class="text-xs text-muted">只保留这次运行的记录，退出即清空。</p>
        <Button
          variant="danger"
          disabled={store.history().length === 0}
          onClick={() => void store.clearHistory()}
        >
          清空
        </Button>
      </div>

      <Show
        when={store.history().length > 0}
        fallback={<Card class="p-8 text-center text-sm text-faint">还没有翻译记录</Card>}
      >
        <For each={store.history()}>
          {(entry) => (
            <Card class="p-3">
              <div class="mb-1.5 flex items-center gap-2 text-[11px] text-faint">
                <span class="font-mono">
                  {languageLabel(entry.sourceLang)} → {languageLabel(entry.targetLang)}
                </span>
                <span class="rounded bg-raised px-1.5 py-0.5">{entry.provider}</span>
                <span class="ml-auto">{formatSeconds(entry.timestamp)}</span>
              </div>
              <p class="text-xs leading-relaxed whitespace-pre-wrap text-muted">
                {entry.original}
              </p>
              <p class="mt-1.5 border-t border-line pt-1.5 text-sm leading-relaxed whitespace-pre-wrap text-ink">
                {entry.translated}
              </p>
            </Card>
          )}
        </For>
      </Show>
    </div>
  );
}
