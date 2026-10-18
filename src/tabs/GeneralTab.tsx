import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { createResource, createSignal, Show } from "solid-js";
import * as api from "../api/commands";
import { useAutostart } from "../autostart";
import { describeError, store } from "../store";
import type { Theme } from "../theme";
import Button from "../ui/Button";
import Card, { Section } from "../ui/Card";
import Field, { Select } from "../ui/Field";
import Toggle from "../ui/Toggle";
import { languageLabel, providerLabel } from "./labels";

type UpdateStatus =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "downloading"; message: string }
  | { status: "installed"; message: string }
  | { status: "none"; message: string }
  | { status: "error"; message: string };

const busy = new Set(["checking", "downloading", "installed"]);

export default function GeneralTab() {
  const [languages] = createResource(() => api.getLanguages());
  const [update, setUpdate] = createSignal<UpdateStatus>({ status: "idle" });
  const autostart = useAutostart();

  /** The language list, with every code the config might hold accounted for. */
  function options() {
    const listed = languages() ?? [];
    // The config can name a language the table no longer has. Showing it beats
    // silently rewriting the user's setting to whatever happens to be first.
    const known = new Set(listed.map((language) => language.code));
    const extra = [store.config()?.sourceLang, store.config()?.targetLang]
      .filter((code): code is string => !!code && !known.has(code))
      .map((code) => ({ code, name: code }));

    return [...listed, ...extra].map((language) => ({
      value: language.code,
      label: languageLabel(language.code),
    }));
  }

  async function checkForUpdates() {
    setUpdate({ status: "checking" });

    try {
      const found = await check();

      if (!found) {
        // The snapshot already carries the version, so there is no second
        // reading that could disagree with what the header shows.
        const version = store.diagnostics()?.appVersion;
        setUpdate({
          status: "none",
          message: version ? `已经是最新版本 v${version}。` : "已经是最新版本。",
        });
        return;
      }

      let downloaded = 0;
      let total: number | undefined;

      await found.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
          downloaded = 0;
          setUpdate({
            status: "downloading",
            message: `正在下载 v${found.version}…`,
          });
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const percent =
            total && total > 0 ? ` ${Math.round((downloaded / total) * 100)}%` : "";
          setUpdate({
            status: "downloading",
            message: `正在下载 v${found.version}${percent}`,
          });
        } else {
          setUpdate({
            status: "installed",
            message: `已安装 v${found.version}，正在重启…`,
          });
        }
      });

      await relaunch();
    } catch (error) {
      setUpdate({ status: "error", message: `更新失败：${describeError(error)}` });
    }
  }

  return (
    <Show when={store.config()}>
      {(cfg) => (
        <div class="space-y-4">
          <Section title="翻译" description="关掉之后，任何触发方式都不再翻译。">
            <Field label="启用 fk-trans">
              <Toggle
                label="启用 fk-trans"
                checked={cfg().enabled}
                onChange={(enabled) => void store.saveConfig({ enabled })}
              />
            </Field>

            <Field
              label="划词触发"
              hint="选中文字后，在选区旁边出现一个小圆点，点击才翻译。"
            >
              <Toggle
                label="划词触发"
                checked={cfg().selectionTriggerEnabled}
                onChange={(selectionTriggerEnabled) =>
                  void store.saveConfig({ selectionTriggerEnabled })
                }
              />
            </Field>
          </Section>

          <Section
            title="当前翻译服务"
            description="出问题时换一个，配置在「翻译服务」页。"
          >
            <Field label="使用">
              <Select
                value={cfg().activeProvider}
                options={cfg().providers.map((provider) => ({
                  value: provider.name,
                  label: providerLabel(provider.name),
                }))}
                onChange={(activeProvider) => void store.saveConfig({ activeProvider })}
              />
            </Field>
          </Section>

          <Section title="语言">
            <Field label="源语言">
              <Select
                value={cfg().sourceLang}
                options={options()}
                disabled={languages.loading}
                onChange={(sourceLang) => void store.saveConfig({ sourceLang })}
              />
            </Field>
            <Field label="目标语言">
              <Select
                value={cfg().targetLang}
                options={options().filter((option) => option.value !== "auto")}
                disabled={languages.loading}
                onChange={(targetLang) => void store.saveConfig({ targetLang })}
              />
            </Field>
          </Section>

          <Section title="触发方式">
            <Field label="鼠标触发键" hint="中键。触控板用户可以用划词触发，或按 ⌘⇧T。">
              <span class="font-mono text-sm text-muted">
                按键 {cfg().mouseTriggerButton}
              </span>
            </Field>
            <p class="text-xs text-faint">
              按键不对？到「诊断」页按下鼠标键，把它设成触发键。
            </p>
          </Section>

          <Section title="外观">
            <Field label="主题">
              <Select<Theme>
                value={store.theme()}
                options={[
                  { value: "system", label: "跟随系统" },
                  { value: "light", label: "浅色" },
                  { value: "dark", label: "深色" },
                ]}
                onChange={(theme) => store.chooseTheme(theme)}
              />
            </Field>
            <Field label="详细日志" hint="记录调试信息，日志文件会变大。">
              <Toggle
                label="详细日志"
                checked={cfg().debugLogging}
                onChange={(debugLogging) => void store.saveConfig({ debugLogging })}
              />
            </Field>
          </Section>

          <Section title="启动">
            <Field
              label="登录时自动启动"
              hint={autostart.error() ?? "开机后静默启动，只在菜单栏出现。"}
            >
              <Toggle
                label="登录时自动启动"
                checked={autostart.enabled()}
                disabled={autostart.error() !== null}
                onChange={(value) => void autostart.set(value)}
              />
            </Field>
          </Section>

          <Card class="p-4">
            <div class="flex items-center justify-between">
              <div>
                <h2 class="text-sm font-semibold text-ink">版本更新</h2>
                <p class="mt-0.5 text-xs text-muted">
                  当前版本 v{store.diagnostics()?.appVersion ?? "…"}
                </p>
              </div>
              <Button
                variant="primary"
                disabled={busy.has(update().status)}
                onClick={() => void checkForUpdates()}
              >
                {update().status === "checking" || update().status === "downloading"
                  ? "检查中…"
                  : "检查更新"}
              </Button>
            </div>
            <Show when={"message" in update() ? update() : null}>
              {(current) => (
                <p
                  class="mt-3 text-xs"
                  classList={{
                    "text-danger": current().status === "error",
                    "text-ok": current().status === "none",
                    "text-muted": current().status === "downloading",
                  }}
                >
                  {(current() as { message: string }).message}
                </p>
              )}
            </Show>
          </Card>
        </div>
      )}
    </Show>
  );
}
