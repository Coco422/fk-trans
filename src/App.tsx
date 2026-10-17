import { For, onCleanup, onMount, Show } from "solid-js";
import { store, TABS } from "./store";
import DiagnosticsTab from "./tabs/DiagnosticsTab";
import GeneralTab from "./tabs/GeneralTab";
import HistoryTab from "./tabs/HistoryTab";
import ProvidersTab from "./tabs/ProvidersTab";
import Banner from "./ui/Banner";

export default function App() {
  onMount(() => {
    // Start from a fresh snapshot rather than waiting for the first event: the
    // config-load banner below has to be there on the first paint, not once
    // something moves.
    void store.refreshDiagnostics();
    onCleanup(store.start());
  });

  const problem = () => store.diagnostics()?.configLoadProblem ?? null;
  const notices = () => store.diagnostics()?.configNotices ?? [];

  return (
    <div class="flex h-screen flex-col bg-canvas text-ink">
      <header class="mx-auto w-full max-w-2xl shrink-0 px-6 pt-6">
        <div class="mb-4 flex items-baseline justify-between">
          <h1 class="text-xl font-semibold">fk-trans</h1>
          <span class="text-xs text-faint">
            {store.diagnostics()?.appVersion ? `v${store.diagnostics()?.appVersion}` : ""}
          </span>
        </div>

        <div class="flex gap-1 border-b border-line" role="tablist">
          <For each={TABS}>
            {(tab) => (
              <button
                type="button"
                role="tab"
                aria-selected={store.tab() === tab.id}
                class="-mb-px cursor-pointer border-b-2 px-4 py-2 text-sm font-medium transition-colors"
                classList={{
                  "border-accent text-accent": store.tab() === tab.id,
                  "border-transparent text-muted hover:text-ink": store.tab() !== tab.id,
                }}
                onClick={() => store.setTab(tab.id)}
              >
                {tab.label}
              </button>
            )}
          </For>
        </div>
      </header>

      <main class="flex-1 overflow-y-auto">
        <div class="mx-auto w-full max-w-2xl space-y-4 px-6 py-6">
          {/*
            The config banners live above the tabs rather than inside Diagnostics.
            A config file that could not be read makes every screen wrong, so
            saying so on one of four tabs means three of them look broken with no
            explanation.
          */}
          <Show when={problem()}>
            {(issue) => (
              <Banner
                tone={issue().kind === "malformed" ? "warn" : "danger"}
                title={
                  issue().kind === "malformed"
                    ? "配置文件读不出来，正在使用默认设置"
                    : "无法读取配置文件，正在使用默认设置"
                }
              >
                <p class="mt-0.5 break-all opacity-90">{issue().path}</p>
                <p class="mt-1 opacity-90">{issue().detail}</p>
                <Show when={issue().backupPath}>
                  <p class="mt-1 break-all opacity-90">
                    原文件已备份到 {issue().backupPath}
                  </p>
                </Show>
              </Banner>
            )}
          </Show>

          <Show when={notices().length > 0}>
            <Banner tone="warn" title="配置已自动调整">
              <ul class="mt-0.5 list-inside list-disc opacity-90">
                <For each={notices()}>{(notice) => <li>{notice}</li>}</For>
              </ul>
            </Banner>
          </Show>

          <Show when={store.tab() === "general"}>
            <GeneralTab />
          </Show>
          <Show when={store.tab() === "providers"}>
            <ProvidersTab />
          </Show>
          <Show when={store.tab() === "diagnostics"}>
            <DiagnosticsTab />
          </Show>
          <Show when={store.tab() === "history"}>
            <HistoryTab />
          </Show>
        </div>
      </main>
    </div>
  );
}
