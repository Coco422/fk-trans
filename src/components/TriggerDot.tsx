import { createSignal } from "solid-js";

/**
 * The trigger point that appears beside a selection.
 *
 * Filled exactly as its window is sized, because the window is exactly this
 * large — the placement maths in Rust assumes the window *is* the box.
 *
 * Hovering only gives feedback; nothing happens until it is clicked. That is
 * deliberate: translating on hover would fire a request every time the pointer
 * crossed the selection on its way somewhere else.
 */
export default function TriggerDot(props: { onActivate: () => void }) {
  const [hovered, setHovered] = createSignal(false);

  return (
    <button
      type="button"
      aria-label="翻译选中的文字"
      title="翻译选中的文字"
      class="w-[22px] h-[22px] grid place-items-center rounded-full border border-gray-300/80 bg-white/95 text-gray-500 shadow-sm cursor-pointer transition-transform duration-150 ease-out hover:scale-110 hover:border-blue-400 hover:text-blue-500 dark:border-gray-600/80 dark:bg-gray-900/95 dark:text-gray-400 dark:hover:border-blue-500 dark:hover:text-blue-400"
      classList={{ "scale-110": hovered() }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onClick={() => props.onActivate()}
    >
      <span class="text-[11px] font-semibold leading-none">文</span>
    </button>
  );
}
