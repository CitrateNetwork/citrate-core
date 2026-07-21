// Q-A.4a — the Storage honesty pass (Rule 1 / display honesty).
//
// The two Rule-1 violations on this surface were:
//   1. a FABRICATED "semantic search download": a Math.random() progress bar that
//      claimed "sha256 verified on completion" against an embedding model that was
//      NEVER fetched. The bge model is bundled WITH the mem-mcp daemon, so semantic
//      availability must be a REAL read from memory_status().semantic — never a
//      fake download, never a "sha256 verified" claim on nothing.
//   2. a FABRICATED socket path: a client-invented ~/.citrate/core/memory/<persona>
//      .sock constant, not the real daemon socket. The MCP panel must show the REAL
//      memory_status().socketPath, or (daemon offline) an honest "start it" state.
//
// These tests are the RED-then-GREEN tripwires: they FAIL if the fabricated
// download bar / "sha256 verified" / "Downloading model…" / the client socket
// constant ever renders, and prove the honest states hold.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Storage } from "./Storage";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// A no-op store stub — Storage renders from `s`; effects/handlers don't fire in
// static markup, so only the render matters here.
const stubStore = {
  setState: () => {},
  copy: () => {},
  refreshMemoryStatus: async () => "offline" as const,
  refreshConstellation: async () => {},
  startMemoryDaemon: async () => {},
} as unknown as Store;

function storageState(patch: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  return { ...s, ...patch };
}

describe("Storage honesty — no fabricated semantic download / 'sha256 verified' (Rule 1)", () => {
  it("NEVER renders the fake download bar, the 'Downloading model…' state, or a 'sha256 verified' claim", () => {
    // Even with the (now-removed) legacy storageMode="dl" set, the fabricated
    // download UI must not exist anywhere in the surface.
    const html = renderToStaticMarkup(
      <Storage store={stubStore} s={storageState({ storageMode: "dl", modelPct: 47, memDaemon: "offline" })} />,
    );
    expect(html).not.toContain("sha256 verified");
    expect(html).not.toContain("Downloading model");
    expect(html).not.toContain("of 440 MB");
    expect(html).not.toContain("Enable semantic search");
  });

  it("does NOT claim semantic is available when the daemon reports semantic:false (memSemantic=false)", () => {
    const html = renderToStaticMarkup(
      <Storage store={stubStore} s={storageState({ memDaemon: "running", memSemantic: false })} />,
    );
    // No "Semantic — available/ready" claim without the daemon's real flag.
    expect(html).not.toContain("Semantic — available");
    expect(html).not.toContain("Semantic — ready");
    // The honest not-yet-available copy shows instead.
    expect(html).toContain("not yet");
  });

  it("shows 'Semantic — available' ONLY when the daemon reports semantic:true (real read)", () => {
    const html = renderToStaticMarkup(
      <Storage store={stubStore} s={storageState({ memDaemon: "running", memSemantic: true })} />,
    );
    expect(html).toContain("Semantic — available");
    // Still no fabricated download claim.
    expect(html).not.toContain("sha256 verified");
  });
});

describe("Storage honesty — MCP endpoint shows the REAL socket, never a client constant (Rule 1)", () => {
  it("daemon offline (memSocketPath=null): shows the honest 'start the memory daemon' state, NOT a fabricated socket", () => {
    const s = storageState({ memDaemon: "offline", memSocketPath: null });
    const html = renderToStaticMarkup(<Storage store={stubStore} s={s} />);
    // The old client-invented constant must NOT render.
    expect(html).not.toContain(s.socketPath);
    expect(html).not.toContain(".citrate/core/memory/");
    // The honest state shows instead.
    expect(html).toContain("start the memory daemon to get your socket path");
    // And 'mcp_connect' (a shim that doesn't exist) must not be referenced.
    expect(html).not.toContain("mcp_connect");
  });

  it("daemon running: shows the REAL daemon-reported socket path, never the client constant", () => {
    const real = "/run/user/1000/citrate/mem-abc123.sock";
    const s = storageState({ memDaemon: "running", memSocketPath: real });
    const html = renderToStaticMarkup(<Storage store={stubStore} s={s} />);
    expect(html).toContain(real);
    // The client-invented default (from freshState) is NOT what renders.
    expect(html).not.toContain(s.socketPath);
    expect(html).not.toContain("mcp_connect");
  });
});
