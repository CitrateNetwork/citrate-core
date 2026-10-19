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

  it("Learn is an honest not-available-yet state", () => {
    const h = html("hermes/learn");
    expect(h).toContain('data-not-available="hermes/learn"');
    expect(h).toContain("Not available yet");
  });

  it("Controls hosts the five moved Settings sections and nothing account-level", () => {
    const h = html("hermes/controls", { sSec: "escalation" });
    expect(h).toContain('data-surface="hermes/controls"');
    for (const label of ["AI providers", "Escalation &amp; spend", "MCP servers", "Web search &amp; decisions", "Budgets"]) expect(h).toContain(label);
    for (const label of ["Account &amp; RBAC", "Memberships &amp; billing", "Node configuration"]) expect(h).not.toContain(label);
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
  it("shows Status, Services, Storage, Earnings, Advanced with Status selected on the old node route", () => {
    const h = html("node");
    expect(tabs(h, "place-tabs")).toEqual(["Status", "Services", "Storage", "Earnings", "Advanced"]);
    expect(selected(h)).toEqual(["Status"]);
    expect(h).toContain('data-surface="node/status"');
  });

  it("Services and Earnings are honest not-available-yet states", () => {
    expect(html("node/services")).toContain('data-not-available="node/services"');
    expect(html("node/earnings")).toContain('data-not-available="node/earnings"');
  });

  it("Storage holds Files, Memory and Pins; the old files and storage routes open them", () => {
    const f = html("files");
    expect(tabs(f, "place-subtabs")).toEqual(["Files", "Memory", "Pins"]);
    expect(selected(f)).toEqual(["Storage", "Files"]);
    expect(selected(html("storage"))).toEqual(["Storage", "Memory"]);
    expect(html("node/storage/pins")).toContain('data-not-available="node/storage/pins"');
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
    for (const label of ["AI providers", "Escalation &amp; spend", "MCP servers", "Web search &amp; decisions", "Node configuration"]) expect(h).not.toContain(">" + label + "<");
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
