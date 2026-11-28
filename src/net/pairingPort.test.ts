// L7-WP03: the pairing port line. Written red-first: the helper was absent.
import { describe, expect, it } from "vitest";
import { pairingPortNote } from "./pairingPort";

describe("L7-WP03 pairingPortNote", () => {
  it("names the fixed port and says it is private-network only", () => {
    const t = pairingPortNote({ port: 4210, portFallback: false });
    expect(t).toContain("port 4210 (TCP)");
    expect(t).toContain("private networks only");
    expect(t).toContain("firewall for private networks");
  });

  it("says plainly when the port was taken and another one is used", () => {
    const t = pairingPortNote({ port: 4210, portFallback: true });
    expect(t).toContain("Port 4210 (TCP) is taken on this machine");
    expect(t).toContain("uses another port");
  });

  it("says nothing when core did not report a port", () => {
    expect(pairingPortNote({})).toBeNull();
  });
});
