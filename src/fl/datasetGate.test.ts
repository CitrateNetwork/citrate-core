// WP-H7: what Hermes is told about a scan and a license report: counts and verdicts, never the
// matched text, and the web preview refuses (no dataset store there).
import { describe, it, expect } from "vitest";
import { datasetGate, GATE_NEEDS_DESKTOP, licensesForAgent, pluralLabel, scanForAgent, type FlLicenseReport, type FlScanReport } from "./datasetGate";

const report = (over: Partial<FlScanReport> = {}): FlScanReport => ({
  scanner: "citrate-pii-scan/1",
  passed: true,
  rowsScanned: 12,
  counts: {},
  findings: [],
  truncated: false,
  coverage: "Checked for email addresses and more.",
  ...over,
});

describe("scanForAgent", () => {
  it("a clean scan says nothing was found and names the scanner and coverage", () => {
    const t = scanForAgent("ds-0123456789abcdef", 2, report());
    expect(t).toContain("found nothing in 12 rows (citrate-pii-scan/1, on this device)");
    expect(t).toContain("Checked for email addresses and more.");
  });
  it("findings are counted by class with row numbers, and publishing is said to be refused", () => {
    const t = scanForAgent(
      "ds-0123456789abcdef",
      2,
      report({ passed: false, counts: { email: 2, phone: 1 }, findings: [{ row: 0, class: "email", start: 4, end: 20 }, { row: 3, class: "email", start: 0, end: 9 }, { row: 3, class: "phone", start: 1, end: 13 }] }),
    );
    expect(t).toContain("found 2 email addresses");
    expect(t).toContain("1 phone number");
    expect(t).toContain("Rows: 1, 4.");
    expect(t).toContain("can never be Public");
  });
});

describe("pluralLabel", () => {
  it("pluralises every class label", () => {
    expect(pluralLabel("email address")).toBe("email addresses");
    expect(pluralLabel("phone number")).toBe("phone numbers");
    expect(pluralLabel("person's name")).toBe("names");
    expect(pluralLabel("key, token or seed phrase")).toBe("keys, tokens or seed phrases");
  });
});

describe("licensesForAgent", () => {
  it("names each source's license and verdict, and a ShareAlike constraint", () => {
    const r: FlLicenseReport = {
      dataset: "ds-0123456789abcdef",
      version: 3,
      ok: false,
      shareAlike: ["CC-BY-SA-4.0"],
      sources: [
        { source: "s-1", kind: "folder", locator: "notes/a.txt", license: null, basis: "unknown", evidence: null, verdict: "member-owned", reason: null },
        { source: "s-2", kind: "web", locator: "https://en.wikipedia.org/wiki/Apple", license: "CC-BY-SA-4.0", basis: "found", evidence: "x", verdict: "ok", reason: null },
        { source: "s-3", kind: "web", locator: "https://example.org/a", license: null, basis: "unknown", evidence: null, verdict: "unknown", reason: "no license is known for this source" },
      ],
    };
    const t = licensesForAgent(r);
    expect(t).toContain("some sources block publishing");
    expect(t).toContain("must be published under that license");
    expect(t).toContain("- s-1 notes/a.txt: no license; the member's own content");
    expect(t).toContain("- s-2 https://en.wikipedia.org/wiki/Apple: CC-BY-SA-4.0 (found); publishable");
    expect(t).toContain("- s-3 https://example.org/a: no license; no license is known for this source");
  });
});

describe("datasetGate", () => {
  it("refuses in the web preview", async () => {
    await expect(datasetGate.piiScan("ds-0123456789abcdef", 1)).rejects.toThrow(GATE_NEEDS_DESKTOP);
  });
});
