/**
 * Whether the app starts when the user logs in.
 *
 * The state is not in `config.json`: the login item *is* the state, and keeping
 * a copy of it in the config would be a second answer that can disagree with
 * macOS — for instance after the user removes the login item themselves.
 */
import { createSignal, onMount } from "solid-js";

export function useAutostart() {
  const [enabled, setEnabled] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    try {
      const { isEnabled } = await import("@tauri-apps/plugin-autostart");
      setEnabled(await isEnabled());
    } catch (reason) {
      // Reported next to the control rather than thrown: the rest of the app
      // works, and a toggle that does nothing is worth saying out loud.
      setError(`无法读取开机启动设置：${String(reason)}`);
    }
  });

  async function set(value: boolean) {
    try {
      const { enable, disable } = await import("@tauri-apps/plugin-autostart");
      await (value ? enable() : disable());
      setEnabled(value);
      setError(null);
    } catch (reason) {
      setError(`设置失败：${String(reason)}`);
    }
  }

  return { enabled, error, set };
}
