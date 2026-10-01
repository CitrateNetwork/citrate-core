---
created: 2026-10-01T12:30:00Z
branch: hup/n4-widgets-daemons
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S10
---

# A sandboxed iframe inherits the page's CSP, so a widget needs its own document

**Dek:** The first design for widgets (an `srcdoc` iframe with `sandbox="allow-scripts"`) would
never have run a line of widget script in the packaged app, and no unit test would have noticed.

## Context

Daytime fan-out 4, lane HUP-S10.3: widgets (sandboxed tiles with a narrow bridge) and daemons
(scheduled Hermes runs inside a budget). The planset (§10, D-35) says "sandboxed `iframe` (no
network, `sandbox="allow-scripts"`), data via a typed `postMessage` bridge".

## What happened

An `srcdoc` (or `blob:` or `data:`) document is a local-scheme document: it inherits the parent's
policy container, including its CSP. The app's CSP is `script-src 'self'`, so every inline
script in a widget would be blocked in the packaged app, while the Vite dev server (no CSP
injection) and jsdom (no CSP at all) would both run it happily. Loosening the app's own
`script-src` to make widgets work would weaken the main window.

The fix is to give widgets their own origin: a custom URI scheme (`citrate-widget://`) whose
handler serves each widget with its own CSP header (inline script allowed, `connect-src 'none'`,
everything else `'none'`, `sandbox allow-scripts`, `frame-ancestors` the app only). The app CSP
gains exactly one directive, `frame-src` for that scheme. As a bonus the handler can refuse any
webview label except `main`, which a `srcdoc` could never do.

The daemon half had a smaller version of the same lesson: "max spend = 0" is only true if the
daemon can never reach a paid gateway, so the runner refuses to claim anything unless the local
model is the provider, and a non-zero spend budget is refused at save time rather than ignored.

## What I would do again

- Ask "which policy does this document actually run under?" before writing a sandbox. Tests that
  run without the real CSP prove the bridge, not the sandbox.
- Put the boundary where it can be tested as data: the CSP header and the handler's refusals are
  pure functions with tests; the iframe's flags are one constant with a test.
- Mutation-check the guards, both the TLA+ invariants and the Rust and TypeScript checks: every
  budget and HIC guard here has a test that fails when it is removed.

## Not observed

The packaged app was not run: whether WKWebView, WebKitGTK and WebView2 all honour the custom
scheme's CSP header and `frame-ancestors` is asserted by the response, not observed on screen.
That click-through is the first thing to do on each OS.
