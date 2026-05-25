import { createMemo, createSignal, onCleanup, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const MIN_SELECTION_PX = 8;

interface OcrSelectionPayload {
  sessionId: string;
  imageDataUrl: string;
  monitorX: number;
  monitorY: number;
  monitorWidth: number;
  monitorHeight: number;
  imageWidth: number;
  imageHeight: number;
}

interface Point {
  x: number;
  y: number;
}

interface OcrSelectionRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

function normalizeSelection(start: Point, end: Point): OcrSelectionRect {
  const x = Math.min(start.x, end.x);
  const y = Math.min(start.y, end.y);
  return {
    x,
    y,
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  };
}

export default function OcrSelectionOverlay() {
  const [payload, setPayload] = createSignal<OcrSelectionPayload | null>(null);
  const [start, setStart] = createSignal<Point | null>(null);
  const [current, setCurrent] = createSignal<Point | null>(null);
  const [submitting, setSubmitting] = createSignal(false);
  let root: HTMLDivElement | undefined;
  let unlistenStarted: (() => void) | undefined;

  const selection = createMemo(() => {
    const a = start();
    const b = current();
    return a && b ? normalizeSelection(a, b) : null;
  });

  onMount(async () => {
    try {
      const initial = await invoke<OcrSelectionPayload | null>(
        "get_ocr_selection_payload"
      );
      if (initial) setPayload(initial);
    } catch {}

    listen<OcrSelectionPayload>("ocr-selection-started", (event) => {
      setPayload(event.payload);
      setStart(null);
      setCurrent(null);
      setSubmitting(false);
    }).then((unlisten) => {
      unlistenStarted = unlisten;
    });

    window.addEventListener("keydown", handleKeydown);
  });

  onCleanup(() => {
    unlistenStarted?.();
    window.removeEventListener("keydown", handleKeydown);
  });

  function pointFromEvent(e: PointerEvent): Point {
    const bounds = root?.getBoundingClientRect();
    return {
      x: e.clientX - (bounds?.left ?? 0),
      y: e.clientY - (bounds?.top ?? 0),
    };
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelSelection();
    }
  }

  function handlePointerDown(e: PointerEvent) {
    if (e.button !== 0 || submitting()) return;
    e.preventDefault();
    root?.setPointerCapture(e.pointerId);
    const point = pointFromEvent(e);
    setStart(point);
    setCurrent(point);
  }

  function handlePointerMove(e: PointerEvent) {
    if (!start() || submitting()) return;
    e.preventDefault();
    setCurrent(pointFromEvent(e));
  }

  function handlePointerUp(e: PointerEvent) {
    const started = start();
    if (!started || submitting()) return;
    e.preventDefault();
    if (root?.hasPointerCapture(e.pointerId)) {
      root.releasePointerCapture(e.pointerId);
    }
    const rect = normalizeSelection(started, pointFromEvent(e));
    setStart(null);
    setCurrent(null);

    if (rect.width < MIN_SELECTION_PX || rect.height < MIN_SELECTION_PX) {
      cancelSelection();
      return;
    }
    completeSelection(rect);
  }

  async function completeSelection(rect: OcrSelectionRect) {
    const active = payload();
    if (!active) return;
    setSubmitting(true);
    try {
      await invoke("complete_ocr_selection", {
        sessionId: active.sessionId,
        selection: rect,
      });
    } catch {
      setSubmitting(false);
    }
  }

  async function cancelSelection() {
    const active = payload();
    if (!active || submitting()) return;
    setSubmitting(true);
    try {
      await invoke("cancel_ocr_selection", {
        sessionId: active.sessionId,
      });
    } catch {
      setSubmitting(false);
    }
  }

  return (
    <div
      ref={root}
      class="relative w-screen h-screen overflow-hidden bg-black cursor-crosshair select-none touch-none"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
    >
      <Show when={payload()}>
        {(active) => (
          <img
            src={active().imageDataUrl}
            draggable={false}
            class="absolute inset-0 w-full h-full object-fill pointer-events-none"
          />
        )}
      </Show>
      <div class="absolute inset-0 bg-black/10 pointer-events-none" />
      <Show when={selection()}>
        {(rect) => (
          <div
            class="absolute border border-blue-300 bg-blue-400/20 shadow-[0_0_0_9999px_rgba(0,0,0,0.35)] pointer-events-none"
            style={{
              left: `${rect().x}px`,
              top: `${rect().y}px`,
              width: `${rect().width}px`,
              height: `${rect().height}px`,
            }}
          />
        )}
      </Show>
    </div>
  );
}
