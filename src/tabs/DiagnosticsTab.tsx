import { For, Show } from "solid-js";
import { store } from "../store";
import Badge from "../ui/Badge";
import Banner from "../ui/Banner";
import Button from "../ui/Button";
import { Section } from "../ui/Card";
import { providerLabel } from "./languages";

/** Timestamps from Rust are milliseconds since the epoch; config times, seconds. */
function formatMillis(value?: number | null): string {
  return value ? new Date(value).toLocaleString() : "—";
}

export default function DiagnosticsTab() {
  return (
    <Show when={store.diagnostics()}>
      {(diag) => (
        <div class="space-y-4">
          <Show when={store.message()}>
            <Banner tone="accent" title="最近一次操作">
              <p class="mt-0.5 break-words opacity-90">{store.message()}</p>
            </Banner>
          </Show>

          <Section
            title="辅助功能权限"
            description="macOS 需要它才能读到别处的选中文字，也才能模拟一次复制。"
          >
            <div class="flex items-center justify-between">
              <Badge tone={diag().accessibilityTrusted ? "ok" : "danger"}>
                {diag().accessibilityTrusted ? "已授权" : "未授权"}
              </Badge>
              <Button onClick={() => void store.openAccessibilitySettings()}>
                打开系统设置
              </Button>
            </div>
            <Show when={!diag().accessibilityTrusted}>
              <p class="text-xs text-warn">授权后需要重启 fk-trans 才会生效。</p>
            </Show>
          </Section>

          <Section title="触发状态">
            <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
              <dt class="text-muted">监听状态</dt>
              <dd class="text-ink">{diag().mouse.status}</dd>

              <dt class="text-muted">触发键</dt>
              <dd class="text-ink">{diag().mouse.triggerButton}</dd>

              <dt class="text-muted">划词触发</dt>
              <dd class="text-ink">
                {diag().mouse.selectionTriggerEnabled ? "已开启" : "已关闭"}
              </dd>

              <dt class="text-muted">最后按键</dt>
              <dd class="text-ink">
                {diag().mouse.lastButton ?? "—"}（{formatMillis(diag().mouse.lastEventAt)}
                ）
              </dd>

              <dt class="text-muted">最后触发</dt>
              <dd class="text-ink">
                {diag().mouse.lastPipelineSource ?? "—"}（
                {formatMillis(diag().mouse.lastTriggerAt)}）
              </dd>

              <dt class="text-muted">最后结果</dt>
              <dd class="text-ink break-words">
                {diag().mouse.lastPipelineResult ?? "—"}
              </dd>

              <Show when={diag().mouse.lastError}>
                <dt class="text-muted">错误</dt>
                <dd class="text-danger break-words">{diag().mouse.lastError}</dd>
              </Show>
            </dl>

            <div class="flex gap-2 border-t border-line pt-3">
              <Button onClick={() => void store.startMouseTest()}>检测鼠标按键</Button>
              <Button
                disabled={diag().mouse.lastButton === null}
                onClick={() => void store.saveLastMouseButton()}
              >
                把最后按下的键设为触发键
              </Button>
            </div>
          </Section>

          <Section title="翻译服务" description="「就绪」表示这一家现在能直接用。">
            <For each={diag().providers}>
              {(provider) => (
                <div class="flex items-start justify-between gap-3">
                  <div class="min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm text-ink">{providerLabel(provider.name)}</span>
                      <Show when={provider.active}>
                        <Badge tone="accent">当前</Badge>
                      </Show>
                    </div>
                    <Show when={provider.reason}>
                      <p class="mt-0.5 text-xs break-words text-faint">
                        {provider.reason}
                      </p>
                    </Show>
                  </div>
                  <Badge tone={provider.ready ? "ok" : "neutral"}>
                    {provider.ready ? "就绪" : "未就绪"}
                  </Badge>
                </div>
              )}
            </For>
          </Section>

          <Section title="日志">
            <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
              <dt class="text-muted">版本</dt>
              <dd class="text-ink">v{diag().appVersion}</dd>

              <dt class="text-muted">目录</dt>
              <dd class="break-all text-ink">{diag().logDir ?? "—"}</dd>

              <dt class="text-muted">单个文件上限</dt>
              <dd class="text-ink">
                {Math.round(diag().logMaxFileSizeBytes / 1024)} KB，保留{" "}
                {diag().logRotationKeepFiles} 个
              </dd>

              <dt class="text-muted">详细日志</dt>
              <dd class="text-ink">{diag().debugLogging ? "已开启" : "已关闭"}</dd>
            </dl>

            <div class="flex gap-2 border-t border-line pt-3">
              <Button onClick={() => void store.revealDiagnosticsFolder()}>
                打开日志目录
              </Button>
              <Button onClick={() => void store.exportDiagnostics()}>导出诊断报告</Button>
            </div>
          </Section>
        </div>
      )}
    </Show>
  );
}
