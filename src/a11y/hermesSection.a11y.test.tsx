// HGW (v0.5.1): accessibility of the Hermes section's new scroll affordances. The transcript is
// now its own scroll region, so it must be reachable and named for keyboard and screen-reader
// members; the narrow-window column switch is a labelled tablist.
import { describe, it, expect, afterEach } from "vitest";
import { act } from "react";
import { AgentChat } from "../components/AgentChat";
import { SectionSwitch } from "../components/sectionLayout";
import { store } from "../shell/store";
import { freshState, type ChatMsg } from "../shell/state";
import { axeFindings, cleanupMounted, mount } from "./axeHarness";

afterEach(() => cleanupMounted());

function msgs(n: number): ChatMsg[] {
  return Array.from({ length: n }, (_, i) => ({ id: "m" + i, who: i % 2 ? "Agent" : "You", text: "message " + i, chips: [], streaming: false }) as ChatMsg);
}

describe("Hermes section a11y", () => {
  it("the transcript scroll region is focusable, named, and clean under axe", async () => {
    store.setState({ ...freshState("p1"), chatMsgs: msgs(6), chatStatus: "ready" });
    const m = mount(<AgentChat store={store} s={store.state} />);
    const list = m.host.querySelector('[data-testid="chat-messages"]') as HTMLElement;
    expect(list.tabIndex).toBe(0);
    expect(list.getAttribute("aria-label")).toBe("Conversation");
    expect(await axeFindings(list)).toEqual([]);
  });

  it("the narrow-window column switch is a labelled tablist with one selected tab", async () => {
    let value: "chat" | "activity" = "chat";
    const m = mount(
      <SectionSwitch
        label="Hermes section"
        options={[
          { id: "chat", label: "Chat" },
          { id: "activity", label: "Activity", badge: 2 },
        ]}
        value={value}
        onChange={(v) => {
          value = v;
        }}
      />,
    );
    const tl = m.host.querySelector('[role="tablist"]') as HTMLElement;
    expect(tl.getAttribute("aria-label")).toBe("Hermes section");
    const tabs = Array.from(tl.querySelectorAll('[role="tab"]'));
    expect(tabs.filter((t) => t.getAttribute("aria-selected") === "true")).toHaveLength(1);
    act(() => (tabs[1] as HTMLButtonElement).click());
    expect(value).toBe("activity");
    expect(await axeFindings(m.host)).toEqual([]);
  });
});
