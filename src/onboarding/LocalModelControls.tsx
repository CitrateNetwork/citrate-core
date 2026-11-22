// The local-model download controls, shared by the onboarding model step and Settings › Models,
// so the model can be downloaded, paused, resumed, retried or swapped for the smaller one from
// either place. Every figure is a real one from `model.status()` / `model.options()` (bytes on
// disk, measured speed, the backend's retry timer, the disk's free space); nothing is estimated
// in the webview. Copy: plain words, no em-dashes.
import type { Store } from "../shell/store";
import type { AppState } from "../shell/state";
import { LOCAL_MODELS, diskTooSmall, etaText, gb, speedText } from "../shell/localModels";

export function LocalModelControls({ store, s, offerSkip }: { store: Store; s: AppState; offerSkip: boolean }) {
  const chosen = LOCAL_MODELS[s.modelChoice];
  const other = s.modelChoice === "default" ? LOCAL_MODELS.small : LOCAL_MODELS.default;
  const total = s.modelTotalBytes > 0 ? s.modelTotalBytes : chosen.sizeBytes;
  const pct = total > 0 ? Math.min(100, Math.round((s.modelDownloadedBytes / total) * 1000) / 10) : 0;
  const lowDisk = diskTooSmall(s.modelFreeBytes, s.modelChoice, s.modelDownloadedBytes);
  const smallFits = !diskTooSmall(s.modelFreeBytes, "small");
  const offerSmaller = s.modelChoice === "default" && smallFits;
  const smallerButton = offerSmaller ? (
    <button className="btn btn-ghost" data-testid="model-choose-smaller" onClick={() => store.chooseSmallerModel()}>
      Choose a smaller model ({gb(LOCAL_MODELS.small.sizeBytes)} GB)
    </button>
  ) : null;
  const skipButton =
    offerSkip && !s.modelSkipped ? (
      <button className="btn btn-ghost" data-testid="model-skip" onClick={() => store.skipModel()}>
        Skip for now, use the gateway
      </button>
    ) : null;

  const diskNote =
    lowDisk && s.modelFreeBytes !== null ? (
      <div className="mono" style={{ fontSize: 11.5, color: "var(--danger)" }} data-testid="model-low-disk">
        This disk has {gb(s.modelFreeBytes)} GB free; {chosen.label} needs {gb(chosen.sizeBytes - s.modelDownloadedBytes)} GB more.
        {offerSmaller ? " The smaller model may fit." : " Free up space, then retry."}
      </div>
    ) : null;

  if (s.modelState === "ready") return null;

  if (s.modelState === "notPresent" || s.modelState === "error") {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }} data-testid="model-start">
        {s.modelState === "error" && s.modelError && (
          <div role="alert" style={{ fontSize: 12.5, color: "var(--danger)", lineHeight: 1.5 }} data-testid="model-error">
            {s.modelError}
          </div>
        )}
        {diskNote}
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button className="btn btn-primary" data-testid="model-download" onClick={() => store.startModelDownload()}>
            {s.modelState === "error" ? "Retry download" : `Download ${chosen.label} (${gb(chosen.sizeBytes)} GB)`}
          </button>
          {smallerButton}
          {s.modelChoice === "small" && (
            <button className="btn btn-ghost" data-testid="model-choose-default" onClick={() => store.chooseDefaultModel()}>
              Use {other.label} instead ({gb(other.sizeBytes)} GB)
            </button>
          )}
          {skipButton}
        </div>
      </div>
    );
  }

  if (s.modelState === "paused") {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }} data-testid="model-paused">
        <div style={{ fontSize: 14, fontWeight: 500 }}>Paused at {pct}%</div>
        <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-2)" }} data-testid="model-bytes">
          {gb(s.modelDownloadedBytes)} / {gb(total)} GB · your progress is saved
        </span>
        {diskNote}
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button className="btn btn-primary" data-testid="model-resume" onClick={() => store.startModelDownload()}>
            Resume
          </button>
          {smallerButton}
          {skipButton}
        </div>
      </div>
    );
  }

  if (s.modelState === "verifying") {
    return (
      <div style={{ fontSize: 13, color: "var(--tx-2)" }} data-testid="model-verifying-inline">
        Checking the file against its published SHA-256.
      </div>
    );
  }

  // downloading
  const waiting = s.modelRetryInSecs !== null;
  const facts = waiting
    ? `Waiting for the connection. Trying again in ${Math.max(1, s.modelRetryInSecs ?? 0)}s.`
    : [
        s.modelBytesPerSec ? speedText(s.modelBytesPerSec) : null,
        s.modelEtaSecs !== null ? etaText(s.modelEtaSecs) : null,
      ]
        .filter(Boolean)
        .join(" · ");
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }} data-testid="model-downloading">
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <div style={{ fontSize: 14, fontWeight: 500 }}>Downloading {chosen.label}</div>
        <div className="mono tabular" style={{ fontSize: 13, color: "var(--accent-text)" }} data-testid="model-pct">
          {pct}%
        </div>
      </div>
      <div style={{ height: 6, background: "var(--srf-inset)", borderRadius: 999, overflow: "hidden", border: "1px solid var(--line-1)" }}>
        <div style={{ height: "100%", background: "var(--accent)", width: pct + "%", transition: "width .5s var(--ease-standard)" }}></div>
      </div>
      <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-2)" }} data-testid="model-bytes">
        {gb(s.modelDownloadedBytes)} / {gb(total)} GB{facts ? ` · ${facts}` : ""}
      </span>
      {waiting && s.modelRetryReason && (
        <span style={{ fontSize: 12, color: "var(--tx-3)", lineHeight: 1.5 }} data-testid="model-retry-reason">
          {s.modelRetryReason}
        </span>
      )}
      <div style={{ display: "flex", gap: 10, flexWrap: "wrap", marginTop: 2 }}>
        <button className="btn btn-ghost" data-testid="model-pause" onClick={() => store.pauseModelDownload()}>
          Pause
        </button>
        {smallerButton}
        {skipButton}
      </div>
    </div>
  );
}
