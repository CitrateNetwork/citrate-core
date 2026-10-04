// HUP-S8.1 follow-on: sharing DeviceLinks with the other members of a group over the relay. The
// helpers decide what to send and what to hand to core; core verifies every signature.
import { describe, expect, it, vi } from "vitest";
import { DEVICE_LINKS_MSG_PREFIX } from "../bridge/domains";
import { ingestDeviceLinkMessages, isDeviceLinkMessage, latestPerSender, shareDeviceLinks } from "./deviceLinkShare";

const msg = (sender: string, body: string) => ({ sender, body });

describe("device link sharing", () => {
  it("recognises its control messages and nothing else", () => {
    expect(isDeviceLinkMessage(`${DEVICE_LINKS_MSG_PREFIX}{}`)).toBe(true);
    expect(isDeviceLinkMessage("hello cdlink1:")).toBe(false);
    expect(isDeviceLinkMessage('cdlink1:{"v":1}')).toBe(false); // typed, without the U+0001 sentinel
    expect(isDeviceLinkMessage("cbind1:{}")).toBe(false);
  });

  it("sends each group the offer once, then marks it", async () => {
    const offer = vi.fn(async (g: string) => (g === "g2" ? null : { body: `${DEVICE_LINKS_MSG_PREFIX}{"v":1}`, digest: "d".repeat(64) }));
    const mark = vi.fn(async () => undefined);
    const send = vi.fn(async () => undefined);
    const sent = await shareDeviceLinks(["g1", "g2", "g3"], { offer, mark, send });
    expect(sent).toBe(2);
    expect(send.mock.calls.map((c) => c[0])).toEqual(["g1", "g3"]);
    expect(mark.mock.calls).toEqual([
      ["g1", "d".repeat(64)],
      ["g3", "d".repeat(64)],
    ]);
  });

  it("does not mark a group whose send failed, and keeps going", async () => {
    const offer = vi.fn(async () => ({ body: `${DEVICE_LINKS_MSG_PREFIX}{}`, digest: "e".repeat(64) }));
    const mark = vi.fn(async () => undefined);
    const send = vi.fn(async (g: string) => {
      if (g === "g1") throw new Error("relay down");
    });
    const sent = await shareDeviceLinks(["g1", "g2"], { offer, mark, send });
    expect(sent).toBe(1);
    expect(mark.mock.calls).toEqual([["g2", "e".repeat(64)]]);
  });

  it("hands core only the newest share per sender (each one carries the full set)", async () => {
    const msgs = [
      msg("aa", `${DEVICE_LINKS_MSG_PREFIX}old`),
      msg("bb", "just chatting"),
      msg("aa", `${DEVICE_LINKS_MSG_PREFIX}new`),
      msg("cc", `${DEVICE_LINKS_MSG_PREFIX}only`),
    ];
    expect(latestPerSender(msgs).map((m) => m.body)).toEqual([`${DEVICE_LINKS_MSG_PREFIX}new`, `${DEVICE_LINKS_MSG_PREFIX}only`]);
    const ingest = vi.fn(async () => ({ links: 1, revocations: 0, refused: [] }));
    const total = await ingestDeviceLinkMessages(msgs, ingest);
    expect(ingest).toHaveBeenCalledTimes(2);
    expect(total).toEqual({ links: 2, revocations: 0 });
  });

  it("a refused or failing share never throws to the surface", async () => {
    const ingest = vi.fn(async () => {
      throw new Error("a malformed device link message");
    });
    await expect(ingestDeviceLinkMessages([msg("aa", `${DEVICE_LINKS_MSG_PREFIX}x`)], ingest)).resolves.toEqual({ links: 0, revocations: 0 });
  });
});
