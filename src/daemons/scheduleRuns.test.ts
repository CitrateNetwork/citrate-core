// 0.5.4 — a finished run of an entry on Hermes's schedule reaches the member in the chat.
import { describe, it, expect, beforeEach } from "vitest";
import { store } from "../shell/store";

beforeEach(() => {
  store.setState({ chatMsgs: [], toast: null });
});

describe("Feature: a scheduled run reports back", () => {
  it("Given an answered run, then its reply is posted in the chat under the entry's name", () => {
    store.postScheduledRun({ name: "Call the notary", outcome: "answered" }, "Reminder: call the notary before noon.");
    const last = store.state.chatMsgs[store.state.chatMsgs.length - 1];
    expect(last.who).toBe("Agent");
    expect(last.text).toBe("Scheduled · Call the notary\n\nReminder: call the notary before noon.");
    expect(store.state.toast).toMatch(/Hermes ran “Call the notary” from your schedule/);
  });

  it("Given a run that did not answer, then nothing is posted and the toast says how it ended", () => {
    store.postScheduledRun({ name: "Call the notary", outcome: "over_budget" }, null);
    expect(store.state.chatMsgs).toHaveLength(0);
    expect(store.state.toast).toBe("“Call the notary” from your schedule ended: over budget");
  });
});
