// =====================================================================
// citrate-core — widgets API (HUP-S10.3)
//
// The Rust `widget*` commands (src-tauri/src/widgets.rs) and the widget document URL. Desktop app
// only: in the web preview there is no widget store and no widget scheme, and `widgetsApi()` is null.
// =====================================================================
import { convertFileSrc } from "@tauri-apps/api/core";
import { invoke } from "../bridge/tauri/invoke";
import { BRIDGE_MODE } from "../bridge/mode";
import { createSlice } from "../shell/slices/createSlice";

export type WidgetAuthor = "member" | "hermes" | "gallery";

export interface WidgetMeta {
  id: string;
  name: string;
  description: string;
  queries: string[];
  author: WidgetAuthor;
  createdMs: number;
  bytes: number;
}

export interface WidgetInput {
  id?: string;
  name: string;
  description: string;
  html: string;
  queries: string[];
  author: WidgetAuthor;
}

export interface WidgetsApi {
  list(): Promise<WidgetMeta[]>;
  save(input: WidgetInput): Promise<WidgetMeta>;
  remove(id: string): Promise<void>;
  source(id: string): Promise<string>;
}

export const tauriWidgetsApi: WidgetsApi = {
  list: () => invoke<WidgetMeta[]>("widgets_list"),
  save: (input) => invoke<WidgetMeta>("widget_save", { input, nowMs: Date.now() }),
  remove: (id) => invoke<void>("widget_delete", { id }),
  source: (id) => invoke<string>("widget_source", { id }),
};

/** The API in the desktop app; null in the web preview. */
export function widgetsApi(): WidgetsApi | null {
  return BRIDGE_MODE === "tauri" ? tauriWidgetsApi : null;
}

/** The scheme Rust serves widget documents on (`widgets::SCHEME`). */
export const WIDGET_SCHEME = "citrate-widget";

/** A widget document's URL: `citrate-widget://localhost/<id>` (Windows: `http://citrate-widget.localhost/<id>`). */
export function widgetUrl(id: string): string {
  return convertFileSrc(id, WIDGET_SCHEME);
}

export interface WidgetsState {
  list: WidgetMeta[];
  loaded: boolean;
  error: string | null;
}

export const widgetsSlice = createSlice<WidgetsState>({ list: [], loaded: false, error: null });

/** Re-read the saved widgets. */
export async function refreshWidgets(): Promise<void> {
  const api = widgetsApi();
  if (!api) {
    widgetsSlice.set({ list: [], loaded: true, error: null });
    return;
  }
  try {
    widgetsSlice.set({ list: await api.list(), loaded: true, error: null });
  } catch (e) {
    widgetsSlice.set({ loaded: true, error: e instanceof Error ? e.message : String(e) });
  }
}
