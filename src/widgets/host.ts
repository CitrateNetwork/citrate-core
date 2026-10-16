// =====================================================================
// citrate-core — widget host bridge, the main window's side (HUP-S10.3, US-10.3 AC1)
//
// One host per widget frame. It hears `message` events, keeps only those whose `source` is its own
// frame's window, validates the shape, and answers a `widget.query` only when the name is in the
// catalog AND in the widget's declared list. Answers go back to that frame only. There is no other
// message type: a widget cannot reach `invoke`, a store method or another widget through here.
// Kept free of React and the store so the escape attempts are testable on their own.
// =====================================================================
import { isWidgetQuery, resolveQuery, type WidgetQuery, type WidgetSources } from "./catalog";

/** The frame's window as far as the host needs it (`iframe.contentWindow`). */
export interface FrameLike {
  postMessage(message: unknown, targetOrigin: string): void;
}

export interface WidgetHostDeps {
  widgetId: string;
  declared: readonly string[];
  frame: FrameLike;
  sources: WidgetSources;
  now(): number;
  /** Called for every answered (true) or refused (false) query; the monitor counts them. */
  onQuery?(widgetId: string, query: string, answered: boolean): void;
}

export interface WidgetHost {
  onMessage(ev: { source: unknown; data: unknown }): void;
  close(): void;
}

/** At most this many queries per widget per rolling minute. */
export const QUERIES_PER_MINUTE = 30;

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

export function createWidgetHost(deps: WidgetHostDeps): WidgetHost {
  let open = true;
  const declared = new Set(deps.declared.filter(isWidgetQuery));
  const recent: number[] = [];

  const reply = (id: number, body: { ok: true; data: unknown } | { ok: false; error: string }) => {
    // The frame has an opaque origin ("null"), which cannot be named as a target; the message goes
    // to that one window object and carries only this widget's declared data.
    deps.frame.postMessage({ v: 1, type: "widget.result", id, ...body }, "*");
  };

  return {
    onMessage(ev) {
      if (!open || ev.source !== deps.frame) return;
      const d = ev.data;
      if (!isObj(d) || d.v !== 1 || d.type !== "widget.query") return;
      if (typeof d.id !== "number" || !Number.isSafeInteger(d.id) || typeof d.query !== "string") return;
      const id = d.id;
      const name = d.query;
      const now = deps.now();
      while (recent.length && now - recent[0] >= 60_000) recent.shift();
      if (recent.length >= QUERIES_PER_MINUTE) {
        deps.onQuery?.(deps.widgetId, name, false);
        reply(id, { ok: false, error: `too many queries: at most ${QUERIES_PER_MINUTE} a minute` });
        return;
      }
      recent.push(now);
      if (!isWidgetQuery(name)) {
        deps.onQuery?.(deps.widgetId, name, false);
        reply(id, { ok: false, error: `${name.slice(0, 64)} is not a widget query` });
        return;
      }
      if (!declared.has(name)) {
        deps.onQuery?.(deps.widgetId, name, false);
        reply(id, { ok: false, error: `${name} was not declared by this widget` });
        return;
      }
      let data: unknown;
      try {
        data = resolveQuery(name as WidgetQuery, deps.sources);
      } catch {
        deps.onQuery?.(deps.widgetId, name, false);
        reply(id, { ok: false, error: `${name} is unavailable right now` });
        return;
      }
      deps.onQuery?.(deps.widgetId, name, true);
      reply(id, { ok: true, data });
    },
    close() {
      open = false;
    },
  };
}
