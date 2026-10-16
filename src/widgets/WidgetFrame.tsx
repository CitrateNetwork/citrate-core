// =====================================================================
// citrate-core — one widget tile (HUP-S10.3, US-10.3 AC1)
//
// A sandboxed iframe on the widget scheme (`citrate-widget://localhost/<id>`, served by Rust with a
// strict CSP to the main window only). The sandbox allows scripts and nothing else: no same-origin
// (an opaque origin, so no storage, cookies or app IPC), no popups, no forms, no top navigation,
// no permissions. The only way out is postMessage to this window, which the widget host answers
// for the widget's declared, read-only catalog queries.
// =====================================================================
import { useEffect, useRef } from "react";
import { createWidgetHost } from "./host";
import { widgetUrl, type WidgetMeta } from "./api";
import type { WidgetSources } from "./catalog";

/** The iframe's sandbox flags: scripts only. */
export const WIDGET_SANDBOX = "allow-scripts";

export function WidgetFrame({
  widget,
  sources,
  height = 96,
  onQuery,
}: {
  widget: WidgetMeta;
  sources: WidgetSources;
  height?: number;
  onQuery?: (widgetId: string, query: string, answered: boolean) => void;
}) {
  const ref = useRef<HTMLIFrameElement | null>(null);
  const declaredKey = widget.queries.join(",");

  useEffect(() => {
    const frame = ref.current?.contentWindow;
    if (!frame) return;
    const host = createWidgetHost({
      widgetId: widget.id,
      declared: declaredKey ? declaredKey.split(",") : [],
      frame,
      sources,
      now: () => Date.now(),
      onQuery,
    });
    const listener = (ev: MessageEvent) => host.onMessage({ source: ev.source, data: ev.data });
    window.addEventListener("message", listener);
    return () => {
      host.close();
      window.removeEventListener("message", listener);
    };
  }, [widget.id, declaredKey, sources, onQuery]);

  return (
    <iframe
      ref={ref}
      title={`Widget: ${widget.name}`}
      src={widgetUrl(widget.id)}
      sandbox={WIDGET_SANDBOX}
      referrerPolicy="no-referrer"
      allow=""
      loading="lazy"
      style={{ width: "100%", height, border: "none", display: "block", background: "transparent" }}
    />
  );
}
