// =====================================================================
// CORE-AI1 (@rule8) — the key editor of one "Providers & keys" row.
//
// The key, model and base URL fields are controlled inputs whose state lives in this component
// only, so a re-render of Settings (the store re-renders it on every poll: node height, peers,
// sync) never touches what the member is typing. The key is handed to core once on Save (sealed in
// the OS keyring) and is never put in AppState or storage; it is dropped when the editor closes.
// The fields start from the preset once, when the editor opens, and the key field takes focus
// once, at that moment.
// =====================================================================
import { useEffect, useRef, useState } from "react";

export interface AiProviderEditorProps {
  /** The preset's fixed https endpoint, or null for the generic OpenAI-compatible preset. */
  fixedBaseUrl: string | null;
  /** The preset's default model ("" = the member names one). */
  defaultModel: string;
  keyPlaceholder: string;
  name: string;
  onSave(v: { key: string; baseUrl: string; model: string }): void;
  onCancel(): void;
}

export function AiProviderEditor(p: AiProviderEditorProps) {
  const [key, setKey] = useState("");
  const [model, setModel] = useState(p.defaultModel);
  const [baseUrl, setBaseUrl] = useState("");
  const keyRef = useRef<HTMLInputElement | null>(null);
  useEffect(() => {
    try {
      keyRef.current?.focus();
    } catch {
      /* focus is a convenience */
    }
  }, []);
  const field = { height: 30, fontSize: 12 } as const;
  return (
    <>
      {p.fixedBaseUrl === null && (
        <input
          className="input mono"
          aria-label={`${p.name} base URL`}
          placeholder="https://…/v1"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          style={{ ...field, width: 220 }}
        />
      )}
      <input
        className="input mono"
        aria-label={`${p.name} model`}
        placeholder="model (e.g. gpt-4o-mini)"
        value={model}
        onChange={(e) => setModel(e.target.value)}
        style={{ ...field, width: 180 }}
      />
      <input
        ref={keyRef}
        className="input mono"
        type="password"
        autoComplete="new-password"
        aria-label={`${p.name} API key`}
        placeholder={p.keyPlaceholder}
        value={key}
        onChange={(e) => setKey(e.target.value)}
        style={{ ...field, width: 220 }}
      />
      <button className="btn btn-secondary btn-sm" onClick={() => p.onSave({ key: key.trim(), baseUrl: baseUrl.trim(), model: model.trim() })}>
        Save key
      </button>
      <button className="btn btn-ghost btn-sm" onClick={p.onCancel}>
        Cancel
      </button>
    </>
  );
}
