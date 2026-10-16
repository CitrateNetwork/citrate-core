import { describe, expect, it } from "vitest";
import { DEVICE_LINKS_MSG_PREFIX, SOCIAL_BINDING_MSG_PREFIX } from "../bridge/domains";
import { isControlMessage, visibleConversation } from "./groupControlMessages";

describe("group control messages", () => {
  it("hides social bindings and device link shares, and only those", () => {
    expect(isControlMessage(`${SOCIAL_BINDING_MSG_PREFIX}{"network":"x"}`)).toBe(true);
    expect(isControlMessage(`${DEVICE_LINKS_MSG_PREFIX}{"v":1}`)).toBe(true);
    // Typed text never matches: both sentinels start with U+0001.
    expect(isControlMessage('cdlink1:{"v":1}')).toBe(false);
    expect(isControlMessage("cbind1:{}")).toBe(false);
    const msgs = [
      { id: "1", body: "hi" },
      { id: "2", body: `${DEVICE_LINKS_MSG_PREFIX}{"v":1,"links":[]}` },
      { id: "3", body: `${SOCIAL_BINDING_MSG_PREFIX}{}` },
      { id: "4", body: "cdlink1: typed by a person" },
    ];
    expect(visibleConversation(msgs).map((m) => m.id)).toEqual(["1", "4"]);
  });
});
