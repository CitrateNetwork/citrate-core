// Five-place navigation (0.5.2): each place renders its tab strip and the existing surface for the
// tab. Tabs with no content yet render an honest "not available yet" state, never invented data.
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { WagmiProvider } from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig } from "../wagmi";
import { PlaceView } from "./PlaceView";
import { Sidebar } from "./Sidebar";
import { store } from "./store";
import { freshState, type AppState } from "./state";
import { locate } from "./places";

function at(route: string, patch: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  return { ...s, stage: "done", route, ...patch };
}

// The app's real providers (Hermes › Chat reads the chain through wagmi). A static render runs no
// effects, so nothing is fetched.
const html = (route: string, patch: Partial<AppState> = {}) =>
  renderToStaticMarkup(
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={new QueryClient()}>
        <PlaceView store={store} s={at(route, patch)} />
      </QueryClientProvider>
    </WagmiProvider>,
  );

/** The labels of a rendered tab strip, in order. */
function tabs(markup: string, list: "place-tabs" | "place-subtabs"): string[] {
  const m = markup.match(new RegExp(`<div[^>]*data-testid="${list}"[^>]*>(.*?)</div>`));
  if (!m) return [];
  return [...m[1].matchAll(/<button[^>]*>([^<]*)<\/button>/g)].map((x) => x[1]);
}

/** The selected tab labels in the place strip and the sub-tab strip (surfaces' own tabs are ignored). */
function selected(markup: string): string[] {
  const out: string[] = [];
  for (const list of ["place-tabs", "place-subtabs"]) {
    const m = markup.match(new RegExp(`<div[^>]*data-testid="${list}"[^>]*>(.*?)</div>`));
    if (m) out.push(...[...m[1].matchAll(/<button[^>]*aria-selected="true"[^>]*>([^<]*)<\/button>/g)].map((x) => x[1]));
  }
  return out;
}

describe("Hermes", () => {
  it("shows Chat, Journal, Work, Learn, Controls with Chat selected on the old dashboard route", () => {
    const h = html("dashboard");
    expect(tabs(h, "place-tabs")).toEqual(["Chat", "Journal", "Work", "Learn", "Controls"]);
    expect(selected(h)).toEqual(["Chat"]);
    expect(h).toContain('data-surface="hermes/chat"');
  });

  it("Work hosts the Agent surface and Journal the Journal surface", () => {
    expect(html("agent")).toContain('data-surface="hermes/work"');
    expect(selected(html("agent"))).toEqual(["Work"]);
    expect(html("journal")).toContain('data-surface="hermes/journal"');
  });

  it("Work composes the Agent surface with daemons and the daemon run log, without the chat or the learning panel", () => {
    const h = html("hermes/work");
    expect(h).toContain('data-surface="hermes/work"');
    expect(h).toContain("Awaiting your review");
    expect(h).toContain('data-testid="daemons-panel"');
    expect(h).toContain('data-testid="daemon-run-log"');
    expect(h).toContain('data-testid="approval-records-link"');
    // Chat lives on Hermes › Chat, learning on Hermes › Learn, MCP servers on Controls › Details.
    expect(h).not.toContain('data-testid="agent-chat"');
    expect(h).not.toContain('data-testid="learned-panel"');
    expect(h).not.toContain('data-testid="hermes-mcp-card"');
    // Ruling 5: daemons never show "0 SALT" (the desktop wording is covered in panels.test.tsx).
    expect(h).not.toMatch(/\d\s*SALT/);
  });

  it("Work carries the approvals badge in the tab strip when approvals wait", async () => {
    const { agentSlice } = await import("./slices/agent");
    const before = agentSlice.get().approvals;
    agentSlice.set({ approvals: [{ id: "a1", kind: "code", summary: "Write a file" }] });
    try {
      const h = html("hermes/chat");
      expect(h).toMatch(/data-testid="place-tabs"[\s\S]*?Work[\s\S]*?aria-label="1 awaiting your review"/);
    } finally {
      agentSlice.set({ approvals: before });
    }
  });

  it("Learn holds learned skills and Teach Hermes, knowledge import and the training opt-in", () => {
    const h = html("hermes/learn");
    expect(h).toContain('data-surface="hermes/learn"');
    expect(h).not.toContain('data-not-available="hermes/learn"');
    expect(h).toContain('data-testid="learned-panel"');
    expect(h).toContain('data-testid="learn-knowledge-import"');
    expect(h).toContain('data-testid="fl-trajectories"');
    expect(h).toContain("Group rounds not live yet");
  });

  it("Controls has three depths as sub-tabs, Switches selected on hermes/controls", () => {
    const h = html("hermes/controls");
    expect(tabs(h, "place-subtabs")).toEqual(["1 · Switches", "2 · Details", "3 · Records"]);
    expect(selected(h)).toEqual(["Controls", "1 · Switches"]);
    expect(h.match(/data-testid="hermes-controls"/g)?.length).toBe(1);
    // Depth one is only the switches: no Settings section is mounted under it.
    expect(h).not.toContain("Provider keys");
  });

  it("settings/hermes still opens the switches", () => {
    const h = html("settings/hermes", { sSec: "hermes" });
    expect(selected(h)).toEqual(["Controls", "1 · Switches"]);
    expect(h).toContain('data-testid="hermes-controls"');
  });

  it("Details hosts the Settings sections for Hermes, and nothing account-level", () => {
    const h = html("hermes/controls/details", { sSec: "escalation" });
    expect(selected(h)).toEqual(["Controls", "2 · Details"]);
    expect(h).not.toContain('data-testid="hermes-controls"');
    for (const label of ["Providers &amp; keys", "Escalation &amp; spend caps", "MCP servers", "Web search &amp; decisions", "Terminal &amp; folders", "Budgets", "Personas &amp; voice"]) expect(h).toContain(label);
    for (const label of ["Account &amp; RBAC", "Memberships &amp; billing", "Node configuration", "Hermes controls"]) expect(h).not.toContain(label);
  });

  it("Details › MCP servers shows Hermes's connected tools and the member's own servers", () => {
    const h = html("hermes/controls/details", { sSec: "mcp" });
    expect(h).toContain('data-testid="hermes-mcp-card"');
  });

  it("Details opens on its first section when the stored one is not a Details section", () => {
    const h = html("hermes/controls/details", { sSec: "account" });
    expect(h).toContain("Provider keys");
  });

  it("Records hosts the third depth", () => {
    const h = html("hermes/controls/records");
    expect(selected(h)).toEqual(["Controls", "3 · Records"]);
    expect(h).toContain('data-testid="controls-records"');
  });
});

describe("Groups", () => {
  it("shows Chat, Rounds, Members, Cluster, with Groups and Alerts under Chat", () => {
    const h = html("groups");
    expect(tabs(h, "place-tabs")).toEqual(["Chat", "Rounds", "Members", "Cluster"]);
    expect(tabs(h, "place-subtabs")).toEqual(["Groups", "Alerts"]);
    expect(selected(h)).toEqual(["Chat", "Groups"]);
  });

  it("the old Training, Members, Cluster and Alerts routes open their tabs", () => {
    expect(selected(html("train"))).toEqual(["Rounds"]);
    expect(selected(html("people"))).toEqual(["Members"]);
    expect(selected(html("cluster"))).toEqual(["Cluster"]);
    expect(selected(html("comms"))).toEqual(["Chat", "Alerts"]);
    expect(html("comms")).toContain('data-surface="groups/chat/alerts"');
  });

  it("ALF is a tab only for ALF members", () => {
    expect(tabs(html("groups", { alfMember: true }), "place-tabs")).toContain("ALF");
    expect(tabs(html("groups", { alfMember: false }), "place-tabs")).not.toContain("ALF");
  });
});

describe("Wallet", () => {
  it("is the Wallet surface with no tab strip", () => {
    const h = html("wallet");
    expect(h).toContain('data-surface="wallet"');
    expect(tabs(h, "place-tabs")).toEqual([]);
  });
});

describe("Node", () => {
  it("shows Status, Services, Storage, Rewards, Advanced with Status selected on the old node route", () => {
    const h = html("node");
    expect(tabs(h, "place-tabs")).toEqual(["Status", "Services", "Storage", "Rewards", "Advanced"]);
    expect(selected(h)).toEqual(["Status"]);
    expect(h).toContain('data-surface="node/status"');
  });

  it("Services renders the services registry snapshot (K1a-2); Rewards is an honest not-available-yet state", () => {
    const services = html("node/services");
    expect(services).not.toContain("data-not-available");
    expect(services).toContain('data-testid="node-services"');
    expect(html("node/rewards")).toContain('data-not-available="node/rewards"');
  });

  it("the earlier node/earnings route opens Rewards", () => {
    const h = html("node/earnings");
    expect(selected(h)).toEqual(["Rewards"]);
    expect(h).toContain('data-not-available="node/rewards"');
    expect(h).not.toContain(">Earnings<");
  });

  it("Storage holds Files, Memory, Pins and Backup (K4); the old files, storage, pins and backup routes open them", () => {
    const f = html("files");
    expect(tabs(f, "place-subtabs")).toEqual(["Files", "Memory", "Pins", "Backup"]);
    expect(selected(f)).toEqual(["Storage", "Files"]);
    expect(selected(html("storage"))).toEqual(["Storage", "Memory"]);
    // K4 shipped real Pins and Backup surfaces: no "not available" card any more.
    expect(html("node/storage/pins")).not.toContain("data-not-available");
    expect(selected(html("pins"))).toEqual(["Storage", "Pins"]);
    expect(html("node/storage/backup")).not.toContain("data-not-available");
    expect(selected(html("backup"))).toEqual(["Storage", "Backup"]);
  });

  it("Advanced hosts Node configuration from Settings, and only that", () => {
    const h = html("node/advanced", { sSec: "account" });
    expect(h).toContain("Node configuration");
    expect(h).not.toContain("Account &amp; RBAC");
  });
});

describe("Store", () => {
  it("shows Apps and Models; the old commissary and models routes open them", () => {
    const h = html("commissary");
    expect(tabs(h, "place-tabs")).toEqual(["Apps", "Models"]);
    expect(selected(h)).toEqual(["Apps"]);
    expect(selected(html("models"))).toEqual(["Models"]);
  });
});

describe("Settings", () => {
  it("keeps only the account-level sections, even if a moved section was last open", () => {
    const h = html("settings", { sSec: "ai" });
    for (const label of ["Account &amp; RBAC", "Connections", "API endpoints &amp; keys", "Keys &amp; security", "Memberships &amp; billing", "Privacy &amp; recovery", "App"]) expect(h).toContain(label);
    for (const label of ["Hermes controls", "Providers &amp; keys", "Escalation &amp; spend caps", "MCP servers", "Web search &amp; decisions", "Terminal &amp; folders", "Personas &amp; voice", "Node configuration"]) expect(h).not.toContain(">" + label + "<");
  });
});

describe("Sidebar", () => {
  const side = (route: string, patch: Partial<AppState> = {}) => renderToStaticMarkup(<Sidebar store={store} s={at(route, patch)} />);
  const items = (markup: string) => [...markup.matchAll(/<a[^>]*href="#\/([^"]*)"[^>]*>.*?<span[^>]*>([^<]*)<\/span>/g)].map((m) => [m[1], m[2]]);

  it("lists the five places then Settings, and nothing else", () => {
    expect(items(side("dashboard"))).toEqual([
      ["hermes/chat", "Hermes"],
      ["groups/chat", "Groups"],
      ["wallet", "Wallet"],
      ["node/status", "Node"],
      ["store/apps", "Store"],
      ["settings", "Settings"],
    ]);
  });

  it("does not list ALF as a sixth place for members (it is a Groups tab)", () => {
    expect(items(side("dashboard", { alfMember: true })).map((x) => x[1])).not.toContain("ALF");
  });

  it("highlights the place that holds an old route", () => {
    const h = side("files");
    expect(h).toMatch(/<a[^>]*href="#\/node\/status"[^>]*aria-current="page"/);
    expect(side("settings/ai")).toMatch(/<a[^>]*href="#\/hermes\/chat"[^>]*aria-current="page"/);
  });
});

describe("Groups › Chat holds the invite banners (#355 on five-place routes)", () => {
  // handleDeepLink's go("groups") lands on the canonical leaf; both invite banners must render there.
  const leaf = locate("groups").route;

  it("the groups route resolves to the Groups › Chat leaf", () => {
    expect(leaf).toBe("groups/chat");
    expect(html(leaf)).toContain('data-surface="groups/chat"');
  });

  it("a v2 invite held in Rust shows the waiting-invite card", () => {
    const h = html(leaf, { pendingJoin: { phase: "waiting", hasKey: true, persisted: true } });
    expect(h).toContain('data-testid="pending-join-cancel"');
    expect(html(leaf)).not.toContain('data-testid="pending-join-cancel"');
  });

  it("a legacy citrate://invite link shows the Join banner", () => {
    const h = html(leaf, { pendingInvite: { url: "citrate://invite?g=grp_1&t=tok123&k=02ab" } });
    expect(h).toContain("invited you to");
    expect(html(leaf)).not.toContain("invited you to");
  });
});
