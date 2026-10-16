// SCL-S7.5a (D-16 extended, US-6.2 AC5) — the per-message question shown when the local model is
// configured but its server is not running: restart the local model, or send this message to the
// gateway this time. Nothing is preselected; each button answers only for the held message.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { LocalModelAsk, LOCAL_ASK_COPY } from "./LocalModelAsk";
import { AgentChat } from "./AgentChat";
import { store } from "../shell/store";
import { freshState } from "../shell/state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root?.render(el));
  return host;
}
const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

describe("SCL-S7.5a local model question", () => {
  it("asks with the two clear choices and says nothing has been sent", () => {
    const el = render(<LocalModelAsk phase="ask" onChoose={vi.fn()} />);
    const text = q(el, "local-model-ask")?.textContent ?? "";
    expect(text).toContain("Your local model is not running");
    expect(text).toContain("has not been sent");
    expect(q(el, "local-model-ask-restart")?.textContent).toBe("Restart the local model");
    expect(q(el, "local-model-ask-gateway")?.textContent).toBe("Send this message to the gateway this time");
    // House style: no em-dashes in any of the wording.
    for (const s of Object.values(LOCAL_ASK_COPY)) expect(s).not.toContain("—");
  });

  it("each button reports exactly its own choice", () => {
    const onChoose = vi.fn();
    const el = render(<LocalModelAsk phase="ask" onChoose={onChoose} />);
    act(() => (q(el, "local-model-ask-restart") as HTMLButtonElement).click());
    expect(onChoose).toHaveBeenLastCalledWith("restart");
    act(() => (q(el, "local-model-ask-gateway") as HTMLButtonElement).click());
    expect(onChoose).toHaveBeenLastCalledWith("gateway");
    act(() => (q(el, "local-model-ask-cancel") as HTMLButtonElement).click());
    expect(onChoose).toHaveBeenLastCalledWith("cancel");
    expect(onChoose).toHaveBeenCalledTimes(3);
  });

  it("while the app's own start is pending it waits and offers no gateway button", () => {
    const el = render(<LocalModelAsk phase="waiting" onChoose={vi.fn()} />);
    expect(q(el, "local-model-ask")?.textContent).toContain("Waiting for your local model to start");
    expect(q(el, "local-model-ask-gateway")).toBeNull();
    expect(q(el, "local-model-ask-restart")).toBeNull();
  });

  it("while restarting it says the answer will come from this device", () => {
    const el = render(<LocalModelAsk phase="restarting" onChoose={vi.fn()} />);
    expect(q(el, "local-model-ask")?.textContent).toContain("Restarting your local model");
    expect(q(el, "local-model-ask-gateway")).toBeNull();
  });

  it("the chat shows the question under the held message, not a thinking spinner", () => {
    store.setState({
      ...freshState("p1"),
      chatMsgs: [{ id: "m9", who: "You", text: "hello", chips: [], streaming: false }],
      chatStatus: "thinking",
      chatRouteHold: { msgId: "m9", phase: "ask" },
    });
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html).toContain('data-testid="local-model-ask"');
    expect(html).toContain("Send this message to the gateway this time");
    expect(html).not.toContain(">reasoning<");
    store.setState({ chatRouteHold: null, chatStatus: "ready" });
  });
});
