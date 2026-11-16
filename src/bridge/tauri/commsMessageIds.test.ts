// #688 review: drained group messages need ids that never repeat. Each `groups_messages` call DRAINS
// the daemon mailbox, so an index alone repeats on every drain, and two drains can land in the same
// millisecond (a poll plus the drain after a send). The retained-history dedupe keys on the id, so a
// repeated id silently drops a real message.
import { describe, it, expect, vi, afterEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("./invoke", () => ({ invoke: (...a: unknown[]) => invokeMock(...a), deadlineFor: () => 12_000 }));

import { tauriGroups, messageId } from "./comms";

afterEach(() => {
  vi.useRealTimers();
  invokeMock.mockReset();
});

describe("group message ids", () => {
  it("two drains in the same millisecond give different ids", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_000);
    invokeMock.mockResolvedValue([["0xaa", "hello"]]);
    const a = await tauriGroups.messages("g");
    const b = await tauriGroups.messages("g");
    expect(a[0].id).not.toBe(b[0].id);
    expect(a[0].ts).toBe(b[0].ts);
  });
  it("messages within one drain differ by index", async () => {
    invokeMock.mockResolvedValue([["0xaa", "one"], ["0xbb", "two"]]);
    const r = await tauriGroups.messages("g");
    expect(new Set(r.map((m) => m.id)).size).toBe(2);
  });
  it("the id names the group, the receipt time, the drain and the index", () => {
    expect(messageId("g", 5, 2, 1)).toBe("g:rx5.2:1");
    expect(messageId("g", 5, 2, 1)).not.toBe(messageId("g", 5, 3, 1));
    expect(messageId("g", 5, 2, 1)).not.toBe(messageId("g", 6, 2, 1));
  });
});
