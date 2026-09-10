// Regression (Rule 1 — the app must never white-screen on real input): the S3
// Enterprise "custom pricing" contact form used to crash the whole app while typing.
// Every field's onChange read `e.currentTarget.value` INSIDE the functional setState
// updater — but React nulls the synthetic event's `currentTarget` after the handler
// returns, and the updater runs later, so the read threw
// "Cannot read properties of null (reading 'value')" and tripped the ErrorBoundary.
// The fix captures the value synchronously before setF. This test types into every
// field and submits; if the anti-pattern regresses, the render throws and this fails.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const enterpriseLeadMock = vi.fn<() => Promise<void>>().mockResolvedValue(undefined);
vi.mock("../bridge", () => ({
  bridge: { membership: { enterpriseLead: enterpriseLeadMock } },
  BRIDGE_MODE: "sim",
}));

import { S3 } from "./Onboarding";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

// React-controlled input: set value via the native setter then dispatch 'input'.
function typeInto(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")!.set!;
  setter.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("S3 enterprise custom-pricing form", () => {
  it("accepts typing in every field and submits without crashing", async () => {
    const submitEnterpriseLead = vi.fn().mockResolvedValue({ ok: true });
    const store = {
      onS3Free: vi.fn(),
      submitEnterpriseLead,
      setState: vi.fn(),
      save: vi.fn(),
      walletIsLinked: vi.fn().mockReturnValue(true),
      linkWallet: vi.fn(),
      onS3Pay: vi.fn(),
      recheckMembership: vi.fn(),
    } as unknown as Store;

    const s = freshState("p1");
    s.s3 = "idle";
    s.stage = "s3";

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => { root.render(<S3 store={store} s={s} />); });

    // Open the enterprise contact form.
    const contactBtn = Array.from(host.querySelectorAll("button")).find((b) => /contact sales/i.test(b.textContent ?? ""));
    expect(contactBtn).toBeTruthy();
    await act(async () => { contactBtn!.click(); });

    // Type into every text field (the crash used to fire here).
    const org = host.querySelector<HTMLInputElement>('input[placeholder="Acme Corp"]')!;
    const email = host.querySelector<HTMLInputElement>('input[placeholder="you@acme.com"]')!;
    const contact = host.querySelector<HTMLInputElement>('input[placeholder="Dana Okafor"]')!;
    const notes = host.querySelector<HTMLTextAreaElement>("textarea")!;
    expect(org && email && contact && notes).toBeTruthy();
    await act(async () => { typeInto(org, "Acme Corp"); });
    await act(async () => { typeInto(email, "dana@acme.com"); });
    await act(async () => { typeInto(contact, "Dana Okafor"); });
    await act(async () => { typeInto(notes, "on-prem, HIPAA"); });

    // The controlled inputs reflect what was typed (proves the value landed, not lost).
    expect(org.value).toBe("Acme Corp");
    expect(email.value).toBe("dana@acme.com");

    // Submit reaches the store (valid org + email enables the button).
    const sendBtn = Array.from(host.querySelectorAll("button")).find((b) => /send request/i.test(b.textContent ?? ""));
    expect(sendBtn).toBeTruthy();
    expect((sendBtn as HTMLButtonElement).disabled).toBe(false);
    await act(async () => { sendBtn!.click(); });
    expect(submitEnterpriseLead).toHaveBeenCalledOnce();

    root.unmount();
    host.remove();
  });
});
