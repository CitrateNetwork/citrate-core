// @vitest-environment node
//
// HUP gate g3-licence: scripts/third-party-notices.mjs turns cargo-about and go-licenses output
// into the bundled THIRD-PARTY-NOTICES.txt. The scanners are not run here (they need the
// federation checkouts and the network); these tests feed the normalisers the scanners' real
// output shapes, one rule at a time, and check the committed notices file against its config.
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  PERMISSIVE,
  formatInventory,
  isFirstParty,
  noticeComponents,
  normaliseCargoAbout,
  normaliseCollected,
  normaliseGoLicenses,
  readConfig,
  renderNotices,
  splitCopyright,
  textKey,
  updateInventory,
} from "./third-party-notices.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const SCRIPT = path.join(here, "third-party-notices.mjs");

const MIT = (who) => `Copyright (c) ${who}\n\nPermission is hereby granted, free of charge, to any person obtaining a copy.\n`;
const APACHE = "\n   Apache License\n   Version 2.0, January 2004\n\n   TERMS AND CONDITIONS\n";
const REG = "registry+https://github.com/rust-lang/crates.io-index";

const crate = (name, version, source = REG, extra = {}) => ({ name, version, id: `${source}#${name}@${version}`, source, ...extra });

/** cargo-about --format json, trimmed to the fields the normaliser reads. */
function aboutJson() {
  const tokio = crate("tokio", "1.40.0", REG, { repository: "https://github.com/tokio-rs/tokio" });
  const mio = crate("mio", "1.0.2");
  const serde = crate("serde", "1.0.210");
  const mine = crate("agent-sidecar", "0.1.0", null);
  const chain = crate("citrate-types", "0.4.0", "git+https://github.com/CitrateNetwork/citrate-chain.git?rev=f4a0#f4a0");
  return {
    overview: [],
    crates: [
      { package: tokio, license: "MIT" },
      { package: mio, license: "MIT" },
      { package: serde, license: "MIT OR  Apache-2.0" },
      { package: mine, license: "Apache-2.0" },
      { package: chain, license: "BUSL-1.1" },
    ],
    licenses: [
      { id: "MIT", name: "MIT License", text: MIT("Tokio Contributors"), used_by: [{ crate: tokio }] },
      { id: "MIT", name: "MIT License", text: MIT("Carl Lerche"), used_by: [{ crate: mio }] },
      { id: "Apache-2.0", name: "Apache License 2.0", text: APACHE, used_by: [{ crate: serde }, { crate: mine }] },
      // The same Apache text re-wrapped: dedupes with the one above.
      { id: "Apache-2.0", name: "Apache License 2.0", text: APACHE.replace(/\n/g, "\n\n"), used_by: [] },
      { id: "BUSL-1.1", name: "Business Source License 1.1", text: "BUSL text", used_by: [{ crate: chain }] },
    ],
  };
}

const FIRST_PARTY = ["git+https://github.com/CitrateNetwork/"];

describe("normaliseCargoAbout", () => {
  it("keeps third-party crates with their texts and leaves first-party crates out", () => {
    const n = normaliseCargoAbout(aboutJson(), { firstPartySources: FIRST_PARTY });
    expect(n.packages.map((p) => `${p.name} ${p.version}`)).toEqual(["mio 1.0.2", "serde 1.0.210", "tokio 1.40.0"]);
    expect(n.packages[1].licence).toBe("MIT OR Apache-2.0");
    expect(n.packages[1].url).toBe("https://crates.io/crates/serde/1.0.210");
    expect(n.packages[2].url).toBe("https://github.com/tokio-rs/tokio");
    // Only texts a kept package uses: one MIT body (tokio, mio) and Apache (serde); BUSL is first party.
    expect(Object.values(n.texts).map((t) => t.id).sort()).toEqual(["Apache-2.0", "MIT"]);
    const mit = Object.values(n.texts).find((t) => t.id === "MIT");
    expect(mit.notices).toEqual({ "Copyright (c) Tokio Contributors": ["tokio 1.40.0"], "Copyright (c) Carl Lerche": ["mio 1.0.2"] });
    expect(n.packages[0].texts).toEqual(n.packages[2].texts);
  });

  it("dedupes texts that differ only in whitespace", () => {
    expect(textKey(APACHE)).toBe(textKey(APACHE.replace(/\n/g, "\n\n")));
    expect(textKey(MIT("a"))).not.toBe(textKey(MIT("b")));
  });

  it("refuses a third-party crate under a licence outside the permissive list", () => {
    const j = aboutJson();
    j.licenses.find((l) => l.id === "BUSL-1.1").used_by.push({ crate: crate("evil", "1.0.0") });
    j.crates.push({ package: crate("evil", "1.0.0"), license: "BUSL-1.1" });
    expect(() => normaliseCargoAbout(j, { firstPartySources: FIRST_PARTY })).toThrow(/evil 1\.0\.0: ships under BUSL-1\.1/);
  });

  it("refuses a third-party crate with no resolved licence text", () => {
    const j = aboutJson();
    j.crates.push({ package: crate("mystery", "0.1.0"), license: "LicenseRef-mystery" });
    expect(() => normaliseCargoAbout(j, { firstPartySources: FIRST_PARTY })).toThrow(/mystery 0\.1\.0: no licence text resolved/);
  });

  it("treats a Citrate git dependency as third party when its source is not configured as first party", () => {
    expect(() => normaliseCargoAbout(aboutJson(), { firstPartySources: [] })).toThrow(/citrate-types 0\.4\.0: ships under BUSL-1\.1/);
  });

  it("refuses output that is not cargo-about JSON", () => {
    expect(() => normaliseCargoAbout({})).toThrow(/no crates/);
  });
});

describe("splitCopyright", () => {
  it("splits the leading copyright block and keeps the title in the body", () => {
    expect(splitCopyright("MIT License\n\nCopyright (c) 2014 Carl\nCopyright (c) 2015 Bob\n\nPermission is hereby granted")).toEqual({
      copyright: "Copyright (c) 2014 Carl\nCopyright (c) 2015 Bob",
      body: "MIT License\n\n\nPermission is hereby granted",
    });
    expect(splitCopyright("Copyright (c) 2009 The Go Authors.\nAll rights reserved.\n\nRedistribution and use").copyright).toBe(
      "Copyright (c) 2009 The Go Authors.\nAll rights reserved.",
    );
  });

  it("keeps a text that is only copyright lines whole", () => {
    expect(splitCopyright("Copyright (c) 2020 Someone")).toEqual({ copyright: "", body: "Copyright (c) 2020 Someone" });
  });

  it("leaves a copyright line inside the body where it is", () => {
    const t = "Apache License\nVersion 2.0\n\n   Copyright [yyyy] [name of copyright owner]\n";
    expect(splitCopyright(t)).toEqual({ copyright: "", body: t });
  });

  it("loses no line: copyright plus body hold every non-empty line of the text", () => {
    const t = "The MIT License (MIT)\r\n\r\nCopyright (c) 2015 A\r\n(c) 2016 B\r\n\r\nPermission is hereby granted\r\nTHE SOFTWARE";
    const { copyright, body } = splitCopyright(t);
    const lines = (x) => x.split("\n").map((l) => l.trim()).filter(Boolean).sort();
    expect([...lines(copyright), ...lines(body)].sort()).toEqual(lines(t.replace(/\r/g, "")));
  });
});

describe("isFirstParty", () => {
  it("path crates and configured prefixes are first party", () => {
    expect(isFirstParty(null, [])).toBe(true);
    expect(isFirstParty("git+https://github.com/CitrateNetwork/citrate-chain.git#x", FIRST_PARTY)).toBe(true);
    expect(isFirstParty(REG, FIRST_PARTY)).toBe(false);
  });
});

describe("normaliseGoLicenses", () => {
  const CACHE = "/home/u/go/pkg/mod";
  const files = {
    [`${CACHE}/github.com/!burnt!sushi/toml@v1.4.0/COPYING`]: MIT("TOML authors"),
    [`${CACHE}/golang.org/x/sys@v0.30.0/LICENSE`]: "Copyright 2009 The Go Authors.\n\nRedistribution and use in source and binary forms",
    [`${CACHE}/github.com/jackpal/go-nat-pmp@v1.0.2/LICENSE`]: "Copyright 2013 John Howard Palevich\n\nLicensed under the Apache License",
    "/goroot/LICENSE": "Copyright 2009 The Go Authors.\n\nRedistribution and use in source and binary forms",
  };
  const tsv = [
    "github.com/ipfs/kubo\tUnknown\tMIT\t/src/kubo/LICENSE-MIT\thttps://github.com/ipfs/kubo",
    "github.com/ipfs/kubo/core\tUnknown\tMIT\t/src/kubo/LICENSE-MIT\thttps://github.com/ipfs/kubo",
    `github.com/BurntSushi/toml\tv1.4.0\tMIT\t${CACHE}/github.com/!burnt!sushi/toml@v1.4.0/COPYING\thttps://github.com/BurntSushi/toml/blob/v1.4.0/COPYING`,
    `golang.org/x/sys/unix\tv0.30.0\tBSD-3-Clause\t${CACHE}/golang.org/x/sys@v0.30.0/LICENSE\thttps://cs.opensource.google/go/x/sys`,
    `golang.org/x/sys/cpu\tv0.30.0\tBSD-3-Clause\t${CACHE}/golang.org/x/sys@v0.30.0/LICENSE\thttps://cs.opensource.google/go/x/sys`,
    "github.com/jackpal/go-nat-pmp\tv1.0.2\tUnknown\tUnknown\tUnknown",
    "github.com/whyrusleeping/base32\tv0.0.0-2017\tUnknown\tUnknown\tUnknown",
    "",
  ].join("\n");
  const opts = (clarify) => ({
    mainModule: "github.com/ipfs/kubo",
    clarify,
    goroot: "/goroot",
    readText: (f) => {
      if (!(f in files)) throw new Error(`no fixture file ${f}`);
      return files[f];
    },
    moduleDir: (mod, version) => `${CACHE}/${mod}@${version}`,
  });
  const CLARIFY = {
    "github.com/jackpal/go-nat-pmp": { spdx: "Apache-2.0", file: "LICENSE" },
    "github.com/whyrusleeping/base32": { spdx: "BSD-3-Clause", file: "GOROOT/LICENSE" },
  };

  it("collapses packages to modules, skips the main module and applies clarifications", () => {
    const n = normaliseGoLicenses(tsv, opts(CLARIFY));
    expect(n.packages.map((p) => `${p.name} ${p.licence}`)).toEqual([
      "github.com/BurntSushi/toml MIT",
      "github.com/jackpal/go-nat-pmp Apache-2.0",
      "github.com/whyrusleeping/base32 BSD-3-Clause",
      "golang.org/x/sys BSD-3-Clause",
    ]);
    // x/sys and base32 (via the Go authors' licence) share one text.
    expect(Object.keys(n.texts)).toHaveLength(3);
    const bsd = Object.values(n.texts).find((t) => t.id === "BSD-3-Clause");
    expect(bsd.notices["Copyright 2009 The Go Authors."].sort()).toEqual(["github.com/whyrusleeping/base32 v0.0.0-2017", "golang.org/x/sys v0.30.0"]);
  });

  it("refuses a module the scanner cannot classify and nobody clarified", () => {
    expect(() => normaliseGoLicenses(tsv, opts({ "github.com/jackpal/go-nat-pmp": CLARIFY["github.com/jackpal/go-nat-pmp"] }))).toThrow(
      /github\.com\/whyrusleeping\/base32 v0\.0\.0-2017: licence unknown/,
    );
  });

  it("refuses a module under a licence outside the permissive list", () => {
    const gpl = `github.com/x/gpl\tv1.0.0\tGPL-3.0\t${CACHE}/github.com/x/gpl@v1.0.0/LICENSE\thttps://x`;
    expect(() => normaliseGoLicenses(gpl, opts({}))).toThrow(/ships under GPL-3\.0/);
  });
});

describe("renderNotices", () => {
  const comps = () => [
    {
      component: "zeta",
      source: "zeta@1",
      tool: "cargo-about 0.9.2",
      packages: [{ name: "b", version: "1.0.0", licence: "MIT", texts: ["k2"], url: null }],
      texts: { k2: { id: "MIT", body: "MIT body\n", notices: { "Copyright (c) B": ["b 1.0.0"] } } },
    },
    {
      component: "alpha",
      source: "alpha@2",
      tool: "cargo-about 0.9.2",
      packages: [
        { name: "a", version: "0.1.0", licence: "Apache-2.0 OR MIT", texts: ["k1"], url: "https://crates.io/crates/a/0.1.0" },
        { name: "c", version: "2.0.0", licence: "MIT", texts: ["k2"], url: null },
      ],
      texts: {
        k1: { id: "Apache-2.0", body: "Apache text  \r\n", notices: {} },
        k2: { id: "MIT", body: "MIT body\n", notices: { "Copyright (c) C": ["c 2.0.0"] } },
      },
    },
  ];

  it("lists components in order, numbers each distinct text once and is deterministic", () => {
    const text = renderNotices(comps());
    expect(text).toBe(renderNotices(comps().reverse()));
    expect(noticeComponents(text)).toEqual(["alpha", "zeta"]);
    expect(text).toContain("a 0.1.0  Apache-2.0 OR MIT  [T1]  https://crates.io/crates/a/0.1.0");
    expect(text).toContain("c 2.0.0  MIT  [T2]");
    expect(text).toContain("b 1.0.0  MIT  [T2]");
    expect(text.match(/---- T\d+:/g)).toEqual(["---- T1:", "---- T2:"]);
    expect(text).toContain("---- T1: Apache-2.0 ----\n\nApache text\n");
    // One MIT body, under the copyright lines of both components' packages.
    expect(text).toContain("---- T2: MIT ----\n\nCopyright (c) B\n  applies to: b 1.0.0\nCopyright (c) C\n  applies to: c 2.0.0\n\nMIT body\n");
    expect(text).not.toMatch(/\r/);
  });
});

describe("normaliseCollected", () => {
  it("normalises a collected Go report from the files it kept, with the Go standard library", () => {
    const raw = {
      component: "kubo",
      kind: "go",
      source: "github.com/ipfs/kubo@v0.42.0 ./cmd/ipfs",
      tool: "go-licenses",
      tsv: "github.com/x/y\tv1.0.0\tMIT\t/cache/github.com/x/y@v1.0.0/LICENSE\thttps://x\n",
      files: { "/cache/github.com/x/y@v1.0.0/LICENSE": MIT("Y"), "/goroot/LICENSE": "Copyright 2009 The Go Authors.\n\nRedistribution and use" },
      goroot: "/goroot",
      goVersion: "go1.26.4",
      modCache: "/cache",
    };
    const n = normaliseCollected(raw, { go: [{ component: "kubo", module: "github.com/ipfs/kubo", clarify: {} }] });
    expect(n.packages.map((p) => p.name)).toEqual(["github.com/x/y", "go (standard library)"]);
    expect(() => normaliseCollected({ ...raw, files: {} }, { go: [{ component: "kubo", module: "github.com/ipfs/kubo" }] })).toThrow(/was not collected/);
    expect(() => normaliseCollected({ ...raw, kind: "zig" }, {})).toThrow(/unknown collected kind/);
  });
});

describe("updateInventory", () => {
  it("records the notices file on each component and on the app, and refuses an unknown component", () => {
    const inv = { app: { name: "app" }, components: [{ id: "alpha" }, { id: "other" }] };
    const out = updateInventory(
      inv,
      [
        { component: "alpha", source: "alpha@2", packages: [1, 2] },
        { component: "app", source: "core@3", packages: [1] },
      ],
      "src-tauri/licenses/THIRD-PARTY-NOTICES.txt",
    );
    expect(out.components[0].third_party_notices).toEqual({ file: "src-tauri/licenses/THIRD-PARTY-NOTICES.txt", source: "alpha@2", packages: 2 });
    expect(out.app.third_party_notices.source).toBe("core@3");
    expect(out.components[1].third_party_notices).toBeUndefined();
    expect(() => updateInventory(inv, [{ component: "ghost", packages: [] }], "x")).toThrow(/ghost has no entry/);
  });
});

describe("formatInventory", () => {
  it("writes release/licences.json byte for byte in its hand-written style", () => {
    const text = fs.readFileSync(path.join(repoRoot, "release", "licences.json"), "utf8");
    expect(formatInventory(JSON.parse(text)) + "\n").toBe(text);
  });

  it("inlines short scalar arrays and expands long ones and objects", () => {
    const out = formatInventory({ a: ["x", "y"], b: ["z".repeat(140)], c: { d: null }, e: [] });
    expect(out).toBe(`{\n  "a": ["x", "y"],\n  "b": [\n    "${"z".repeat(140)}"\n  ],\n  "c": {\n    "d": null\n  },\n  "e": []\n}`);
  });
});

describe("the committed config and notices", () => {
  it("configures every first-party sidecar and the app, and Kubo", () => {
    const cfg = readConfig(repoRoot);
    const ids = [...cfg.rust, ...cfg.go].map((e) => e.component).sort();
    expect(ids).toEqual(["app", "citrate-node", "cluster-daemon", "comms-member-daemon", "hermes-sidecar", "kubo", "mem-mcp", "node-agent"]);
    expect(cfg.output).toBe("src-tauri/licenses/THIRD-PARTY-NOTICES.txt");
  });

  it("the Kubo version matches the kubo entry of the licence inventory", () => {
    const cfg = readConfig(repoRoot);
    const inv = JSON.parse(fs.readFileSync(path.join(repoRoot, "release", "licences.json"), "utf8"));
    const kubo = inv.components.find((c) => c.id === "kubo");
    expect(kubo.name).toContain(cfg.go[0].version.replace(/^v/, ""));
  });

  it("keeps MPL-2.0 the only copyleft licence on the permissive list", () => {
    expect([...PERMISSIVE].filter((l) => /GPL|MPL|EPL|CDDL|SSPL|BUSL/.test(l))).toEqual(["MPL-2.0"]);
  });

  it("CLI check passes on the committed notices", () => {
    const r = spawnSync(process.execPath, [SCRIPT, "check"], { cwd: repoRoot, encoding: "utf8" });
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
  });

  it("CLI exits 2 on a usage error", () => {
    expect(spawnSync(process.execPath, [SCRIPT], { encoding: "utf8" }).status).toBe(2);
    expect(spawnSync(process.execPath, [SCRIPT, "render", "--bogus"], { encoding: "utf8" }).status).toBe(2);
  });
});
