// K5a — the Store's catalog states and install sheet. Each honest state renders its own words,
// nothing installs without the member's approval, an approved install says it arrived paused,
// and no state shows a listing, a verification or a price that core did not give.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import {
  InstallSheetView,
  CatalogStateBanner,
  ListingCard,
  ListingPage,
  InstalledList,
  CATALOG_STATE_TEXT,
} from "./StoreCatalog";
import { Commissary } from "./Commissary";
import { storeSlice, beginInstall, approveInstall, closeSheet, removeInstall, registerDaemonCanceller, type StoreSheet } from "../shell/slices/store";
import { bridge } from "../bridge";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";
import type { StoreCatalogView, StoreListing, StoreStaged, StoreInstalled } from "../bridge/domains";

const budget = { maxRunsPerDay: 2, maxTokensPerDay: 8000, maxTokensPerRun: 4000, maxSpendSalt: "0" };
const perms = [
  { id: "local_model", reason: "Runs on your local model." },
  { id: "files_read", reason: "Reads the folders you granted." },
];

function staged(patch: Partial<StoreStaged> = {}): StoreStaged {
  return {
    stageId: "a".repeat(32),
    listingId: "daily-brief",
    kind: "daemon",
    name: "Daily brief",
    version: "1.0.0",
    publisher: "Citrate",
    sizeBytes: 120,
    sha256: "ab".repeat(32),
    permissions: perms,
    budget,
    hashCheck: "verified",
    expiresAt: 0,
    ...patch,
  };
}

function sheet(patch: Partial<StoreSheet>): StoreSheet {
  return { listingId: "daily-brief", phase: "review", staged: staged(), refusal: null, installed: null, pct: null, ...patch };
}

const render = (s: StoreSheet) =>
  renderToStaticMarkup(<InstallSheetView sheet={s} listingName="Daily brief" onApprove={() => {}} onClose={() => {}} />);

function listing(patch: Partial<StoreListing> = {}): StoreListing {
  return {
    id: "daily-brief",
    kind: "daemon",
    name: "Daily brief",
    summary: "A morning summary.",
    version: "1.0.0",
    publisher: "Citrate",
    firstParty: true,
    tier: "member",
    sizeBytes: 120,
    sha256: "ab".repeat(32),
    permissions: perms,
    budget,
    locked: false,
    lockReason: null,
    installable: true,
    installNote: null,
    installed: false,
    ...patch,
  };
}

const NO_PRICE = /\$|price|earn|yield|APY|invest/i;

describe("install sheet states", () => {
  it("review: shows the declared permissions and budget, and says it installs paused", () => {
    const out = render(sheet({}));
    expect(out).toContain("Use your local model");
    expect(out).toContain("Reads the folders you granted.");
    expect(out).toContain("Up to 2 runs a day");
    expect(out).toContain("spend 0 SALT");
    expect(out).toContain("installs paused and does nothing until you start it");
    expect(out).toContain("SHA-256 matched the signed catalog");
    expect(out).toContain("Approve and install paused");
    expect(out).not.toMatch(NO_PRICE);
  });

  it("review of a model: the hash is checked on download and the model is not used until selected", () => {
    const out = render(sheet({ staged: staged({ kind: "model", budget: null, hashCheck: "on_download" }) }));
    expect(out).toContain("checked as it downloads");
    expect(out).not.toContain("SHA-256 matched");
    expect(out).toContain("not used until you select it in Models");
    expect(out).toContain("No budget");
  });

  it("staging and installing offer no approval", () => {
    const st = render(sheet({ phase: "staging", staged: null }));
    expect(st).toContain("Checking the signature");
    expect(st).not.toContain("Approve");
    const inst = render(sheet({ phase: "installing", pct: 42 }));
    expect(inst).toContain("42% downloaded");
    expect(inst).not.toContain("Approve");
  });

  it("done: says the install arrived paused", () => {
    const installed: StoreInstalled = {
      listingId: "daily-brief",
      kind: "daemon",
      name: "Daily brief",
      version: "1.0.0",
      target: "d01",
      installedAt: 1,
      state: "paused",
    };
    const out = render(sheet({ phase: "done", installed }));
    expect(out).toContain("Daily brief installed");
    expect(out).toContain("paused, does nothing until you start it");
    expect(out).not.toContain("Approve");
  });

  it.each([
    ["hash_failed", "Hash check failed: nothing was installed"],
    ["tier_locked", "Not included in your membership"],
    ["no_key", "Store catalog not available yet"],
    ["signature_failed", "Signature check failed"],
    ["fetch_failed", "Could not download it"],
    ["not_installable", "Not installable yet"],
    ["stage_expired", "This review expired"],
  ] as const)("refused (%s) shows its own title and the core's reason, with no approval", (code, title) => {
    const out = render(sheet({ phase: "refused", refusal: { code, message: "core says why" } }));
    expect(out).toContain(title);
    expect(out).toContain("core says why");
    expect(out).not.toContain("Approve");
    expect(out).not.toMatch(NO_PRICE);
  });
});

describe("catalog states", () => {
  const view = (state: StoreCatalogView["state"], message: string | null = null): StoreCatalogView => ({
    state,
    message,
    catalogUrl: "https://example.invalid/catalog.json",
    keyFingerprint: null,
    sequence: null,
    expiresAt: null,
    listings: [],
  });

  it("no key: the exact fail-closed message, and no listings", () => {
    const out = renderToStaticMarkup(
      <CatalogStateBanner view={view("no_key", "Store catalog not available yet: signing key not issued")} error={null} />,
    );
    expect(out).toContain("Store catalog not available yet");
    expect(out).toContain("signing key not issued");
  });

  it.each(["fetch_failed", "signature_failed", "invalid", "expired", "rollback", "desktop_only"] as const)(
    "%s has its own words",
    (state) => {
      const out = renderToStaticMarkup(<CatalogStateBanner view={view(state)} error={null} />);
      expect(out).toContain(CATALOG_STATE_TEXT[state].title);
      expect(out).toContain(`data-store-state="${state}"`);
    },
  );

  it("ready shows no banner; a thrown error shows its text", () => {
    expect(renderToStaticMarkup(<CatalogStateBanner view={view("ready")} error={null} />)).toBe("");
    expect(renderToStaticMarkup(<CatalogStateBanner view={null} error="boom" />)).toContain("boom");
  });

  it("a tier-locked listing shows why and offers no install", () => {
    const l = listing({ locked: true, lockReason: "Needs an active membership" });
    expect(renderToStaticMarkup(<ListingCard l={l} onOpen={() => {}} />)).toContain("Needs an active membership");
    const page = renderToStaticMarkup(<ListingPage l={l} installed={null} onBack={() => {}} onInstall={() => {}} />);
    expect(page).toContain("Needs an active membership");
    expect(page).not.toContain("Review and install");
    expect(page).not.toMatch(NO_PRICE);
  });

  it("a kind without an installer says not installable yet and offers no install", () => {
    const l = listing({ kind: "widget", installable: false, installNote: "Not installable yet: widget installs from the Store are not built" });
    const page = renderToStaticMarkup(<ListingPage l={l} installed={null} onBack={() => {}} onInstall={() => {}} />);
    expect(page).toContain("Not installable yet");
    expect(page).not.toContain("Review and install");
  });

  it("an installable listing shows its declared permissions and budget before any install", () => {
    const page = renderToStaticMarkup(<ListingPage l={listing()} installed={null} onBack={() => {}} onInstall={() => {}} />);
    expect(page).toContain("Declared permissions");
    expect(page).toContain("Up to 2 runs a day");
    expect(page).toContain("Review and install");
  });
});

describe("install flow through the slice", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    closeSheet();
  });

  it("a refused stage lands in the refused phase with the core's code", async () => {
    vi.spyOn(bridge.store, "stage").mockResolvedValue({ refused: { code: "hash_failed", message: "does not match" } });
    await beginInstall("daily-brief");
    const s = storeSlice.get().sheet;
    expect(s?.phase).toBe("refused");
    expect(s?.refusal?.code).toBe("hash_failed");
  });

  it("nothing installs until the member approves; then it arrives paused", async () => {
    const install = vi.spyOn(bridge.store, "install").mockResolvedValue({
      ok: { listingId: "daily-brief", kind: "daemon", name: "Daily brief", version: "1.0.0", target: "d01", installedAt: 1, state: "paused" },
    });
    vi.spyOn(bridge.store, "stage").mockResolvedValue({ ok: staged() });
    vi.spyOn(bridge.store, "catalog").mockResolvedValue({
      state: "no_key",
      message: null,
      catalogUrl: "",
      keyFingerprint: null,
      sequence: null,
      expiresAt: null,
      listings: [],
    });
    vi.spyOn(bridge.store, "installed").mockResolvedValue([]);
    await beginInstall("daily-brief");
    expect(storeSlice.get().sheet?.phase).toBe("review");
    expect(install).not.toHaveBeenCalled();
    await approveInstall();
    expect(install).toHaveBeenCalledWith("a".repeat(32), expect.any(Function));
    expect(storeSlice.get().sheet?.phase).toBe("done");
    expect(storeSlice.get().sheet?.installed?.state).toBe("paused");
  });

  it("the web preview says the Store needs the desktop app", async () => {
    const v = await bridge.store.catalog();
    expect(v.state).toBe("desktop_only");
    expect(v.listings).toEqual([]);
  });
});

describe("K5-F: no Remove while an install is in flight", () => {
  const item = (patch: Partial<StoreInstalled> = {}): StoreInstalled => ({
    listingId: "tiny-model",
    kind: "model",
    name: "Tiny model",
    version: "1.0.0",
    target: "tiny-model.gguf",
    installedAt: 1,
    state: "not_selected",
    ...patch,
  });
  const removeButton = (out: string) => out.match(/<button[^>]*>(Remove|Removing…|Installing…)<\/button>/)?.[0] ?? "";

  it("an install core reports as installing offers no Remove", () => {
    const out = renderToStaticMarkup(<InstalledList items={[item({ state: "installing" })]} removing={null} installingId={null} />);
    const btn = removeButton(out);
    expect(btn).toContain("disabled");
    expect(btn).toContain("Installing…");
    expect(out).not.toContain(">Start<");
  });

  it("the listing whose install sheet is still installing offers no Remove", () => {
    const out = renderToStaticMarkup(<InstalledList items={[item({ state: "missing" })]} removing={null} installingId="tiny-model" />);
    expect(removeButton(out)).toContain("disabled");
  });

  it("a finished install keeps its Remove", () => {
    const out = renderToStaticMarkup(<InstalledList items={[item()]} removing={null} installingId={null} />);
    const btn = removeButton(out);
    expect(btn).toContain(">Remove<");
    expect(btn).not.toContain("disabled");
  });

  it("removeInstall refuses locally while that listing is installing, and shows core's refusal otherwise", async () => {
    vi.restoreAllMocks();
    const remove = vi.spyOn(bridge.store, "remove").mockResolvedValue({
      refused: { code: "install_running", message: "It is still installing." },
    });
    vi.spyOn(bridge.store, "catalog").mockResolvedValue({
      state: "no_key", message: null, catalogUrl: "", keyFingerprint: null, sequence: null, expiresAt: null, listings: [],
    });
    vi.spyOn(bridge.store, "installed").mockResolvedValue([]);
    storeSlice.set({
      sheet: { listingId: "tiny-model", phase: "installing", staged: null, refusal: null, installed: null, pct: 10 },
    });
    await removeInstall("tiny-model");
    expect(remove).not.toHaveBeenCalled();
    expect(storeSlice.get().notice).toContain("still installing");
    closeSheet();
    await removeInstall("tiny-model");
    expect(remove).toHaveBeenCalledWith("tiny-model");
    expect(storeSlice.get().notice).toContain("still installing");
  });
});

describe("removing a started daemon stops its run first", () => {
  it("cancels the daemon's run (and waits) before asking core to remove it", async () => {
    vi.restoreAllMocks();
    closeSheet();
    const order: string[] = [];
    registerDaemonCanceller(async (id) => {
      order.push("cancel:" + id);
      await new Promise((r) => setTimeout(r, 5));
      order.push("cancelled:" + id);
    });
    vi.spyOn(bridge.store, "remove").mockImplementation(async (id) => {
      order.push("remove:" + id);
      return { ok: "removed" };
    });
    vi.spyOn(bridge.store, "catalog").mockResolvedValue({
      state: "no_key", message: null, catalogUrl: "", keyFingerprint: null, sequence: null, expiresAt: null, listings: [],
    });
    vi.spyOn(bridge.store, "installed").mockResolvedValue([]);
    storeSlice.set({
      installed: [{ listingId: "daily-brief", kind: "daemon", name: "Daily brief", version: "1.0.0", target: "d0a", installedAt: 1, state: "active" }],
    });
    await removeInstall("daily-brief");
    expect(order).toEqual(["cancel:d0a", "cancelled:d0a", "remove:daily-brief"]);
  });

  it("a cancel that fails stops the removal and says why", async () => {
    vi.restoreAllMocks();
    registerDaemonCanceller(async () => {
      throw new Error("could not stop it");
    });
    const remove = vi.spyOn(bridge.store, "remove").mockResolvedValue({ ok: "removed" });
    storeSlice.set({
      installed: [{ listingId: "daily-brief", kind: "daemon", name: "Daily brief", version: "1.0.0", target: "d0a", installedAt: 1, state: "active" }],
    });
    await removeInstall("daily-brief");
    expect(remove).not.toHaveBeenCalled();
    expect(storeSlice.get().notice).toContain("could not stop it");
    registerDaemonCanceller(null);
  });
});

describe("the surface", () => {
  const stub = { setState: () => {}, save: () => {}, copy: () => {} } as unknown as Store;
  it.each(["catalog", "apps", "sdks", "docs", "services"])(
    "is labelled Store and shows only the signed catalog, whatever the saved tab (%s)",
    (cTab) => {
      const out = renderToStaticMarkup(<Commissary store={stub} s={{ ...freshState("p1"), cTab, tier: "enterprise" }} />);
      expect(out).toContain(">Store<");
      expect(out).not.toContain(">Commissary<");
      // The local seed is gone from under the Store heading: no seed tabs, cards or actions.
      for (const seed of ["local seed", ">SDKs<", ">Services<", "Download", "Open panel", "Lattice Observatory", "Requires "]) {
        expect(out).not.toContain(seed);
      }
      expect(out).not.toContain("$48");
    },
  );
});
