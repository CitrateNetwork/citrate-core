/* @ds-bundle: {"format":3,"namespace":"CitrateDesignSystem_244c4c","components":[{"name":"Button","sourcePath":"components/Button.jsx"}],"sourceHashes":{"components/Button.jsx":"6bf6cd748f21","pitch_deck/deck-stage.js":"d8d952171670","ui_kits/console/App.jsx":"c4d53080337a","ui_kits/console/AuthScreen.jsx":"fff13fc77562","ui_kits/console/Dashboard.jsx":"eeb23ad0eaf1","ui_kits/console/FilingDetail.jsx":"78004c9a1322","ui_kits/console/LedgerTable.jsx":"493071d93ba1","ui_kits/console/Primitives.jsx":"163540f5d503","ui_kits/console/Sidebar.jsx":"e83a1fb1770f","ui_kits/console/TopBar.jsx":"b685a85ed467","ui_kits/marketing/ContractCard.jsx":"e1eccdfef52d","ui_kits/marketing/Footer.jsx":"4d7327b2a657","ui_kits/marketing/Hero.jsx":"f58f3c1d56bf","ui_kits/marketing/LogoCloud.jsx":"7f0c4561f537","ui_kits/marketing/MarketingApp.jsx":"e149ae1d5125","ui_kits/marketing/Nav.jsx":"6161827aeff2","ui_kits/marketing/Quote.jsx":"49773a7ab167","ui_kits/marketing/Section.jsx":"9ffacc8f02f8","ui_kits/marketing/Stat.jsx":"1e112cb2006d","ui_kits/mobile/MobileApp.jsx":"44144181f43e","ui_kits/mobile/ios-frame.jsx":"d67eb3ffe562"},"inlinedExternals":[],"unexposedExports":[]} */

(() => {

const __ds_ns = (window.CitrateDesignSystem_244c4c = window.CitrateDesignSystem_244c4c || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// components/Button.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
// Citrate — Button. The canonical action control.
// Exported as window.<Namespace>.Button via the compiled bundle.
function Button({
  variant = "primary",
  size = "md",
  children,
  onClick,
  type = "button",
  style = {},
  ...rest
}) {
  const [hover, setHover] = React.useState(false);
  const sizes = {
    sm: {
      height: 32,
      padding: "0 14px",
      fontSize: 13
    },
    md: {
      height: 40,
      padding: "0 18px",
      fontSize: 14
    },
    lg: {
      height: 48,
      padding: "0 24px",
      fontSize: 15
    }
  };
  const base = {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
    fontFamily: "var(--font-sans)",
    fontWeight: 500,
    letterSpacing: "0.005em",
    borderRadius: "var(--r-1)",
    border: "1px solid transparent",
    cursor: "pointer",
    whiteSpace: "nowrap",
    transition: "background 140ms var(--ease-standard), color 140ms var(--ease-standard), border-color 140ms var(--ease-standard)",
    ...(sizes[size] || sizes.md)
  };
  const variants = {
    primary: {
      background: "var(--citrate-green)",
      color: "var(--ink)",
      borderColor: "var(--citrate-green)"
    },
    secondary: {
      background: "var(--ink)",
      color: "var(--paper)",
      borderColor: "var(--ink)"
    },
    ghost: {
      background: "transparent",
      color: "var(--ink)",
      borderColor: "var(--stone-300)"
    },
    danger: {
      background: "transparent",
      color: "var(--semantic-danger)",
      borderColor: "var(--semantic-danger)"
    },
    link: {
      background: "transparent",
      color: "var(--citrate-green-deep)",
      border: "none",
      borderRadius: 0,
      padding: 0,
      height: "auto",
      borderBottom: "1px solid var(--citrate-green-deep)"
    }
  };
  const hovers = {
    primary: {
      background: "var(--citrate-green-deep)",
      color: "#fff",
      borderColor: "var(--citrate-green-deep)"
    },
    secondary: {
      background: "var(--ink-2)"
    },
    ghost: {
      background: "var(--paper-2)",
      borderColor: "var(--ink)"
    },
    danger: {
      background: "var(--semantic-danger)",
      color: "#fff"
    },
    link: {}
  };
  const v = variants[variant] || variants.primary;
  const h = hover ? hovers[variant] || {} : {};
  return /*#__PURE__*/React.createElement("button", _extends({
    type: type,
    onClick: onClick,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
      ...base,
      ...v,
      ...h,
      ...style
    }
  }, rest), children);
}
Object.assign(__ds_scope, { Button });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/Button.jsx", error: String((e && e.message) || e) }); }

// pitch_deck/deck-stage.js
try { (() => {
/**
 * <deck-stage> — reusable web component for HTML decks.
 *
 * Handles:
 *  (a) speaker notes — reads <script type="application/json" id="speaker-notes">
 *      and posts {slideIndexChanged: N} to the parent window on nav.
 *  (b) keyboard navigation — ←/→, PgUp/PgDn, Space, Home/End, number keys.
 *  (c) press R to reset to slide 0 (with a tasteful keyboard hint).
 *  (d) bottom-center overlay showing slide count + hints, fades out on idle.
 *  (e) auto-scaling — inner canvas is a fixed design size (default 1920×1080)
 *      scaled with `transform: scale()` to fit the viewport, letterboxed.
 *      Set the `noscale` attribute to render at authored size (1:1) — the
 *      PPTX exporter sets this so its DOM capture sees unscaled geometry.
 *  (f) print — `@media print` lays every slide out as its own page at the
 *      design size, so the browser's Print → Save as PDF produces a clean
 *      one-page-per-slide PDF with no extra setup.
 *  (g) thumbnail rail — resizable left-hand column of per-slide thumbnails
 *      (static clones). Click to navigate; ↑/↓ with a thumbnail focused to
 *      step between slides; drag to reorder; right-click for
 *      Skip / Move up / Move down / Delete (opens a Cancel/Delete confirm
 *      dialog). Drag the rail's right edge to resize; width persists to
 *      localStorage. Skipped slides carry `data-deck-skip`, are dimmed in
 *      the rail, omitted from prev/next navigation, and hidden at print.
 *      The rail is suppressed in presenting mode, in the host's Preview
 *      mode (ViewerMode='none'), on `noscale`, and via the `no-rail`
 *      attribute. Rail mutations dispatch a `deckchange`
 *      CustomEvent on the element: detail = {action, from, to, slide}.
 *
 * Slides are HIDDEN, not unmounted. Non-active slides stay in the DOM with
 * `visibility: hidden` + `opacity: 0`, so their state (videos, iframes,
 * form inputs, React trees) is preserved across navigation.
 *
 * Lifecycle event — the component dispatches a `slidechange` CustomEvent on
 * itself whenever the active slide changes (including the initial mount).
 * The event bubbles and composes out of shadow DOM, so you can listen on
 * the <deck-stage> element or on document:
 *
 *   document.querySelector('deck-stage').addEventListener('slidechange', (e) => {
 *     e.detail.index         // new 0-based index
 *     e.detail.previousIndex // previous index, or -1 on init
 *     e.detail.total         // total slide count
 *     e.detail.slide         // the new active slide element
 *     e.detail.previousSlide // the prior slide element, or null on init
 *     e.detail.reason        // 'init' | 'keyboard' | 'click' | 'tap' | 'api'
 *   });
 *
 * Persistence: none at the deck level. The host app keeps the current slide
 * in its own URL (?slide=) and re-delivers it via location.hash on load, so a
 * bare load with no hash always starts at slide 1.
 *
 * Usage:
 *   <style>deck-stage:not(:defined){visibility:hidden}</style>
 *   <deck-stage width="1920" height="1080">
 *     <section data-label="Title">...</section>
 *     <section data-label="Agenda">...</section>
 *   </deck-stage>
 *   <script src="deck-stage.js"></script>
 *
 * The :not(:defined) rule prevents a flash of the first slide at its
 * authored styles before this script runs and attaches the shadow root.
 *
 * Slides are the direct element children of <deck-stage>. Each slide is
 * automatically tagged with:
 *   - data-screen-label="NN Label"   (1-indexed, for comment flow)
 *   - data-om-validate="no_overflowing_text,no_overlapping_text,slide_sized_text"
 */

(() => {
  const DESIGN_W_DEFAULT = 1920;
  const DESIGN_H_DEFAULT = 1080;
  const OVERLAY_HIDE_MS = 1800;
  const VALIDATE_ATTR = 'no_overflowing_text,no_overlapping_text,slide_sized_text';
  const pad2 = n => String(n).padStart(2, '0');

  // Label precedence: data-label → data-screen-label (number stripped) → first heading → "Slide".
  const getSlideLabel = el => {
    const explicit = el.getAttribute('data-label');
    if (explicit) return explicit;
    const existing = el.getAttribute('data-screen-label');
    if (existing) return existing.replace(/^\s*\d+\s*/, '').trim() || existing;
    const h = el.querySelector('h1, h2, h3, [data-title]');
    const t = h && (h.textContent || '').trim().slice(0, 40);
    if (t) return t;
    return 'Slide';
  };
  const stylesheet = `
    :host {
      position: fixed;
      inset: 0;
      display: block;
      background: #000;
      color: #fff;
      font-family: -apple-system, BlinkMacSystemFont, "Helvetica Neue", Helvetica, Arial, sans-serif;
      overflow: hidden;
    }
    /* connectedCallback holds this until document.fonts.ready (capped 2s) so
     * the first visible paint has the deck's real typography + final rail
     * layout. opacity (not visibility) so the active slide can't un-hide
     * itself via the ::slotted([data-deck-active]) visibility:visible rule.
     * Only the stage/rail hide — the black :host background stays, so the
     * iframe doesn't flash the page's default white. */
    :host([data-fonts-pending]) .stage,
    :host([data-fonts-pending]) .rail { opacity: 0; pointer-events: none; }

    .stage {
      position: absolute;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
    }

    .canvas {
      position: relative;
      transform-origin: center center;
      flex-shrink: 0;
      background: #fff;
      will-change: transform;
    }

    /* Slides live in light DOM (via <slot>) so authored CSS still applies.
       We absolutely position each slotted child to stack them. */
    ::slotted(*) {
      position: absolute !important;
      inset: 0 !important;
      width: 100% !important;
      height: 100% !important;
      box-sizing: border-box !important;
      overflow: hidden;
      opacity: 0;
      pointer-events: none;
      visibility: hidden;
    }
    ::slotted([data-deck-active]) {
      opacity: 1;
      pointer-events: auto;
      visibility: visible;
    }

    /* Tap zones for mobile — back/forward thirds like Stories.
       Transparent, no visible UI, don't block the overlay. */
    .tapzones {
      position: fixed;
      inset: 0;
      display: flex;
      z-index: 2147482000;
      pointer-events: none;
    }
    .tapzone {
      flex: 1;
      pointer-events: auto;
      -webkit-tap-highlight-color: transparent;
    }
    /* Only activate tap zones on coarse pointers (touch devices). */
    @media (hover: hover) and (pointer: fine) {
      .tapzones { display: none; }
    }

    .overlay {
      position: fixed;
      left: 50%;
      bottom: 22px;
      transform: translate(-50%, 6px) scale(0.92);
      filter: blur(6px);
      display: flex;
      align-items: center;
      gap: 4px;
      padding: 4px;
      background: #000;
      color: #fff;
      border-radius: 999px;
      font-size: 12px;
      font-feature-settings: "tnum" 1;
      letter-spacing: 0.01em;
      opacity: 0;
      pointer-events: none;
      transition: opacity 260ms ease, transform 260ms cubic-bezier(.2,.8,.2,1), filter 260ms ease;
      transform-origin: center bottom;
      z-index: 2147483000;
      user-select: none;
    }
    .overlay[data-visible] {
      opacity: 1;
      pointer-events: auto;
      transform: translate(-50%, 0) scale(1);
      filter: blur(0);
    }

    .btn {
      appearance: none;
      -webkit-appearance: none;
      background: transparent;
      border: 0;
      margin: 0;
      padding: 0;
      color: inherit;
      font: inherit;
      cursor: default;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      height: 28px;
      min-width: 28px;
      border-radius: 999px;
      color: rgba(255,255,255,0.72);
      transition: background 140ms ease, color 140ms ease;
      -webkit-tap-highlight-color: transparent;
    }
    .btn:hover { background: rgba(255,255,255,0.12); color: #fff; }
    .btn:active { background: rgba(255,255,255,0.18); }
    .btn:focus { outline: none; }
    .btn:focus-visible { outline: none; }
    .btn::-moz-focus-inner { border: 0; }
    .btn svg { width: 14px; height: 14px; display: block; }
    .btn.reset {
      font-size: 11px;
      font-weight: 500;
      letter-spacing: 0.02em;
      padding: 0 10px 0 12px;
      gap: 6px;
      color: rgba(255,255,255,0.72);
    }
    .btn.reset .kbd {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 16px;
      height: 16px;
      padding: 0 4px;
      font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
      font-size: 10px;
      line-height: 1;
      color: rgba(255,255,255,0.88);
      background: rgba(255,255,255,0.12);
      border-radius: 4px;
    }

    .count {
      font-variant-numeric: tabular-nums;
      color: #fff;
      font-weight: 500;
      padding: 0 8px;
      min-width: 42px;
      text-align: center;
      font-size: 12px;
    }
    .count .sep { color: rgba(255,255,255,0.45); margin: 0 3px; font-weight: 400; }
    .count .total { color: rgba(255,255,255,0.55); }

    .divider {
      width: 1px;
      height: 14px;
      background: rgba(255,255,255,0.18);
      margin: 0 2px;
    }

    /* ── Thumbnail rail ──────────────────────────────────────────────────
       Fixed column on the left; each thumbnail is a static deep-clone of
       the light-DOM slide scaled into a 16:9 (or design-aspect) frame. The
       stage re-fits around it (see _fit); hidden during present / noscale
       / print so capture geometry and fullscreen output are unchanged. */
    .rail {
      position: fixed;
      left: 0;
      top: 0;
      bottom: 0;
      width: var(--deck-rail-w, 188px);
      background: #141414;
      border-right: 1px solid rgba(255,255,255,0.08);
      overflow-y: auto;
      overflow-x: hidden;
      padding: 12px 10px;
      box-sizing: border-box;
      display: flex;
      flex-direction: column;
      gap: 12px;
      z-index: 2147482500;
      scrollbar-width: thin;
      scrollbar-color: rgba(255,255,255,0.18) transparent;
    }
    .rail::-webkit-scrollbar { width: 8px; }
    .rail::-webkit-scrollbar-track { background: transparent; margin: 2px; }
    .rail::-webkit-scrollbar-thumb {
      background: rgba(255,255,255,0.18);
      border-radius: 4px;
      border: 2px solid transparent;
      background-clip: content-box;
    }
    .rail::-webkit-scrollbar-thumb:hover {
      background: rgba(255,255,255,0.28);
      border: 2px solid transparent;
      background-clip: content-box;
    }
    :host([no-rail]) .rail,
    :host([noscale]) .rail { display: none; }
    .rail[data-presenting] { display: none; }
    /* User-driven show/hide (the TweaksPanel toggle) slides instead of
       popping. Transitions are gated on :host([data-rail-anim]) — set only
       for the 200ms around the toggle — so window-resize and rail-width
       drag (which also call _fit) don't lag behind the cursor. */
    .rail[data-user-hidden] { transform: translateX(-100%); }
    :host([data-rail-anim]) .rail { transition: transform 200ms cubic-bezier(.3,.7,.4,1); }
    :host([data-rail-anim]) .stage { transition: left 200ms cubic-bezier(.3,.7,.4,1); }
    :host([data-rail-anim]) .canvas { transition: transform 200ms cubic-bezier(.3,.7,.4,1); }
    /* transition shorthand replaces rather than merges — repeat the base
       .overlay opacity/transform/filter transitions so visibility changes
       during the 200ms toggle window still fade instead of popping. */
    :host([data-rail-anim]) .overlay {
      transition: margin-left 200ms cubic-bezier(.3,.7,.4,1),
                  opacity 260ms ease,
                  transform 260ms cubic-bezier(.2,.8,.2,1),
                  filter 260ms ease;
    }
    :host([data-rail-anim]) .tapzones { transition: left 200ms cubic-bezier(.3,.7,.4,1); }

    .thumb {
      position: relative;
      display: flex;
      align-items: flex-start;
      gap: 8px;
      cursor: pointer;
      user-select: none;
    }
    .thumb .num {
      width: 16px;
      flex-shrink: 0;
      font-size: 11px;
      font-weight: 500;
      text-align: right;
      color: rgba(255,255,255,0.55);
      padding-top: 2px;
      font-variant-numeric: tabular-nums;
    }
    .thumb .frame {
      position: relative;
      flex: 1;
      min-width: 0;
      aspect-ratio: var(--deck-aspect);
      background: #fff;
      border-radius: 4px;
      outline: 2px solid transparent;
      outline-offset: 0;
      overflow: hidden;
      transition: outline-color 120ms ease;
    }
    .thumb:hover .frame { outline-color: rgba(255,255,255,0.25); }
    .thumb { outline: none; }
    .thumb:focus-visible .frame { outline-color: rgba(255,255,255,0.5); }
    .thumb[data-current] .num { color: #fff; }
    .thumb[data-current] .frame { outline-color: #D97757; }
    .thumb[data-dragging] { opacity: 0.35; }
    .thumb::before {
      content: '';
      position: absolute;
      left: 24px;
      right: 0;
      height: 3px;
      border-radius: 2px;
      background: #D97757;
      opacity: 0;
      pointer-events: none;
    }
    .thumb[data-drop="before"]::before { top: -8px; opacity: 1; }
    .thumb[data-drop="after"]::before { bottom: -8px; opacity: 1; }
    .thumb[data-skip] .frame { opacity: 0.35; }
    .thumb[data-skip] .frame::after {
      content: 'Skipped';
      position: absolute;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      background: rgba(0,0,0,0.45);
      color: #fff;
      font-size: 10px;
      font-weight: 500;
      letter-spacing: 0.04em;
    }

    .ctxmenu {
      position: fixed;
      min-width: 150px;
      padding: 4px;
      background: #242424;
      border: 1px solid rgba(255,255,255,0.12);
      border-radius: 7px;
      box-shadow: 0 8px 24px rgba(0,0,0,0.45);
      z-index: 2147483100;
      display: none;
      font-size: 12px;
    }
    .ctxmenu[data-open] { display: block; }
    .ctxmenu button {
      display: block;
      width: 100%;
      appearance: none;
      border: 0;
      background: transparent;
      color: #e8e8e8;
      font: inherit;
      text-align: left;
      padding: 6px 10px;
      border-radius: 4px;
      cursor: pointer;
    }
    .ctxmenu button:hover:not(:disabled) { background: rgba(255,255,255,0.08); }
    .ctxmenu button:disabled { opacity: 0.35; cursor: default; }
    .ctxmenu hr {
      border: 0;
      border-top: 1px solid rgba(255,255,255,0.1);
      margin: 4px 2px;
    }

    .rail-resize {
      position: fixed;
      left: calc(var(--deck-rail-w, 188px) - 3px);
      top: 0;
      bottom: 0;
      width: 6px;
      cursor: col-resize;
      z-index: 2147482600;
      touch-action: none;
    }
    .rail-resize:hover,
    .rail-resize[data-dragging] { background: rgba(255,255,255,0.12); }
    :host([no-rail]) .rail-resize,
    :host([noscale]) .rail-resize,
    .rail[data-presenting] + .rail-resize,
    .rail[data-user-hidden] + .rail-resize { display: none; }

    /* Delete-confirm popup — matches the SPA's ConfirmDialog layout
       (title + message body, depressed footer with Cancel / Delete). */
    .confirm-backdrop {
      position: fixed;
      inset: 0;
      background: rgba(0,0,0,0.45);
      z-index: 2147483200;
      display: none;
      align-items: center;
      justify-content: center;
    }
    .confirm-backdrop[data-open] { display: flex; }
    .confirm {
      width: 320px;
      max-width: calc(100vw - 32px);
      background: #2a2a2a;
      color: #e8e8e8;
      border: 1px solid rgba(255,255,255,0.12);
      border-radius: 12px;
      box-shadow: 0 12px 32px rgba(0,0,0,0.5);
      overflow: hidden;
      font-family: inherit;
      animation: deck-confirm-in 0.18s ease;
    }
    @keyframes deck-confirm-in {
      from { opacity: 0; transform: scale(0.96); }
      to { opacity: 1; transform: scale(1); }
    }
    .confirm .body { padding: 20px 20px 16px; }
    .confirm .title { font-size: 14px; font-weight: 600; margin-bottom: 4px; }
    .confirm .msg { font-size: 13px; line-height: 1.5; color: rgba(255,255,255,0.65); }
    .confirm .footer {
      padding: 14px 20px;
      background: #1f1f1f;
      border-top: 1px solid rgba(255,255,255,0.08);
      display: flex;
      justify-content: flex-end;
      gap: 8px;
    }
    .confirm button {
      appearance: none;
      font: inherit;
      font-size: 13px;
      font-weight: 500;
      padding: 8px 16px;
      border-radius: 8px;
      cursor: pointer;
    }
    .confirm .cancel {
      background: transparent;
      border: 0;
      color: rgba(255,255,255,0.8);
    }
    .confirm .cancel:hover { background: rgba(255,255,255,0.08); }
    .confirm .danger {
      background: #c96442;
      border: 1px solid rgba(0,0,0,0.15);
      color: #fff;
      box-shadow: 0 1px 3px rgba(166,50,68,0.3), 0 2px 6px rgba(166,50,68,0.18);
    }
    .confirm .danger:hover { background: #b5563a; }

    /* ── Print: one page per slide, no chrome ────────────────────────────
       The screen layout stacks every slide at inset:0 inside a scaled
       canvas; for print we want them in document flow at the authored
       design size so the browser paginates one slide per sheet. The
       @page size is set from the width/height attributes via the inline
       <style id="deck-stage-print-page"> that connectedCallback injects
       into <head> (the @page at-rule has no effect inside shadow DOM). */
    @media print {
      :host {
        position: static;
        inset: auto;
        background: none;
        overflow: visible;
        color: inherit;
      }
      .stage { position: static; display: block; }
      .canvas {
        transform: none !important;
        width: auto !important;
        height: auto !important;
        background: none;
        will-change: auto;
      }
      ::slotted(*) {
        position: relative !important;
        inset: auto !important;
        width: var(--deck-design-w) !important;
        height: var(--deck-design-h) !important;
        box-sizing: border-box !important;
        opacity: 1 !important;
        visibility: visible !important;
        pointer-events: auto;
        break-after: page;
        page-break-after: always;
        break-inside: avoid;
        overflow: hidden;
      }
      /* :last-child alone isn't enough once data-deck-skip hides the
         trailing slide(s) — the last *visible* slide still carries
         break-after:page and prints a blank sheet. _markLastVisible()
         maintains data-deck-last-visible on the last non-skipped slide. */
      ::slotted(*:last-child),
      ::slotted([data-deck-last-visible]) {
        break-after: auto;
        page-break-after: auto;
      }
      ::slotted([data-deck-skip]) { display: none !important; }
      .overlay, .tapzones, .rail, .rail-resize, .ctxmenu, .confirm-backdrop { display: none !important; }
    }
  `;
  class DeckStage extends HTMLElement {
    static get observedAttributes() {
      return ['width', 'height', 'noscale', 'no-rail'];
    }
    constructor() {
      super();
      this._root = this.attachShadow({
        mode: 'open'
      });
      this._index = 0;
      this._slides = [];
      this._notes = [];
      this._hideTimer = null;
      this._mouseIdleTimer = null;
      this._menuIndex = -1;
      this._onKey = this._onKey.bind(this);
      this._onResize = this._onResize.bind(this);
      this._onSlotChange = this._onSlotChange.bind(this);
      this._onMouseMove = this._onMouseMove.bind(this);
      this._onTapBack = this._onTapBack.bind(this);
      this._onTapForward = this._onTapForward.bind(this);
      this._onMessage = this._onMessage.bind(this);
      // Capture-phase close so a click anywhere dismisses the menu, but
      // ignore clicks that land inside the menu itself — otherwise the
      // capture handler runs before the menu's own (bubble) handler and
      // clears _menuIndex out from under it.
      this._onDocClick = e => {
        if (this._menu && e.composedPath && e.composedPath().includes(this._menu)) return;
        this._closeMenu();
      };
    }
    get designWidth() {
      return parseInt(this.getAttribute('width'), 10) || DESIGN_W_DEFAULT;
    }
    get designHeight() {
      return parseInt(this.getAttribute('height'), 10) || DESIGN_H_DEFAULT;
    }
    connectedCallback() {
      // Presenter-view popup loads deckUrl?_snthumb=...#N for its prev/cur/
      // next thumbnails — the rail has no business rendering inside those
      // (wrong scale, and it offsets the stage so the thumb shows a gutter).
      if (/[?&]_snthumb=/.test(location.search)) this.setAttribute('no-rail', '');
      this._render();
      this._loadNotes();
      this._syncPrintPageRule();
      window.addEventListener('keydown', this._onKey);
      window.addEventListener('resize', this._onResize);
      window.addEventListener('mousemove', this._onMouseMove, {
        passive: true
      });
      window.addEventListener('message', this._onMessage);
      window.addEventListener('click', this._onDocClick, true);
      // Initial collection + layout happens via slotchange, which fires on mount.
      this._enableRail();
      // Hold the stage hidden until webfonts are ready so the first visible
      // paint has the deck's real typography — the :not(:defined) guard in
      // the page HTML only covers custom-element upgrade, not font load.
      // Capped so a 404'd font URL can't blank the deck indefinitely.
      this.setAttribute('data-fonts-pending', '');
      const reveal = () => this.removeAttribute('data-fonts-pending');
      // rAF first: fonts.ready is a pre-resolved promise until layout has
      // resolved the slotted text's font-family and pushed a FontFace into
      // 'loading'. Reading it here in connectedCallback (parse-time) would
      // settle the race in a microtask before any font fetch starts.
      requestAnimationFrame(() => {
        Promise.race([document.fonts ? document.fonts.ready : Promise.resolve(), new Promise(r => setTimeout(r, 2000))]).then(reveal, reveal);
      });
    }
    _enableRail() {
      // Idempotent — older host builds still post __omelette_rail_enabled.
      // no-rail guard keeps the observers/stylesheet walk off the cheap path
      // for presenter-popup thumbnail iframes (up to 9 per view).
      if (this._railEnabled || this.hasAttribute('no-rail')) return;
      this._railEnabled = true;
      // Per-viewer preference — restored alongside rail width. Default on;
      // only a stored '0' (from the TweaksPanel toggle) hides it.
      this._railVisible = true;
      try {
        if (localStorage.getItem('deck-stage.railVisible') === '0') this._railVisible = false;
      } catch (e) {}
      // Live thumbnail updates: watch the light-DOM slides for content
      // edits and re-clone just the affected thumb(s), debounced. Ignore
      // the data-deck-* / data-screen-label / data-om-validate attributes
      // this component itself writes so nav and skip don't trigger
      // spurious refreshes.
      const OWN_ATTRS = /^data-(deck-|screen-label$|om-validate$)/;
      this._liveDirty = new Set();
      this._liveObserver = new MutationObserver(records => {
        for (const r of records) {
          if (r.type === 'attributes' && OWN_ATTRS.test(r.attributeName || '')) continue;
          let n = r.target;
          while (n && n.parentElement !== this) n = n.parentElement;
          if (n && this._slideSet && this._slideSet.has(n)) this._liveDirty.add(n);
        }
        if (this._liveDirty.size && !this._liveTimer) {
          this._liveTimer = setTimeout(() => {
            this._liveTimer = null;
            this._liveDirty.forEach(s => this._refreshThumb(s));
            this._liveDirty.clear();
          }, 200);
        }
      });
      this._liveObserver.observe(this, {
        subtree: true,
        childList: true,
        characterData: true,
        attributes: true
      });
      // Lazy thumbnail materialization — clone the slide only when its
      // frame scrolls into (or near) the rail viewport. rootMargin gives
      // ~4 thumbs of pre-load so fast scrolling doesn't flash blanks.
      this._railObserver = new IntersectionObserver(entries => {
        entries.forEach(e => {
          if (e.isIntersecting && e.target.__deckThumb) {
            this._materialize(e.target.__deckThumb);
          }
        });
      }, {
        root: this._rail,
        rootMargin: '400px 0px'
      });
      // Tweaks typically change CSS vars / attrs OUTSIDE <deck-stage>
      // (on <html>, <body>, a wrapper div, or a <style> tag), which
      // _liveObserver can't see. Re-snapshot author CSS (constructable
      // sheet is shared by reference, so one replaceSync updates every
      // thumb shadow root) and re-sync each thumb host's attrs + custom
      // properties. In-slide DOM mutations are _liveObserver's job.
      // Debounced so slider drags don't thrash.
      this._onTweakChange = () => {
        clearTimeout(this._tweakTimer);
        this._tweakTimer = setTimeout(() => {
          this._snapshotAuthorCss();
          // One getComputedStyle for the whole batch — each
          // getPropertyValue read below reuses the same computed style
          // as long as nothing invalidates layout between thumbs.
          const cs = getComputedStyle(this);
          (this._thumbs || []).forEach(t => {
            if (t.host) this._syncThumbHostAttrs(t.host, cs);
          });
        }, 120);
      };
      window.addEventListener('tweakchange', this._onTweakChange);
      this._snapshotAuthorCss();
      // Build the rail now that it's enabled — slotchange already fired,
      // so _renderRail's early-return skipped the initial build.
      this._syncRailHidden();
      this._renderRail();
      this._fit();
    }

    /** Snapshot document stylesheets into a constructable sheet that each
     *  thumbnail's nested shadow root adopts — so author CSS styles the
     *  cloned slide content without touching this component's chrome.
     *  Cross-origin sheets throw on .cssRules — skip them. Re-callable:
     *  the existing constructable sheet is reused via replaceSync so every
     *  already-adopted shadow root picks up the fresh CSS without re-adopt. */
    _snapshotAuthorCss() {
      // :root in an adopted sheet inside a shadow root matches nothing
      // (only the document root qualifies), so author rules like
      // `:root[data-voice="modern"] .serif` never reach the clones.
      // Rewrite :root → :host and mirror <html>'s data-*/class/lang onto
      // each thumb host (see _syncThumbHostAttrs) so the same selectors
      // match inside the thumbnail's shadow tree.
      const authorCss = Array.from(document.styleSheets).map(sh => {
        try {
          return Array.from(sh.cssRules).map(r => r.cssText).join('\n');
        } catch (e) {
          return '';
        }
      }).join('\n')
      // The shadow host is featureless outside the functional :host(...)
      // form, so any compound on :root — [attr], .class, #id, :pseudo —
      // must become :host(<compound>) not :host<compound>. Same for the
      // html type selector (Tailwind class-strategy dark mode emits
      // html.dark; Pico uses html[data-theme]), which has nothing to
      // match inside the thumb's shadow tree.
      .replace(/:root((?:\[[^\]]*\]|[.#][-\w]+|:[-\w]+(?:\([^)]*\))?)+)/g, ':host($1)').replace(/:root\b/g, ':host').replace(/(^|[\s,>~+(}])html((?:\[[^\]]*\]|[.#][-\w]+|:[-\w]+(?:\([^)]*\))?)+)(?![-\w])/g, '$1:host($2)').replace(/(^|[\s,>~+(}])html(?![-\w])/g, '$1:host');
      // Every custom property the author references. _syncThumbHostAttrs
      // mirrors each one's *computed* value at <deck-stage> onto the
      // thumb host so the live value wins over the :host default above
      // regardless of which ancestor the tweak wrote to (<html>, <body>,
      // a wrapper div, or the deck-stage element itself all inherit
      // down to getComputedStyle(this)).
      this._authorVars = new Set(authorCss.match(/--[\w-]+/g) || []);
      try {
        if (!this._adoptedSheet) this._adoptedSheet = new CSSStyleSheet();
        this._adoptedSheet.replaceSync(authorCss);
      } catch (e) {
        this._adoptedSheet = null;
        this._authorCss = authorCss;
      }
    }
    _syncThumbHostAttrs(host, cs) {
      const de = document.documentElement;
      // setAttribute overwrites but can't delete — an attr removed from
      // <html> (toggleAttribute off, classList emptied) would linger on
      // the host and :host([data-*]) / :host(.foo) rules would keep
      // matching. Remove stale mirrored attrs first; iterate backward
      // because removeAttribute mutates the live NamedNodeMap.
      for (let i = host.attributes.length - 1; i >= 0; i--) {
        const n = host.attributes[i].name;
        if ((n.startsWith('data-') || n === 'class' || n === 'lang') && !de.hasAttribute(n)) {
          host.removeAttribute(n);
        }
      }
      for (const a of de.attributes) {
        if (a.name.startsWith('data-') || a.name === 'class' || a.name === 'lang') {
          host.setAttribute(a.name, a.value);
        }
      }
      // The :root→:host rewrite in _snapshotAuthorCss pins each custom
      // property to its stylesheet default on the thumb host, shadowing
      // the live value that would otherwise inherit. Tweaks can write the
      // live value on any ancestor — <html>, <body>, a wrapper div, the
      // deck-stage element — so read it as the *computed* value at
      // <deck-stage> (which sees the whole inheritance chain) rather than
      // trying to guess which element the author wrote to. Inline on the
      // host beats the :host{} rule. remove-stale covers vars dropped
      // from the stylesheet between snapshots.
      const vars = this._authorVars || new Set();
      for (let i = host.style.length - 1; i >= 0; i--) {
        const p = host.style[i];
        if (p.startsWith('--') && !vars.has(p)) host.style.removeProperty(p);
      }
      const live = cs || getComputedStyle(this);
      vars.forEach(p => {
        const v = live.getPropertyValue(p);
        if (v) host.style.setProperty(p, v.trim());else host.style.removeProperty(p);
      });
    }
    disconnectedCallback() {
      window.removeEventListener('keydown', this._onKey);
      window.removeEventListener('resize', this._onResize);
      window.removeEventListener('mousemove', this._onMouseMove);
      window.removeEventListener('message', this._onMessage);
      window.removeEventListener('click', this._onDocClick, true);
      if (this._hideTimer) clearTimeout(this._hideTimer);
      if (this._mouseIdleTimer) clearTimeout(this._mouseIdleTimer);
      if (this._liveTimer) clearTimeout(this._liveTimer);
      if (this._tweakTimer) clearTimeout(this._tweakTimer);
      if (this._railAnimTimer) clearTimeout(this._railAnimTimer);
      if (this._scaleRaf) cancelAnimationFrame(this._scaleRaf);
      if (this._liveObserver) this._liveObserver.disconnect();
      if (this._railObserver) this._railObserver.disconnect();
      if (this._onTweakChange) window.removeEventListener('tweakchange', this._onTweakChange);
    }
    attributeChangedCallback() {
      if (this._canvas) {
        this._canvas.style.width = this.designWidth + 'px';
        this._canvas.style.height = this.designHeight + 'px';
        this._canvas.style.setProperty('--deck-design-w', this.designWidth + 'px');
        this._canvas.style.setProperty('--deck-design-h', this.designHeight + 'px');
        if (this._rail) {
          this._rail.style.setProperty('--deck-aspect', this.designWidth + '/' + this.designHeight);
        }
        this._fit();
        this._scaleThumbs();
        this._syncPrintPageRule();
      }
    }
    _render() {
      const style = document.createElement('style');
      style.textContent = stylesheet;
      const stage = document.createElement('div');
      stage.className = 'stage';
      const canvas = document.createElement('div');
      canvas.className = 'canvas';
      canvas.style.width = this.designWidth + 'px';
      canvas.style.height = this.designHeight + 'px';
      canvas.style.setProperty('--deck-design-w', this.designWidth + 'px');
      canvas.style.setProperty('--deck-design-h', this.designHeight + 'px');
      const slot = document.createElement('slot');
      slot.addEventListener('slotchange', this._onSlotChange);
      canvas.appendChild(slot);
      stage.appendChild(canvas);

      // Tap zones (mobile): left third = back, right third = forward.
      const tapzones = document.createElement('div');
      tapzones.className = 'tapzones export-hidden';
      tapzones.setAttribute('aria-hidden', 'true');
      tapzones.setAttribute('data-noncommentable', '');
      const tzBack = document.createElement('div');
      tzBack.className = 'tapzone tapzone--back';
      const tzMid = document.createElement('div');
      tzMid.className = 'tapzone tapzone--mid';
      tzMid.style.pointerEvents = 'none';
      const tzFwd = document.createElement('div');
      tzFwd.className = 'tapzone tapzone--fwd';
      tzBack.addEventListener('click', this._onTapBack);
      tzFwd.addEventListener('click', this._onTapForward);
      tapzones.append(tzBack, tzMid, tzFwd);

      // Overlay: compact, solid black, with clickable controls.
      const overlay = document.createElement('div');
      overlay.className = 'overlay export-hidden';
      overlay.setAttribute('role', 'toolbar');
      overlay.setAttribute('aria-label', 'Deck controls');
      overlay.setAttribute('data-noncommentable', '');
      overlay.innerHTML = `
        <button class="btn prev" type="button" aria-label="Previous slide" title="Previous (←)">
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 3L5 8l5 5"/></svg>
        </button>
        <span class="count" aria-live="polite"><span class="current">1</span><span class="sep">/</span><span class="total">1</span></span>
        <button class="btn next" type="button" aria-label="Next slide" title="Next (→)">
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 3l5 5-5 5"/></svg>
        </button>
        <span class="divider"></span>
        <button class="btn reset" type="button" aria-label="Reset to first slide" title="Reset (R)">Reset<span class="kbd">R</span></button>
      `;
      overlay.querySelector('.prev').addEventListener('click', () => this._advance(-1, 'click'));
      overlay.querySelector('.next').addEventListener('click', () => this._advance(1, 'click'));
      overlay.querySelector('.reset').addEventListener('click', () => this._go(0, 'click'));

      // Thumbnail rail + context menu. Thumbnails are populated in
      // _renderRail() after _collectSlides().
      const rail = document.createElement('div');
      rail.className = 'rail export-hidden';
      rail.setAttribute('data-noncommentable', '');
      rail.style.setProperty('--deck-aspect', this.designWidth + '/' + this.designHeight);
      // Edge auto-scroll while dragging a thumb near the rail's top/bottom
      // so off-screen drop targets are reachable. Native dragover fires
      // continuously while the pointer is stationary, so a per-event nudge
      // (ramped by edge proximity) is enough — no rAF loop needed.
      rail.addEventListener('dragover', e => {
        if (this._dragFrom == null) return;
        const r = rail.getBoundingClientRect();
        const EDGE = 40;
        const dt = e.clientY - r.top;
        const db = r.bottom - e.clientY;
        if (dt < EDGE) rail.scrollTop -= Math.ceil((EDGE - dt) / 3);else if (db < EDGE) rail.scrollTop += Math.ceil((EDGE - db) / 3);
      });
      const menu = document.createElement('div');
      menu.className = 'ctxmenu export-hidden';
      menu.setAttribute('data-noncommentable', '');
      menu.innerHTML = `
        <button type="button" data-act="skip">Skip slide</button>
        <button type="button" data-act="up">Move up</button>
        <button type="button" data-act="down">Move down</button>
        <hr>
        <button type="button" data-act="delete">Delete slide</button>
      `;
      menu.addEventListener('click', e => {
        const act = e.target && e.target.getAttribute && e.target.getAttribute('data-act');
        if (!act) return;
        const i = this._menuIndex;
        this._closeMenu();
        if (act === 'skip') this._toggleSkip(i);else if (act === 'up') this._moveSlide(i, i - 1);else if (act === 'down') this._moveSlide(i, i + 1);else if (act === 'delete') this._openConfirm(i);
      });
      menu.addEventListener('contextmenu', e => e.preventDefault());

      // Rail resize handle — drag to set --deck-rail-w, persisted to
      // localStorage so the width survives reloads.
      const resize = document.createElement('div');
      resize.className = 'rail-resize export-hidden';
      resize.setAttribute('data-noncommentable', '');
      resize.addEventListener('pointerdown', e => {
        e.preventDefault();
        resize.setPointerCapture(e.pointerId);
        resize.setAttribute('data-dragging', '');
        const move = ev => this._setRailWidth(ev.clientX);
        const up = () => {
          resize.removeEventListener('pointermove', move);
          resize.removeEventListener('pointerup', up);
          resize.removeEventListener('pointercancel', up);
          resize.removeAttribute('data-dragging');
          try {
            localStorage.setItem('deck-stage.railWidth', String(this._railPx));
          } catch (err) {}
        };
        resize.addEventListener('pointermove', move);
        resize.addEventListener('pointerup', up);
        resize.addEventListener('pointercancel', up);
      });

      // Delete-confirm dialog — mirrors the SPA's ConfirmDialog layout.
      const confirm = document.createElement('div');
      confirm.className = 'confirm-backdrop export-hidden';
      confirm.setAttribute('data-noncommentable', '');
      confirm.innerHTML = `
        <div class="confirm" role="dialog" aria-modal="true">
          <div class="body">
            <div class="title">Delete slide?</div>
            <div class="msg">This slide will be removed from the deck.</div>
          </div>
          <div class="footer">
            <button type="button" class="cancel">Cancel</button>
            <button type="button" class="danger">Delete</button>
          </div>
        </div>
      `;
      confirm.addEventListener('click', e => {
        if (e.target === confirm) this._closeConfirm();
      });
      confirm.querySelector('.cancel').addEventListener('click', () => this._closeConfirm());
      confirm.querySelector('.danger').addEventListener('click', () => {
        const i = this._confirmIndex;
        this._closeConfirm();
        this._deleteSlide(i);
      });
      this._root.append(style, rail, resize, stage, tapzones, overlay, menu, confirm);
      this._canvas = canvas;
      this._slot = slot;
      this._overlay = overlay;
      this._tapzones = tapzones;
      this._rail = rail;
      this._resize = resize;
      this._menu = menu;
      this._confirm = confirm;
      this._countEl = overlay.querySelector('.current');
      this._totalEl = overlay.querySelector('.total');

      // Restore persisted rail width.
      let rw = 188;
      try {
        const s = localStorage.getItem('deck-stage.railWidth');
        if (s) rw = parseInt(s, 10) || rw;
      } catch (err) {}
      this._setRailWidth(rw);
      this._syncRailHidden();
    }
    _setRailWidth(px) {
      const w = Math.max(120, Math.min(360, Math.round(px)));
      this._railPx = w;
      this.style.setProperty('--deck-rail-w', w + 'px');
      this._fit();
      // _scaleThumbs forces a sync layout (frame.offsetWidth) then writes
      // N transforms. During a resize drag this runs per-pointermove;
      // coalesce to one per frame.
      if (!this._scaleRaf) {
        this._scaleRaf = requestAnimationFrame(() => {
          this._scaleRaf = null;
          this._scaleThumbs();
        });
      }
    }

    /** @page must live in the document stylesheet — it's a no-op inside
     *  shadow DOM. Inject/update a single <head> style tag so the print
     *  sheet matches the design size and Save-as-PDF yields one slide per
     *  page with no margins. */
    _syncPrintPageRule() {
      const id = 'deck-stage-print-page';
      let tag = document.getElementById(id);
      if (!tag) {
        tag = document.createElement('style');
        tag.id = id;
        document.head.appendChild(tag);
      }
      tag.textContent = '@page { size: ' + this.designWidth + 'px ' + this.designHeight + 'px; margin: 0; } ' + '@media print { html, body { margin: 0 !important; padding: 0 !important; background: none !important; overflow: visible !important; height: auto !important; } ' + '* { -webkit-print-color-adjust: exact; print-color-adjust: exact; } }';
    }
    _onSlotChange() {
      // Rail mutations (delete/move) already reconcile synchronously and
      // emit slidechange with reason 'api'; skip the async slotchange that
      // would otherwise re-broadcast with reason 'init'.
      if (this._squelchSlotChange) {
        this._squelchSlotChange = false;
        return;
      }
      this._collectSlides();
      this._restoreIndex();
      this._applyIndex({
        showOverlay: false,
        broadcast: true,
        reason: 'init'
      });
      this._fit();
    }
    _collectSlides() {
      const assigned = this._slot.assignedElements({
        flatten: true
      });
      this._slides = assigned.filter(el => {
        // Skip template/style/script nodes even if someone slots them.
        const tag = el.tagName;
        return tag !== 'TEMPLATE' && tag !== 'SCRIPT' && tag !== 'STYLE';
      });
      this._slideSet = new Set(this._slides);
      this._slides.forEach((slide, i) => {
        const n = i + 1;
        slide.setAttribute('data-screen-label', `${pad2(n)} ${getSlideLabel(slide)}`);

        // Validation attribute for comment flow / auto-checks.
        if (!slide.hasAttribute('data-om-validate')) {
          slide.setAttribute('data-om-validate', VALIDATE_ATTR);
        }
        slide.setAttribute('data-deck-slide', String(i));
      });
      if (this._totalEl) this._totalEl.textContent = String(this._slides.length || 1);
      if (this._index >= this._slides.length) this._index = Math.max(0, this._slides.length - 1);
      this._markLastVisible();
      this._renderRail();
    }

    /** Tag the last non-skipped slide so print CSS can drop its
     *  break-after (see the @media print comment above — :last-child
     *  alone matches a hidden skipped slide). */
    _markLastVisible() {
      let last = null;
      this._slides.forEach(s => {
        s.removeAttribute('data-deck-last-visible');
        if (!s.hasAttribute('data-deck-skip')) last = s;
      });
      if (last) last.setAttribute('data-deck-last-visible', '');
    }
    _loadNotes() {
      const tag = document.getElementById('speaker-notes');
      if (!tag) {
        this._notes = [];
        return;
      }
      try {
        const parsed = JSON.parse(tag.textContent || '[]');
        if (Array.isArray(parsed)) this._notes = parsed;
      } catch (e) {
        console.warn('[deck-stage] Failed to parse #speaker-notes JSON:', e);
        this._notes = [];
      }
    }
    _restoreIndex() {
      // The host's ?slide= param is delivered as a #<int> hash (1-indexed) on
      // the iframe src. No hash → slide 1; the deck itself keeps no position
      // state across loads.
      const h = (location.hash || '').match(/^#(\d+)$/);
      if (h) {
        const n = parseInt(h[1], 10) - 1;
        if (n >= 0 && n < this._slides.length) this._index = n;
      }
    }
    _applyIndex({
      showOverlay = true,
      broadcast = true,
      reason = 'init'
    } = {}) {
      if (!this._slides.length) return;
      const prev = this._prevIndex == null ? -1 : this._prevIndex;
      const curr = this._index;
      // Keep the iframe's own hash in sync so an in-iframe location.reload()
      // (reload banner path in viewer-handle.ts) lands on the current slide,
      // not the stale deep-link hash from initial load.
      try {
        history.replaceState(null, '', '#' + (curr + 1));
      } catch (e) {}
      this._slides.forEach((s, i) => {
        if (i === curr) s.setAttribute('data-deck-active', '');else s.removeAttribute('data-deck-active');
      });
      if (this._countEl) this._countEl.textContent = String(curr + 1);
      // Follow-scroll on every navigation (init deep-link, keyboard, click,
      // tap, external goTo) — the only time we *don't* want the rail to
      // track current is after a rail-internal mutation, where _renderRail
      // has already restored the user's scroll position and yanking back to
      // current would undo it.
      this._syncRail(reason !== 'mutation');
      if (broadcast) {
        // (1) Legacy: host-window postMessage for speaker-notes renderers.
        try {
          window.postMessage({
            slideIndexChanged: curr,
            deckTotal: this._slides.length,
            deckSkipped: this._skippedIndices()
          }, '*');
        } catch (e) {}

        // (2) In-page CustomEvent on the <deck-stage> element itself.
        //     Bubbles and composes out of shadow DOM so slide code can listen:
        //       document.querySelector('deck-stage').addEventListener('slidechange', e => {
        //         e.detail.index, e.detail.previousIndex, e.detail.total, e.detail.slide, e.detail.reason
        //       });
        const detail = {
          index: curr,
          previousIndex: prev,
          total: this._slides.length,
          slide: this._slides[curr] || null,
          previousSlide: prev >= 0 ? this._slides[prev] || null : null,
          reason: reason // 'init' | 'keyboard' | 'click' | 'tap' | 'api'
        };
        this.dispatchEvent(new CustomEvent('slidechange', {
          detail,
          bubbles: true,
          composed: true
        }));
      }
      this._prevIndex = curr;
      if (showOverlay) this._flashOverlay();
    }
    _flashOverlay() {
      // Host posts __omelette_presenting while in fullscreen/tab presentation
      // mode — suppress the nav footer entirely (both hover and slide-change
      // flash) so the audience sees clean slides.
      if (!this._overlay || this._presenting) return;
      this._overlay.setAttribute('data-visible', '');
      if (this._hideTimer) clearTimeout(this._hideTimer);
      this._hideTimer = setTimeout(() => {
        this._overlay.removeAttribute('data-visible');
      }, OVERLAY_HIDE_MS);
    }
    _railWidth() {
      // State-based, no offsetWidth: the first _fit() can run before the
      // rail has had layout on some load paths, and a 0 there paints the
      // slide full-width for one frame before the post-slotchange _fit()
      // corrects it.
      if (!this._railEnabled || !this._railVisible || this.hasAttribute('no-rail') || this.hasAttribute('noscale') || this._presenting || this._previewMode) return 0;
      return this._railPx || 0;
    }
    _fit() {
      if (!this._canvas) return;
      const stage = this._canvas.parentElement;
      // PPTX export sets noscale so the DOM capture sees authored-size
      // geometry — the scaled canvas is in shadow DOM, so the exporter's
      // resetTransformSelector can't reach .canvas.style.transform directly.
      if (this.hasAttribute('noscale')) {
        this._canvas.style.transform = 'none';
        if (stage) stage.style.left = '0';
        if (this._overlay) this._overlay.style.marginLeft = '0';
        if (this._tapzones) this._tapzones.style.left = '0';
        return;
      }
      const rw = this._railWidth();
      if (stage) stage.style.left = rw + 'px';
      // Overlay is centred on the viewport via left:50% + translate(-50%);
      // marginLeft shifts the centre by rw/2 so it lands in the middle of
      // the [rw, innerWidth] stage region. Tapzones just inset from rw.
      if (this._overlay) this._overlay.style.marginLeft = rw / 2 + 'px';
      if (this._tapzones) this._tapzones.style.left = rw + 'px';
      const vw = window.innerWidth - rw;
      const vh = window.innerHeight;
      const s = Math.min(vw / this.designWidth, vh / this.designHeight);
      this._canvas.style.transform = `scale(${s})`;
    }
    _onResize() {
      this._fit();
    }
    _onMouseMove() {
      // Keep overlay visible while mouse moves; hide after idle.
      this._flashOverlay();
    }
    _onMessage(e) {
      const d = e.data;
      if (d && typeof d.__omelette_presenting === 'boolean') {
        this._presenting = d.__omelette_presenting;
        if (this._presenting && this._overlay) {
          this._overlay.removeAttribute('data-visible');
          if (this._hideTimer) clearTimeout(this._hideTimer);
        }
        this._syncRailHidden();
        this._closeMenu();
        this._closeConfirm();
        this._fit();
        this._scaleThumbs();
      }
      // Host's Preview segment (ViewerMode='none'): the rail's drag-reorder /
      // right-click skip-delete affordances are editing chrome, so hide it
      // while the user is just looking at the deck. Same hard-hide path as
      // presenting; independent of the user's _railVisible preference so
      // returning to Edit restores whatever they had.
      if (d && typeof d.__omelette_preview_mode === 'boolean') {
        if (d.__omelette_preview_mode === this._previewMode) return;
        this._previewMode = d.__omelette_preview_mode;
        this._syncRailHidden();
        this._closeMenu();
        this._closeConfirm();
        this._fit();
        this._scaleThumbs();
      }
      // Per-viewer show/hide, driven by the TweaksPanel's auto-injected
      // "Thumbnail rail" toggle (or any author script). Independent of
      // whether the Tweaks panel itself is open — closing the panel
      // doesn't change rail visibility. Persists alongside rail width.
      if (d && d.type === '__deck_rail_visible' && typeof d.on === 'boolean') {
        if (d.on === this._railVisible) return;
        this._railVisible = d.on;
        try {
          localStorage.setItem('deck-stage.railVisible', d.on ? '1' : '0');
        } catch (e) {}
        // Arm the transition, commit it, then flip state — otherwise the
        // browser coalesces both writes and nothing animates on show.
        this.setAttribute('data-rail-anim', '');
        void (this._rail && this._rail.offsetHeight);
        this._syncRailHidden();
        this._fit();
        this._scaleThumbs();
        clearTimeout(this._railAnimTimer);
        this._railAnimTimer = setTimeout(() => this.removeAttribute('data-rail-anim'), 220);
      }
      if (d && d.type === '__omelette_rail_enabled') this._enableRail();
    }
    _syncRailHidden() {
      if (!this._rail) return;
      // data-presenting is the hard hide (display:none) for flag-off,
      // presentation mode, and the host's Preview segment — instant, no
      // transition. data-user-hidden is the soft hide (translateX(-100%))
      // for the viewer's rail toggle, so show/hide slides under
      // :host([data-rail-anim]).
      const hard = !this._railEnabled || this._presenting || this._previewMode;
      if (hard) this._rail.setAttribute('data-presenting', '');else this._rail.removeAttribute('data-presenting');
      if (!this._railVisible) this._rail.setAttribute('data-user-hidden', '');else this._rail.removeAttribute('data-user-hidden');
      // translateX hide leaves thumbs (tabIndex=0) in the tab order —
      // inert keeps them unfocusable while the rail is off-screen.
      this._rail.inert = hard || !this._railVisible;
    }
    _onTapBack(e) {
      e.preventDefault();
      this._advance(-1, 'tap');
    }
    _onTapForward(e) {
      e.preventDefault();
      this._advance(1, 'tap');
    }
    _onKey(e) {
      // Ignore when the user is typing.
      const t = e.target;
      if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
      // Confirm dialog swallows nav keys while open; Escape cancels. Enter
      // is left to the focused button's native activation so Tab→Cancel
      // →Enter activates Cancel, not the window-level confirm path.
      if (this._confirm && this._confirm.hasAttribute('data-open')) {
        if (e.key === 'Escape') {
          this._closeConfirm();
          e.preventDefault();
        }
        return;
      }
      if (e.key === 'Escape' && this._menu && this._menu.hasAttribute('data-open')) {
        this._closeMenu();
        e.preventDefault();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const key = e.key;
      let handled = true;
      if (key === 'ArrowRight' || key === 'PageDown' || key === ' ' || key === 'Spacebar') {
        this._advance(1, 'keyboard');
      } else if (key === 'ArrowLeft' || key === 'PageUp') {
        this._advance(-1, 'keyboard');
      } else if (key === 'Home') {
        this._go(0, 'keyboard');
      } else if (key === 'End') {
        this._go(this._slides.length - 1, 'keyboard');
      } else if (key === 'r' || key === 'R') {
        this._go(0, 'keyboard');
      } else if (/^[0-9]$/.test(key)) {
        // 1..9 jump to that slide; 0 jumps to 10.
        const n = key === '0' ? 9 : parseInt(key, 10) - 1;
        if (n < this._slides.length) this._go(n, 'keyboard');
      } else {
        handled = false;
      }
      if (handled) {
        e.preventDefault();
        this._flashOverlay();
      }
    }
    _go(i, reason = 'api') {
      if (!this._slides.length) return;
      const clamped = Math.max(0, Math.min(this._slides.length - 1, i));
      if (clamped === this._index) {
        this._flashOverlay();
        return;
      }
      this._index = clamped;
      this._applyIndex({
        showOverlay: true,
        broadcast: true,
        reason
      });
    }

    /** Step forward/back skipping any slide marked data-deck-skip. Falls
     *  back to _go's clamp-at-ends behaviour (flash overlay) when there's
     *  nothing further in that direction. */
    _advance(dir, reason) {
      if (!this._slides.length) return;
      let i = this._index + dir;
      while (i >= 0 && i < this._slides.length && this._slides[i].hasAttribute('data-deck-skip')) {
        i += dir;
      }
      if (i < 0 || i >= this._slides.length) {
        this._flashOverlay();
        return;
      }
      this._go(i, reason);
    }

    // ── Thumbnail rail ────────────────────────────────────────────────────
    //
    // Thumbs are keyed by slide element and reused across _renderRail()
    // calls, so a reorder/delete is an O(changed) DOM shuffle instead of an
    // O(N) teardown-and-re-clone. Each thumb starts as a lightweight shell
    // (num + empty frame); the clone is materialized lazily by an
    // IntersectionObserver when the frame scrolls into (or near) view, so
    // only visible-ish slides pay the clone + image-decode cost.

    _renderRail() {
      if (!this._rail || !this._railEnabled) {
        this._thumbs = [];
        return;
      }
      // FLIP: record each *materialized* thumb's top before the reconcile.
      // Off-screen (non-materialized) thumbs don't need the animation and
      // skipping their getBoundingClientRect saves a forced layout per
      // off-screen thumb on large decks.
      const prevTops = new Map();
      (this._thumbs || []).forEach(({
        thumb,
        slide,
        host
      }) => {
        if (host) prevTops.set(slide, thumb.getBoundingClientRect().top);
      });
      const st = this._rail.scrollTop;

      // Reconcile: reuse thumbs that already exist for a slide, create
      // shells for new slides, drop thumbs for removed slides.
      const bySlide = new Map();
      (this._thumbs || []).forEach(t => bySlide.set(t.slide, t));
      const next = [];
      this._slides.forEach(slide => {
        let t = bySlide.get(slide);
        if (t) bySlide.delete(slide);else t = this._makeThumb(slide);
        next.push(t);
      });
      // Orphans — slides removed since last render.
      bySlide.forEach(t => {
        if (this._railObserver) this._railObserver.unobserve(t.frame);
        t.thumb.remove();
      });
      // Put thumbs into document order to match _slides. insertBefore on
      // an already-correctly-placed node is a no-op, so this is cheap
      // when nothing moved.
      next.forEach((t, i) => {
        const want = t.thumb;
        const at = this._rail.children[i];
        if (at !== want) this._rail.insertBefore(want, at || null);
        t.i = i;
        t.num.textContent = String(i + 1);
        if (t.slide.hasAttribute('data-deck-skip')) t.thumb.setAttribute('data-skip', '');else t.thumb.removeAttribute('data-skip');
      });
      this._thumbs = next;
      this._rail.scrollTop = st;
      if (prevTops.size) {
        const moved = [];
        this._thumbs.forEach(({
          thumb,
          slide
        }) => {
          const old = prevTops.get(slide);
          if (old == null) return;
          const dy = old - thumb.getBoundingClientRect().top;
          if (Math.abs(dy) < 1) return;
          thumb.style.transition = 'none';
          thumb.style.transform = `translateY(${dy}px)`;
          moved.push(thumb);
        });
        if (moved.length) {
          // Commit the inverted positions before flipping the transition
          // on — otherwise the browser coalesces both style writes and
          // nothing animates.
          void this._rail.offsetHeight;
          moved.forEach(t => {
            t.style.transition = 'transform 180ms cubic-bezier(.2,.7,.3,1)';
            t.style.transform = '';
          });
          setTimeout(() => moved.forEach(t => {
            t.style.transition = '';
          }), 220);
        }
      }
      requestAnimationFrame(() => this._scaleThumbs());
      this._syncRail(false);
    }

    /** Create a lightweight thumb shell for one slide. The clone is
     *  materialized later by the IntersectionObserver. Event handlers
     *  look up the thumb's *current* index (via _thumbs.indexOf) so the
     *  same element can be reused across reorders. */
    _makeThumb(slide) {
      const thumb = document.createElement('div');
      thumb.className = 'thumb';
      thumb.tabIndex = 0;
      const num = document.createElement('div');
      num.className = 'num';
      const frame = document.createElement('div');
      frame.className = 'frame';
      thumb.append(num, frame);
      const entry = {
        thumb,
        num,
        frame,
        slide,
        clone: null,
        host: null,
        i: -1
      };
      // entry.i is refreshed on every _renderRail reconcile pass, so
      // handlers read the thumb's current position without an O(N) scan.
      const idx = () => entry.i;
      thumb.addEventListener('click', () => this._go(idx(), 'click'));
      // ↑/↓ step through the rail when a thumb has focus. _go clamps at the
      // ends and _applyIndex→_syncRail scrolls the new current thumb into
      // view; we move focus to it (preventScroll — _syncRail already
      // scrolled) so a held key walks the whole list. stopPropagation keeps
      // this out of the window-level _onKey nav handler.
      thumb.addEventListener('keydown', e => {
        if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return;
        if (e.metaKey || e.ctrlKey || e.altKey) return;
        e.preventDefault();
        e.stopPropagation();
        this._go(idx() + (e.key === 'ArrowDown' ? 1 : -1), 'keyboard');
        const cur = this._thumbs && this._thumbs[this._index];
        if (cur) cur.thumb.focus({
          preventScroll: true
        });
      });
      thumb.addEventListener('contextmenu', e => {
        e.preventDefault();
        this._openMenu(idx(), e.clientX, e.clientY);
      });
      thumb.draggable = true;
      thumb.addEventListener('dragstart', e => {
        this._dragFrom = idx();
        thumb.setAttribute('data-dragging', '');
        e.dataTransfer.effectAllowed = 'move';
        try {
          e.dataTransfer.setData('text/plain', String(this._dragFrom));
        } catch (err) {}
      });
      thumb.addEventListener('dragend', () => {
        thumb.removeAttribute('data-dragging');
        this._clearDrop();
        this._dragFrom = null;
      });
      thumb.addEventListener('dragover', e => {
        if (this._dragFrom == null) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        const r = thumb.getBoundingClientRect();
        this._setDrop(idx(), e.clientY < r.top + r.height / 2 ? 'before' : 'after');
      });
      thumb.addEventListener('drop', e => {
        if (this._dragFrom == null) return;
        e.preventDefault();
        const i = idx();
        const r = thumb.getBoundingClientRect();
        let to = e.clientY >= r.top + r.height / 2 ? i + 1 : i;
        if (this._dragFrom < to) to--;
        const from = this._dragFrom;
        this._clearDrop();
        this._dragFrom = null;
        if (to !== from) this._moveSlide(from, to);
      });
      if (this._railObserver) this._railObserver.observe(frame);
      frame.__deckThumb = entry;
      return entry;
    }

    /** Lazily build the clone for a thumb that has scrolled into view. */
    _materialize(entry) {
      if (entry.host) return;
      const dw = this.designWidth,
        dh = this.designHeight;
      let clone = entry.slide.cloneNode(true);
      clone.removeAttribute('id');
      clone.removeAttribute('data-deck-active');
      clone.querySelectorAll('[id]').forEach(el => el.removeAttribute('id'));
      // Neuter heavy media; replace <video> with its poster so the box
      // keeps a visual. <iframe>/<audio> become empty placeholders.
      clone.querySelectorAll('iframe, audio, object, embed').forEach(el => {
        el.removeAttribute('src');
        el.removeAttribute('srcdoc');
        el.removeAttribute('data');
        el.innerHTML = '';
      });
      clone.querySelectorAll('video').forEach(el => {
        if (!el.poster) {
          el.removeAttribute('src');
          el.innerHTML = '';
          return;
        }
        const img = document.createElement('img');
        img.src = el.poster;
        img.alt = '';
        img.style.cssText = el.style.cssText + ';object-fit:cover;width:100%;height:100%;';
        img.className = el.className;
        el.replaceWith(img);
      });
      // Images: defer decode and let the browser pick the smallest
      // srcset candidate for the ~140px thumb. Same-URL clones reuse the
      // slide's decoded bitmap (URL-keyed cache), so the remaining cost
      // is paint/composite — lazy+async keeps that off the main thread.
      clone.querySelectorAll('img').forEach(el => {
        el.loading = 'lazy';
        el.decoding = 'async';
        if (el.srcset) el.sizes = (this._railPx || 188) + 'px';
      });
      // Custom elements inside the slide would have their
      // connectedCallback fire when the clone is appended. Replace them
      // with inert boxes so a component-heavy deck doesn't run N copies
      // of each component's mount logic in the rail. Children are
      // preserved so layout-wrapper elements (<my-column><h2>…</h2>)
      // still show their authored content; the querySelectorAll NodeList
      // is static, so nested custom elements in the moved subtree are
      // still visited on later iterations.
      const neuter = el => {
        const box = document.createElement('div');
        box.style.cssText = (el.getAttribute('style') || '') + ';background:rgba(0,0,0,0.06);border:1px dashed rgba(0,0,0,0.15);';
        box.className = el.className;
        // Preserve theming/i18n hooks so [data-*] / :lang() / [dir]
        // descendant selectors still match the neutered root.
        for (const a of el.attributes) {
          const n = a.name;
          if (n.startsWith('data-') || n.startsWith('aria-') || n === 'lang' || n === 'dir' || n === 'role' || n === 'title') {
            box.setAttribute(n, a.value);
          }
        }
        while (el.firstChild) box.appendChild(el.firstChild);
        return box;
      };
      // querySelectorAll('*') returns descendants only — a custom-element
      // slide root (<my-slide>…</my-slide>) would slip through and upgrade
      // on append. Swap the root first.
      if (clone.tagName.includes('-')) clone = neuter(clone);
      clone.querySelectorAll('*').forEach(el => {
        if (el.tagName.includes('-')) el.replaceWith(neuter(el));
      });
      clone.style.cssText += ';position:absolute;top:0;left:0;transform-origin:0 0;' + 'pointer-events:none;width:' + dw + 'px;height:' + dh + 'px;' + 'box-sizing:border-box;overflow:hidden;visibility:visible;opacity:1;';
      const host = document.createElement('div');
      host.style.cssText = 'position:absolute;inset:0;';
      this._syncThumbHostAttrs(host);
      const sr = host.attachShadow({
        mode: 'open'
      });
      if (this._adoptedSheet) sr.adoptedStyleSheets = [this._adoptedSheet];else {
        const st = document.createElement('style');
        st.textContent = this._authorCss || '';
        sr.appendChild(st);
      }
      sr.appendChild(clone);
      entry.frame.appendChild(host);
      entry.host = host;
      entry.clone = clone;
      if (this._thumbScale) clone.style.transform = 'scale(' + this._thumbScale + ')';
      // Once materialized the IO callback is a no-op early-return —
      // unobserve so scroll doesn't keep firing it.
      if (this._railObserver) this._railObserver.unobserve(entry.frame);
    }

    /** Re-clone a single thumb (live-update path). No-op if the thumb
     *  hasn't been materialized yet — it'll pick up current content when
     *  it scrolls into view. */
    _refreshThumb(slide) {
      const entry = (this._thumbs || []).find(t => t.slide === slide);
      if (!entry || !entry.host) return;
      entry.host.remove();
      entry.host = entry.clone = null;
      this._materialize(entry);
    }
    _scaleThumbs() {
      if (!this._thumbs || !this._thumbs.length) return;
      // Every frame is the same width; if it reads 0 the rail is
      // display:none (noscale / no-rail / presenting / print) — leave the
      // clones as-is and re-run when the rail is revealed.
      const fw = this._thumbs[0].frame.offsetWidth;
      if (!fw) return;
      this._thumbScale = fw / this.designWidth;
      this._thumbs.forEach(({
        clone
      }) => {
        if (clone) clone.style.transform = 'scale(' + this._thumbScale + ')';
      });
    }
    _setDrop(i, where) {
      // dragover fires at pointer-event rate; touch only the previous
      // and new target rather than sweeping all N thumbs.
      const t = this._thumbs && this._thumbs[i];
      if (this._dropOn && this._dropOn !== t) {
        this._dropOn.thumb.removeAttribute('data-drop');
      }
      if (t) t.thumb.setAttribute('data-drop', where);
      this._dropOn = t || null;
    }
    _clearDrop() {
      if (this._dropOn) this._dropOn.thumb.removeAttribute('data-drop');
      this._dropOn = null;
    }
    _syncRail(follow) {
      if (!this._thumbs) return;
      this._thumbs.forEach(({
        thumb
      }, i) => {
        if (i === this._index) {
          thumb.setAttribute('data-current', '');
          if (follow && typeof thumb.scrollIntoView === 'function') {
            thumb.scrollIntoView({
              block: 'nearest'
            });
          }
        } else {
          thumb.removeAttribute('data-current');
        }
      });
    }
    _openMenu(i, x, y) {
      if (!this._menu) return;
      this._menuIndex = i;
      const slide = this._slides[i];
      const skip = slide && slide.hasAttribute('data-deck-skip');
      this._menu.querySelector('[data-act="skip"]').textContent = skip ? 'Unskip slide' : 'Skip slide';
      this._menu.querySelector('[data-act="up"]').disabled = i <= 0;
      this._menu.querySelector('[data-act="down"]').disabled = i >= this._slides.length - 1;
      this._menu.querySelector('[data-act="delete"]').disabled = this._slides.length <= 1;
      // Place, then clamp to viewport after it's measurable.
      this._menu.style.left = x + 'px';
      this._menu.style.top = y + 'px';
      this._menu.setAttribute('data-open', '');
      const r = this._menu.getBoundingClientRect();
      const nx = Math.min(x, window.innerWidth - r.width - 4);
      const ny = Math.min(y, window.innerHeight - r.height - 4);
      this._menu.style.left = Math.max(4, nx) + 'px';
      this._menu.style.top = Math.max(4, ny) + 'px';
    }
    _closeMenu() {
      if (this._menu) this._menu.removeAttribute('data-open');
      this._menuIndex = -1;
    }
    _openConfirm(i) {
      if (!this._confirm) return;
      this._confirmIndex = i;
      this._confirm.querySelector('.title').textContent = 'Delete slide ' + (i + 1) + '?';
      this._confirm.setAttribute('data-open', '');
      const btn = this._confirm.querySelector('.danger');
      if (btn && btn.focus) btn.focus();
    }
    _closeConfirm() {
      if (this._confirm) this._confirm.removeAttribute('data-open');
      this._confirmIndex = -1;
    }
    _emitDeckChange(detail) {
      this.dispatchEvent(new CustomEvent('deckchange', {
        detail,
        bubbles: true,
        composed: true
      }));
    }
    _deleteSlide(i) {
      const slide = this._slides[i];
      if (!slide || this._slides.length <= 1) return;
      const wasCurrent = i === this._index;
      if (i < this._index || wasCurrent && i === this._slides.length - 1) this._index--;
      this._squelchSlotChange = true;
      slide.remove();
      this._emitDeckChange({
        action: 'delete',
        from: i,
        slide
      });
      this._collectSlides();
      this._applyIndex({
        showOverlay: true,
        broadcast: true,
        reason: 'mutation'
      });
    }
    _toggleSkip(i) {
      const slide = this._slides[i];
      if (!slide) return;
      const on = !slide.hasAttribute('data-deck-skip');
      if (on) slide.setAttribute('data-deck-skip', '');else slide.removeAttribute('data-deck-skip');
      if (this._thumbs && this._thumbs[i]) {
        if (on) this._thumbs[i].thumb.setAttribute('data-skip', '');else this._thumbs[i].thumb.removeAttribute('data-skip');
      }
      this._markLastVisible();
      this._emitDeckChange({
        action: on ? 'skip' : 'unskip',
        from: i,
        slide
      });
      // Re-broadcast so the presenter popup's prev/next thumbnails re-pick
      // the nearest non-skipped slide without waiting for a nav event.
      try {
        window.postMessage({
          slideIndexChanged: this._index,
          deckTotal: this._slides.length,
          deckSkipped: this._skippedIndices()
        }, '*');
      } catch (e) {}
    }
    _skippedIndices() {
      const out = [];
      for (let i = 0; i < this._slides.length; i++) {
        if (this._slides[i].hasAttribute('data-deck-skip')) out.push(i);
      }
      return out;
    }
    _moveSlide(i, j) {
      if (j < 0 || j >= this._slides.length || j === i) return;
      const slide = this._slides[i];
      const ref = j < i ? this._slides[j] : this._slides[j].nextSibling;
      // Track the active slide across the reorder so the same content
      // stays on screen.
      const cur = this._index;
      if (cur === i) this._index = j;else if (i < cur && j >= cur) this._index = cur - 1;else if (i > cur && j <= cur) this._index = cur + 1;
      this._squelchSlotChange = true;
      this.insertBefore(slide, ref);
      this._emitDeckChange({
        action: 'move',
        from: i,
        to: j,
        slide
      });
      this._collectSlides();
      this._applyIndex({
        showOverlay: false,
        broadcast: true,
        reason: 'mutation'
      });
    }

    // Public API ------------------------------------------------------------

    /** Current slide index (0-based). */
    get index() {
      return this._index;
    }
    /** Total slide count. */
    get length() {
      return this._slides.length;
    }
    /** Programmatically navigate. */
    goTo(i) {
      this._go(i, 'api');
    }
    next() {
      this._advance(1, 'api');
    }
    prev() {
      this._advance(-1, 'api');
    }
    reset() {
      this._go(0, 'api');
    }
  }
  if (!customElements.get('deck-stage')) {
    customElements.define('deck-stage', DeckStage);
  }
})();
})(); } catch (e) { __ds_ns.__errors.push({ path: "pitch_deck/deck-stage.js", error: String((e && e.message) || e) }); }

// ui_kits/console/App.jsx
try { (() => {
const {
  useState,
  useEffect
} = React;
function App() {
  // Hash router: '', '#ledger', '#filing/CTR-XYZ', '#auth'
  const [hash, setHash] = useState(window.location.hash || "#auth");
  useEffect(() => {
    const onHash = () => setHash(window.location.hash || "#auth");
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  const [justSignedId, setJustSignedId] = useState(null);
  const go = h => {
    window.location.hash = h;
  };
  if (hash === "#auth" || hash === "") {
    return /*#__PURE__*/React.createElement(AuthScreen, {
      onSignIn: () => go("#dashboard")
    });
  }
  let route = "dashboard";
  let filingId = null;
  if (hash.startsWith("#filing/")) {
    route = "filing";
    filingId = hash.slice("#filing/".length);
  } else if (hash === "#ledger") route = "ledger";else if (hash === "#dashboard") route = "dashboard";else if (hash === "#witnesses" || hash === "#custodians" || hash === "#audit" || hash === "#settings") route = hash.slice(1);
  return /*#__PURE__*/React.createElement("div", {
    className: "app"
  }, /*#__PURE__*/React.createElement(Sidebar, {
    route: route === "filing" ? "ledger" : route,
    onRoute: id => go("#" + id)
  }), /*#__PURE__*/React.createElement("main", {
    style: {
      overflow: "auto"
    }
  }, route === "dashboard" && /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(TopBar, {
    breadcrumbs: ["defense_prime · Procurement #14", "Dashboard"],
    eyebrow: "17 May 2026 \xB7 14:02 UTC",
    title: "Good afternoon, Aleia.",
    actions: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("button", {
      className: "btn btn-ghost"
    }, "Export"), /*#__PURE__*/React.createElement("button", {
      className: "btn btn-primary"
    }, /*#__PURE__*/React.createElement(Icon, {
      name: "plus",
      size: 13
    }), "New filing"))
  }), /*#__PURE__*/React.createElement(Dashboard, {
    onOpen: id => id === "__list__" ? go("#ledger") : go("#filing/" + id)
  })), route === "ledger" && /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(TopBar, {
    breadcrumbs: ["defense_prime · Procurement #14", "Ledger"],
    eyebrow: "312 filings on record \xB7 live",
    title: "Ledger",
    actions: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("button", {
      className: "btn btn-ghost"
    }, /*#__PURE__*/React.createElement(Icon, {
      name: "download",
      size: 13
    }), "Export ledger"), /*#__PURE__*/React.createElement("button", {
      className: "btn btn-primary"
    }, /*#__PURE__*/React.createElement(Icon, {
      name: "plus",
      size: 13
    }), "New filing"))
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "28px 32px 64px"
    }
  }, /*#__PURE__*/React.createElement(LedgerTable, {
    onOpen: id => go("#filing/" + id),
    justSignedId: justSignedId
  }))), route === "filing" && /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(TopBar, {
    breadcrumbs: ["defense_prime · Procurement #14", "Ledger", filingId],
    eyebrow: "Filing detail",
    title: filingId,
    actions: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("button", {
      className: "btn btn-ghost",
      onClick: () => go("#ledger")
    }, "\u2190 Ledger"))
  }), /*#__PURE__*/React.createElement(FilingDetail, {
    id: filingId,
    onClose: () => go("#ledger"),
    onSign: id => {
      setJustSignedId(id);
      go("#ledger");
      setTimeout(() => setJustSignedId(null), 1800);
    }
  })), (route === "witnesses" || route === "custodians" || route === "audit" || route === "settings") && /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(TopBar, {
    breadcrumbs: ["defense_prime · Procurement #14", route[0].toUpperCase() + route.slice(1)],
    eyebrow: "Section",
    title: route[0].toUpperCase() + route.slice(1)
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "28px 32px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "surface",
    style: {
      padding: "48px",
      textAlign: "center"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 12
    }
  }, "Placeholder"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 24,
      color: "var(--ink)",
      fontVariationSettings: '"opsz" 24'
    }
  }, "This surface is left intentionally blank."), /*#__PURE__*/React.createElement("p", {
    style: {
      color: "var(--fg-2)",
      marginTop: 12,
      fontFamily: "var(--font-sans)"
    }
  }, "The kit demonstrates the core lifecycle (auth \u2192 dashboard \u2192 ledger \u2192 filing). Other sections will be built once the canonical screens exist in the product."))))));
}
ReactDOM.createRoot(document.getElementById("root")).render(/*#__PURE__*/React.createElement(App, null));
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/App.jsx", error: String((e && e.message) || e) }); }

// ui_kits/console/AuthScreen.jsx
try { (() => {
function AuthScreen({
  onSignIn
}) {
  const [email, setEmail] = React.useState("aleia.rouhani@defense_prime.com");
  return /*#__PURE__*/React.createElement("div", {
    "data-screen-label": "Auth",
    style: {
      minHeight: "100vh",
      background: "var(--paper)",
      display: "grid",
      gridTemplateColumns: "1fr 1.1fr"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      justifyContent: "space-between",
      padding: "40px 64px"
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "assets/citrate_marquee_black.svg",
    alt: "Citrate",
    style: {
      height: 22,
      alignSelf: "flex-start"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 420,
      margin: "auto 0"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 18
    }
  }, "Sign in"), /*#__PURE__*/React.createElement("h1", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 60',
      fontWeight: 360,
      fontSize: 56,
      lineHeight: 1.04,
      letterSpacing: "-0.022em",
      margin: "0 0 14px",
      color: "var(--ink)"
    }
  }, "The instrument of record."), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 17,
      lineHeight: 1.55,
      color: "var(--fg-2)",
      margin: 0,
      fontVariationSettings: '"opsz" 17'
    }
  }, "Sign in with your tenant's custodian. Access requires a witnessed identity from a Citrate-approved attester."), /*#__PURE__*/React.createElement("form", {
    style: {
      marginTop: 36,
      display: "flex",
      flexDirection: "column",
      gap: 16
    },
    onSubmit: e => {
      e.preventDefault();
      onSignIn();
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("label", {
    className: "lbl"
  }, "Work email"), /*#__PURE__*/React.createElement("input", {
    className: "input",
    value: email,
    onChange: e => setEmail(e.target.value),
    placeholder: "you@tenant.com"
  })), /*#__PURE__*/React.createElement("button", {
    type: "submit",
    className: "btn btn-primary btn-lg",
    style: {
      justifyContent: "center"
    }
  }, "Continue with custodian SSO")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 14,
      margin: "28px 0"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      height: 1,
      background: "var(--stone-200)"
    }
  }), /*#__PURE__*/React.createElement("span", {
    className: "eyebrow"
  }, "or"), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      height: 1,
      background: "var(--stone-200)"
    }
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 8
    }
  }, [{
    name: "Kestrel SSO",
    j: "Federal · State"
  }, {
    name: "Aria Custodian",
    j: "EU · UK"
  }, {
    name: "Beacon Substrate",
    j: "APAC · JP-GOV"
  }, {
    name: "Government PIV",
    j: "US · DOD"
  }].map(p => /*#__PURE__*/React.createElement("button", {
    key: p.name,
    className: "btn btn-ghost",
    style: {
      height: 44,
      padding: "0 16px",
      justifyContent: "space-between",
      width: "100%"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "custody",
    size: 16
  }), /*#__PURE__*/React.createElement("span", null, p.name)), /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 11,
      color: "var(--fg-3)"
    }
  }, p.j))))), /*#__PURE__*/React.createElement("div", {
    className: "mono",
    style: {
      fontSize: 11,
      color: "var(--fg-3)",
      letterSpacing: ".06em"
    }
  }, "Continuously attested by Kestrel Advisory \xB7 EY \xB7 Beacon \xB7 99.997 % uptime \xB7 30 days")), /*#__PURE__*/React.createElement("div", {
    style: {
      background: "var(--deep-evergreen)",
      color: "#cde7d6",
      padding: "40px 64px",
      display: "flex",
      flexDirection: "column",
      justifyContent: "space-between",
      position: "relative",
      overflow: "hidden"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      color: "var(--citrate-green)"
    }
  }, "Citrate Network \xB7 live in 87 jurisdictions"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      alignItems: "flex-start"
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "assets/citrate_mark_white.svg",
    style: {
      width: 220,
      marginBottom: 36
    }
  }), /*#__PURE__*/React.createElement("blockquote", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 28,
      lineHeight: 1.3,
      color: "var(--paper)",
      margin: 0,
      fontVariationSettings: '"opsz" 28',
      fontWeight: 380,
      maxWidth: 480
    }
  }, "\"We replaced six procurement audit systems with Citrate's substrate. Our auditors now read the same record our operators write.\""), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 24,
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.6)"
    }
  }, "CFO, top-5 defence prime \xB7 2026")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "repeat(3, 1fr)",
      gap: 18,
      paddingTop: 32,
      borderTop: "1px solid rgba(205,231,214,0.15)"
    }
  }, /*#__PURE__*/React.createElement(Stat2, {
    v: "87",
    l: "Jurisdictions"
  }), /*#__PURE__*/React.createElement(Stat2, {
    v: "$ 2.4T",
    l: "Routed YTD"
  }), /*#__PURE__*/React.createElement(Stat2, {
    v: "99.997%",
    l: "Attestation"
  }))));
}
function Stat2({
  v,
  l
}) {
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 28,
      color: "var(--paper)",
      fontVariationSettings: '"opsz" 28',
      fontFeatureSettings: '"tnum","lnum"'
    }
  }, v), /*#__PURE__*/React.createElement("div", {
    className: "mono",
    style: {
      fontSize: 10,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.55)",
      marginTop: 4
    }
  }, l));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/AuthScreen.jsx", error: String((e && e.message) || e) }); }

// ui_kits/console/Dashboard.jsx
try { (() => {
function Dashboard({
  onOpen
}) {
  const recent = FILINGS.slice(0, 4);
  return /*#__PURE__*/React.createElement("div", {
    "data-screen-label": "Dashboard",
    style: {
      padding: "28px 32px 64px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "repeat(4, 1fr)",
      gap: 16,
      marginBottom: 28
    }
  }, /*#__PURE__*/React.createElement(StatCard, {
    label: "Notional this quarter",
    value: "$ 21.4M",
    delta: "+12.4 %",
    suffix: "vs Q1"
  }), /*#__PURE__*/React.createElement(StatCard, {
    label: "Filings on record",
    value: "312",
    delta: "+18",
    suffix: "this week"
  }), /*#__PURE__*/React.createElement(StatCard, {
    label: "Pending witness",
    value: "4",
    delta: "\u22122",
    suffix: "vs yesterday",
    caution: true
  }), /*#__PURE__*/React.createElement(StatCard, {
    label: "Attestation uptime",
    value: "99.997 %",
    delta: "0.000",
    suffix: "30 days"
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1.5fr 1fr",
      gap: 16
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "surface",
    style: {
      padding: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "16px 20px",
      borderBottom: "1px solid var(--stone-200)",
      display: "flex",
      justifyContent: "space-between",
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, "Open filings"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 20,
      fontWeight: 440,
      color: "var(--ink)",
      marginTop: 4,
      fontVariationSettings: '"opsz" 20'
    }
  }, "Requires your attention")), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-sm",
    onClick: () => onOpen("__list__")
  }, "Open ledger \u2192")), /*#__PURE__*/React.createElement("div", null, recent.map((f, i) => /*#__PURE__*/React.createElement("div", {
    key: f.id,
    onClick: () => onOpen(f.id),
    className: "filing-row",
    style: {
      display: "grid",
      gridTemplateColumns: "auto 1fr auto auto",
      gap: 16,
      alignItems: "center",
      padding: "16px 20px",
      borderBottom: i === recent.length - 1 ? "none" : "1px solid var(--stone-150)",
      cursor: "pointer"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 12,
      color: "var(--ink)"
    }
  }, f.id), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 14,
      color: "var(--ink)"
    }
  }, f.type, " ", /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--fg-3)"
    }
  }, "\xB7 ", f.reg)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      color: "var(--fg-2)",
      marginTop: 2
    }
  }, f.cp)), /*#__PURE__*/React.createElement("span", {
    className: "mono tabular",
    style: {
      fontSize: 13
    }
  }, money(f.amt)), /*#__PURE__*/React.createElement(Pill, {
    status: f.status
  }, f.status === "settled" ? "Settled" : f.status === "pending" ? "Pending" : "Blocked"))))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 16
    }
  }, /*#__PURE__*/React.createElement(AttestationPanel, null), /*#__PURE__*/React.createElement(JurisdictionPanel, null))));
}
function StatCard({
  label,
  value,
  delta,
  suffix,
  caution
}) {
  const positive = !caution && !String(delta).startsWith("−") && !String(delta).startsWith("-");
  return /*#__PURE__*/React.createElement("div", {
    className: "surface",
    style: {
      padding: 18
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, label), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 36',
      fontSize: 32,
      fontWeight: 440,
      color: "var(--ink)",
      marginTop: 10,
      letterSpacing: "-0.012em",
      fontFeatureSettings: '"tnum","lnum"',
      whiteSpace: "nowrap"
    }
  }, value), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "baseline",
      gap: 6,
      marginTop: 6,
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      whiteSpace: "nowrap",
      color: positive ? "var(--citrate-green-deep)" : caution ? "var(--semantic-warning)" : "var(--fg-2)"
    }
  }, /*#__PURE__*/React.createElement("span", null, delta), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--fg-3)"
    }
  }, suffix)));
}
function AttestationPanel() {
  return /*#__PURE__*/React.createElement("div", {
    className: "surface",
    style: {
      padding: 20
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, "Continuous attestation"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "baseline",
      gap: 8,
      marginTop: 12
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 8,
      height: 8,
      borderRadius: "50%",
      background: "var(--citrate-green-deep)",
      animation: "pulseDot 1.6s var(--ease-standard) infinite"
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 20,
      fontVariationSettings: '"opsz" 20',
      color: "var(--ink)"
    }
  }, "3 auditors witnessing")), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 16,
      display: "flex",
      flexDirection: "column",
      gap: 10
    }
  }, [{
    name: "Kestrel Advisory LLP",
    ago: "12 s",
    ok: true
  }, {
    name: "Aria Compliance",
    ago: "44 s",
    ok: true
  }, {
    name: "Beacon Substrate",
    ago: "3 min",
    ok: true
  }].map(a => /*#__PURE__*/React.createElement("div", {
    key: a.name,
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8,
      fontSize: 13
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "check",
    size: 14
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1,
      color: "var(--ink)"
    }
  }, a.name), /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 11,
      color: "var(--fg-3)"
    }
  }, a.ago, " ago")))));
}
function JurisdictionPanel() {
  return /*#__PURE__*/React.createElement("div", {
    className: "surface",
    style: {
      padding: 20
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, "Jurisdictions in play"), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 14,
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      rowGap: 12,
      columnGap: 14
    }
  }, [{
    j: "US-FED",
    n: 142
  }, {
    j: "US-DE",
    n: 38
  }, {
    j: "JP-GOV",
    n: 22
  }, {
    j: "EU",
    n: 18
  }, {
    j: "SA",
    n: 7
  }, {
    j: "+ 82 more",
    n: null
  }].map(r => /*#__PURE__*/React.createElement("div", {
    key: r.j,
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline",
      paddingBottom: 8,
      borderBottom: "1px solid var(--stone-150)"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 12
    }
  }, r.j), r.n != null && /*#__PURE__*/React.createElement("span", {
    className: "mono tabular",
    style: {
      fontSize: 13,
      color: "var(--ink)"
    }
  }, r.n)))));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/Dashboard.jsx", error: String((e && e.message) || e) }); }

// ui_kits/console/FilingDetail.jsx
try { (() => {
function FilingDetail({
  id,
  onClose,
  onSign
}) {
  const f = FILINGS.find(x => x.id === id) || FILINGS[0];
  const lineage = [{
    ts: "2026-05-15 09:14 UTC",
    who: "Aleia Rouhani",
    what: "Filed",
    hash: "0x8e…a019",
    custodian: false
  }, {
    ts: "2026-05-15 09:14 UTC",
    who: "Kestrel Advisory LLP",
    what: "Witness 1 of 3",
    hash: "0xcc…7b21",
    custodian: true
  }, {
    ts: "2026-05-16 11:02 UTC",
    who: "Aria Compliance",
    what: "Witness 2 of 3",
    hash: "0x09…4f02",
    custodian: true
  }, {
    ts: f.status === "pending" ? null : "2026-05-17 14:02 UTC",
    who: "Beacon Substrate",
    what: f.status === "pending" ? "Awaiting witness 3 of 3" : "Witness 3 of 3",
    hash: f.status === "pending" ? null : "0x4f…12cc",
    custodian: true,
    pending: f.status === "pending"
  }];
  return /*#__PURE__*/React.createElement("div", {
    "data-screen-label": "FilingDetail",
    style: {
      padding: "28px 32px 64px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1.4fr 1fr",
      gap: 24
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "surface",
    style: {
      padding: 28
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      paddingBottom: 16,
      borderBottom: "1px solid var(--stone-200)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, "Filing \xB7 ", f.id, " \xB7 ", f.j), /*#__PURE__*/React.createElement(Pill, {
    status: f.status
  }, f.status === "settled" ? "Settled · on record" : f.status === "pending" ? "Witness pending" : "Blocked")), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-display)",
      fontWeight: 440,
      fontVariationSettings: '"opsz" 36',
      fontSize: 36,
      lineHeight: 1.1,
      margin: "22px 0 8px",
      letterSpacing: "-0.012em",
      color: "var(--ink)"
    }
  }, f.type), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 14,
      color: "var(--fg-2)"
    }
  }, "Filed under ", /*#__PURE__*/React.createElement("span", {
    className: "mono"
  }, f.reg), " by Aleia Rouhani \xB7 effective ", f.effective), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      gap: 20,
      marginTop: 28
    }
  }, /*#__PURE__*/React.createElement(KV, {
    k: "Counterparty",
    v: f.cp
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Witness custodian",
    v: f.custodian
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Notional",
    v: money(f.amt),
    mono: true
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Witnesses",
    v: f.w,
    mono: true
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Effective",
    v: f.effective,
    mono: true
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Jurisdiction",
    v: f.j,
    mono: true
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 28,
      padding: 24,
      background: "var(--paper-pure)",
      border: "1px solid var(--stone-200)",
      borderRadius: "var(--r-1)",
      borderLeft: "3px solid var(--ink)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 12
    }
  }, "Part I \u2014 Declaration"), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 16,
      lineHeight: 1.65,
      color: "var(--ink)",
      margin: 0,
      fontVariationSettings: '"opsz" 16'
    }
  }, "The party named above (\"Counterparty\"), acting on its own authority and within the jurisdiction identified, hereby files this instrument for continuous attestation through the Citrate substrate, and represents that all information contained herein is accurate as of the effective date. Witness custodians named in ", /*#__PURE__*/React.createElement("span", {
    className: "mono"
  }, "Part II"), " may countersign for the duration of this filing's lifecycle.")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 10,
      marginTop: 28,
      paddingTop: 24,
      borderTop: "1px solid var(--stone-200)"
    }
  }, f.status === "pending" ? /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("button", {
    className: "btn btn-primary btn-lg",
    onClick: () => onSign(f.id)
  }, "Sign & settle"), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-lg"
  }, "Notify custodian")) : f.status === "blocked" ? /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("button", {
    className: "btn btn-secondary btn-lg"
  }, "Re-route through alternative custodian"), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-lg"
  }, "View blocker")) : /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("button", {
    className: "btn btn-secondary btn-lg"
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "download",
    size: 14
  }), "Download settled instrument"), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-lg"
  }, "View on public record \u2192")))), /*#__PURE__*/React.createElement("div", {
    className: "surface",
    style: {
      padding: 24
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, "Witness lineage"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 20,
      fontWeight: 440,
      color: "var(--ink)",
      marginTop: 4,
      fontVariationSettings: '"opsz" 20'
    }
  }, "Continuously attested"), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 18,
      position: "relative"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      left: 9,
      top: 8,
      bottom: 8,
      width: 1,
      background: "var(--stone-200)"
    }
  }), lineage.map((l, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      display: "grid",
      gridTemplateColumns: "20px 1fr",
      gap: 14,
      paddingBottom: 18
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 18,
      height: 18,
      borderRadius: "50%",
      background: l.pending ? "var(--paper-pure)" : "var(--citrate-green)",
      border: l.pending ? "1.5px dashed var(--semantic-warning)" : "1.5px solid var(--citrate-green-deep)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      position: "relative",
      zIndex: 1
    }
  }, !l.pending && /*#__PURE__*/React.createElement(Icon, {
    name: "check",
    size: 10
  })), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      fontWeight: 500,
      color: l.pending ? "var(--fg-2)" : "var(--ink)"
    }
  }, l.who), l.ts && /*#__PURE__*/React.createElement("div", {
    className: "mono",
    style: {
      fontSize: 10,
      color: "var(--fg-3)",
      letterSpacing: ".06em"
    }
  }, l.ts)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      color: "var(--fg-2)",
      marginTop: 2
    }
  }, l.what), l.hash && /*#__PURE__*/React.createElement("div", {
    className: "mono",
    style: {
      fontSize: 11,
      color: "var(--fg-3)",
      marginTop: 4
    }
  }, l.hash))))), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 10,
      paddingTop: 18,
      borderTop: "1px solid var(--stone-200)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 10
    }
  }, "Continuous record"), /*#__PURE__*/React.createElement("div", {
    className: "mono",
    style: {
      fontSize: 11,
      color: "var(--fg-2)",
      lineHeight: 1.7
    }
  }, "Last attestation \xB7 14:02 UTC", /*#__PURE__*/React.createElement("br", null), "Cadence \xB7 every 6 s", /*#__PURE__*/React.createElement("br", null), "Public ledger \xB7 ctr.network/", f.id.toLowerCase())))), /*#__PURE__*/React.createElement("button", {
    onClick: onClose,
    className: "btn btn-ghost btn-sm",
    style: {
      marginTop: 18
    }
  }, "\u2190 Back to ledger"));
}
function KV({
  k,
  v,
  mono
}) {
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      fontSize: 10
    }
  }, k), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
      fontSize: mono ? 14 : 15,
      fontWeight: 500,
      color: "var(--ink)",
      marginTop: 6,
      fontFeatureSettings: '"tnum","lnum"'
    }
  }, v));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/FilingDetail.jsx", error: String((e && e.message) || e) }); }

// ui_kits/console/LedgerTable.jsx
try { (() => {
// Mock data shared across screens
const FILINGS = [{
  id: "CTR-9F4A",
  type: "Supply contract",
  reg: "FAR 52.246",
  cp: "defense_prime Procurement #14",
  j: "US-FED",
  amt: 2418000,
  w: "3 of 3",
  custodian: "Kestrel Advisory",
  status: "settled",
  effective: "2026-05-17"
}, {
  id: "CTR-8B12",
  type: "Hazmat manifest",
  reg: "DOT 173",
  cp: "Meridian Chemical, Inc.",
  j: "US-DE",
  amt: 1084200,
  w: "2 of 3",
  custodian: "EY",
  status: "pending",
  effective: "2026-05-15"
}, {
  id: "CTR-7AE0",
  type: "Routing instrument",
  reg: "WTO TBT",
  cp: "Continental Trading America",
  j: "JP/US",
  amt: 94500,
  w: "4 of 4",
  custodian: "Beacon",
  status: "settled",
  effective: "2026-05-14"
}, {
  id: "CTR-6D8C",
  type: "Procurement exemption",
  reg: "METI 38",
  cp: "METI Jurisdiction 38",
  j: "JP-GOV",
  amt: 312000,
  w: "0 of 2",
  custodian: "—",
  status: "blocked",
  effective: "2026-05-12"
}, {
  id: "CTR-5C71",
  type: "Defense supply",
  reg: "DFARS 252",
  cp: "Vanguard Systems VS-12",
  j: "US-FED",
  amt: 8920000,
  w: "3 of 3",
  custodian: "Kestrel Advisory",
  status: "settled",
  effective: "2026-05-11"
}, {
  id: "CTR-4B02",
  type: "Energy contract",
  reg: "FERC 35.13",
  cp: "Gulf Energy Services Co.",
  j: "SA/US",
  amt: 415000,
  w: "1 of 3",
  custodian: "PwC pending",
  status: "pending",
  effective: "2026-05-10"
}, {
  id: "CTR-3A99",
  type: "Aerospace MRO",
  reg: "FAA 145",
  cp: "Sentinel Technologies",
  j: "US-FED",
  amt: 1750000,
  w: "3 of 3",
  custodian: "Kestrel Advisory",
  status: "settled",
  effective: "2026-05-09"
}, {
  id: "CTR-2F44",
  type: "EU procurement",
  reg: "EU 2014/24",
  cp: "Aerostar Defence & Space",
  j: "EU",
  amt: 6300000,
  w: "2 of 4",
  custodian: "Mazars",
  status: "pending",
  effective: "2026-05-08"
}];
function money(n) {
  return "$ " + n.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  });
}
function LedgerTable({
  onOpen,
  justSignedId
}) {
  return /*#__PURE__*/React.createElement("div", {
    "data-screen-label": "LedgerTable",
    className: "surface",
    style: {
      overflow: "hidden"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8,
      padding: "12px 16px",
      borderBottom: "1px solid var(--stone-200)",
      background: "var(--paper-2)"
    }
  }, /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-sm"
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "filter",
    size: 13
  }), "All filings"), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-sm"
  }, "Status \xB7 Any"), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-sm"
  }, "Jurisdiction \xB7 5"), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-sm"
  }, "Custodian \xB7 Any"), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1
    }
  }), /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 12,
      color: "var(--fg-3)"
    }
  }, FILINGS.length, " filings"), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-sm"
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "download",
    size: 13
  }), "Export")), /*#__PURE__*/React.createElement("table", {
    style: {
      width: "100%",
      borderCollapse: "collapse",
      fontFamily: "var(--font-sans)",
      fontSize: 13
    }
  }, /*#__PURE__*/React.createElement("thead", null, /*#__PURE__*/React.createElement("tr", {
    style: {
      background: "var(--paper-2)"
    }
  }, ["Filing", "Type", "Counterparty", "Jurisdiction", "Notional", "Witnesses", "Status", ""].map((h, i) => /*#__PURE__*/React.createElement("th", {
    key: i,
    style: {
      fontFamily: "var(--font-mono)",
      fontWeight: 500,
      fontSize: 10,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "var(--fg-3)",
      textAlign: i === 4 ? "right" : "left",
      padding: "10px 16px",
      borderBottom: "1px solid var(--ink)"
    }
  }, h)))), /*#__PURE__*/React.createElement("tbody", null, FILINGS.map(f => /*#__PURE__*/React.createElement("tr", {
    key: f.id,
    onClick: () => onOpen(f.id),
    className: "filing-row" + (justSignedId === f.id ? " just-signed" : ""),
    style: {
      cursor: "pointer"
    }
  }, /*#__PURE__*/React.createElement("td", {
    style: td
  }, /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 12,
      color: "var(--ink)"
    }
  }, f.id)), /*#__PURE__*/React.createElement("td", {
    style: td
  }, /*#__PURE__*/React.createElement("div", null, f.type), /*#__PURE__*/React.createElement("div", {
    className: "mono",
    style: {
      fontSize: 11,
      color: "var(--fg-3)",
      letterSpacing: ".06em"
    }
  }, f.reg)), /*#__PURE__*/React.createElement("td", {
    style: td
  }, f.cp), /*#__PURE__*/React.createElement("td", {
    style: td
  }, /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 12,
      color: "var(--fg-2)"
    }
  }, f.j)), /*#__PURE__*/React.createElement("td", {
    style: {
      ...td,
      textAlign: "right"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "mono tabular",
    style: {
      fontSize: 13
    }
  }, money(f.amt))), /*#__PURE__*/React.createElement("td", {
    style: td
  }, /*#__PURE__*/React.createElement("span", {
    className: "mono",
    style: {
      fontSize: 12,
      color: "var(--fg-2)"
    }
  }, f.w)), /*#__PURE__*/React.createElement("td", {
    style: td
  }, /*#__PURE__*/React.createElement(Pill, {
    status: f.status
  }, f.status === "settled" ? "Settled" : f.status === "pending" ? "Pending" : "Blocked")), /*#__PURE__*/React.createElement("td", {
    style: {
      ...td,
      color: "var(--fg-3)"
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "chevright",
    size: 14
  })))))));
}
const td = {
  padding: "13px 16px",
  borderBottom: "1px solid var(--stone-150)",
  verticalAlign: "baseline",
  color: "var(--ink)"
};
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/LedgerTable.jsx", error: String((e && e.message) || e) }); }

// ui_kits/console/Primitives.jsx
try { (() => {
// Primitives — Pill, Button, Field, Icon

function Pill({
  status,
  children
}) {
  const styles = {
    settled: {
      bg: "#ecf5d4",
      bd: "#4f8a05",
      fg: "#3a6500"
    },
    pending: {
      bg: "#fff1c4",
      bd: "#b07b00",
      fg: "#6e5100"
    },
    blocked: {
      bg: "#f6e1de",
      bd: "#a72414",
      fg: "#7a1a10"
    },
    notice: {
      bg: "#dbe7ef",
      bd: "#1b4965",
      fg: "#143548"
    },
    draft: {
      bg: "var(--paper-pure)",
      bd: "var(--stone-400)",
      fg: "var(--fg-2)"
    },
    onrecord: {
      bg: "var(--ink)",
      bd: "var(--ink)",
      fg: "var(--citrate-green)"
    }
  }[status] || {
    bg: "var(--paper-2)",
    bd: "var(--stone-300)",
    fg: "var(--fg-1)"
  };
  return /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 6,
      padding: "3px 9px",
      borderRadius: "var(--r-pill)",
      background: styles.bg,
      border: `1px solid ${styles.bd}`,
      color: styles.fg,
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      fontWeight: 500,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      whiteSpace: "nowrap"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: "50%",
      background: styles.bd,
      animation: status === "pending" ? "pulseDot 1.6s var(--ease-standard) infinite" : "none"
    }
  }), children);
}

// Tiny inline icon set — 1.5px stroke, currentColor
function Icon({
  name,
  size = 16
}) {
  const s = {
    width: size,
    height: size,
    strokeWidth: 1.5,
    fill: "none",
    stroke: "currentColor",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  };
  const paths = {
    home: /*#__PURE__*/React.createElement("path", {
      d: "M3 11 L12 3 L21 11 V20 H14 V14 H10 V20 H3 Z"
    }),
    ledger: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("rect", {
      x: "4",
      y: "4",
      width: "16",
      height: "16",
      rx: "2"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M4 9 L20 9 M9 4 L9 20"
    })),
    witness: /*#__PURE__*/React.createElement("path", {
      d: "M12 22C12 22 4 15 4 9.5C4 6.5 6.5 4 9.5 4C11 4 12 5 12 5C12 5 13 4 14.5 4C17.5 4 20 6.5 20 9.5C20 15 12 22 12 22Z"
    }),
    custody: /*#__PURE__*/React.createElement("path", {
      d: "M12 2 L20 6 V12 C20 17 16 21 12 22 C8 21 4 17 4 12 V6 Z"
    }),
    audit: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("circle", {
      cx: "12",
      cy: "12",
      r: "9"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M9 12 L11 14 L15 10"
    })),
    settings: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("circle", {
      cx: "12",
      cy: "12",
      r: "3"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
    })),
    search: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("circle", {
      cx: "10",
      cy: "10",
      r: "6"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M15 15 L21 21"
    })),
    filter: /*#__PURE__*/React.createElement("path", {
      d: "M4 5 H20 M7 12 H17 M10 19 H14"
    }),
    plus: /*#__PURE__*/React.createElement("path", {
      d: "M12 4 V20 M4 12 H20"
    }),
    chevdown: /*#__PURE__*/React.createElement("path", {
      d: "M6 9 L12 15 L18 9"
    }),
    chevright: /*#__PURE__*/React.createElement("path", {
      d: "M9 6 L15 12 L9 18"
    }),
    arrow: /*#__PURE__*/React.createElement("path", {
      d: "M5 12 H19 M13 6 L19 12 L13 18"
    }),
    check: /*#__PURE__*/React.createElement("path", {
      d: "M5 12 L10 17 L19 8"
    }),
    bell: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("path", {
      d: "M18 16 V11 A6 6 0 0 0 6 11 V16 L4 18 H20 L18 16Z"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M10 21 A2 2 0 0 0 14 21"
    })),
    globe: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("circle", {
      cx: "12",
      cy: "12",
      r: "9"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M3 12 H21 M12 3 C16 7 16 17 12 21 C8 17 8 7 12 3"
    })),
    file: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("path", {
      d: "M14 3 H6 V21 H18 V7 Z"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M14 3 V7 H18"
    })),
    download: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("path", {
      d: "M12 4 V16 M6 10 L12 16 L18 10"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M4 20 H20"
    }))
  };
  return /*#__PURE__*/React.createElement("svg", {
    viewBox: "0 0 24 24",
    style: s
  }, paths[name] || null);
}
const css = `
@keyframes pulseDot { 0%,100% { opacity: 1 } 50% { opacity: .35 } }
@keyframes filingStrokeIn {
  from { background: var(--citrate-green-tint); }
  to   { background: transparent; }
}
.filing-row { transition: background var(--dur-base) var(--ease-standard); }
.filing-row:hover { background: var(--paper); }
.filing-row.just-signed { animation: filingStrokeIn 1.6s var(--ease-standard) 1; }
`;
const styleEl = document.createElement("style");
styleEl.textContent = css;
document.head.appendChild(styleEl);
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/Primitives.jsx", error: String((e && e.message) || e) }); }

// ui_kits/console/Sidebar.jsx
try { (() => {
function Sidebar({
  route,
  onRoute
}) {
  const nav = [{
    id: "dashboard",
    label: "Dashboard",
    icon: "home"
  }, {
    id: "ledger",
    label: "Ledger",
    icon: "ledger",
    badge: "84"
  }, {
    id: "witnesses",
    label: "Witnesses",
    icon: "witness"
  }, {
    id: "custodians",
    label: "Custodians",
    icon: "custody"
  }, {
    id: "audit",
    label: "Audit log",
    icon: "audit"
  }];
  const admin = [{
    id: "settings",
    label: "Settings",
    icon: "settings"
  }];
  return /*#__PURE__*/React.createElement("aside", {
    "data-screen-label": "Sidebar",
    style: {
      background: "var(--deep-evergreen)",
      color: "#cde7d6",
      padding: "20px 16px",
      display: "flex",
      flexDirection: "column",
      borderRight: "1px solid var(--ink-2)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 10,
      padding: "4px 8px 24px"
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "assets/citrate_mark_white.svg",
    alt: "",
    style: {
      width: 28,
      height: 28
    }
  }), /*#__PURE__*/React.createElement("img", {
    src: "assets/citrate_marquee_white.svg",
    alt: "Citrate",
    style: {
      height: 14
    }
  })), /*#__PURE__*/React.createElement(ProjectSwitcher, null), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 22,
      flex: 1
    }
  }, /*#__PURE__*/React.createElement(NavSection, {
    items: nav,
    route: route,
    onRoute: onRoute
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      height: 22
    }
  }), /*#__PURE__*/React.createElement(NavSection, {
    label: "Admin",
    items: admin,
    route: route,
    onRoute: onRoute
  })), /*#__PURE__*/React.createElement(JurisdictionStrip, null), /*#__PURE__*/React.createElement(UserCard, null));
}
function NavSection({
  label,
  items,
  route,
  onRoute
}) {
  return /*#__PURE__*/React.createElement("div", null, label && /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.5)",
      padding: "8px 12px"
    }
  }, label), /*#__PURE__*/React.createElement("ul", {
    style: {
      listStyle: "none",
      padding: 0,
      margin: 0
    }
  }, items.map(it => {
    const active = route === it.id;
    return /*#__PURE__*/React.createElement("li", {
      key: it.id
    }, /*#__PURE__*/React.createElement("a", {
      href: `#${it.id}`,
      onClick: e => {
        e.preventDefault();
        onRoute(it.id);
      },
      style: {
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "9px 12px",
        borderRadius: "var(--r-1)",
        fontFamily: "var(--font-sans)",
        fontSize: 13,
        fontWeight: active ? 500 : 400,
        color: active ? "var(--ink)" : "#cde7d6",
        background: active ? "var(--citrate-green)" : "transparent",
        marginBottom: 2,
        transition: "background var(--dur-fast) var(--ease-standard), color var(--dur-fast) var(--ease-standard)"
      }
    }, /*#__PURE__*/React.createElement(Icon, {
      name: it.icon,
      size: 16
    }), /*#__PURE__*/React.createElement("span", {
      style: {
        flex: 1
      }
    }, it.label), it.badge && /*#__PURE__*/React.createElement("span", {
      style: {
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        background: active ? "var(--ink)" : "rgba(205,231,214,0.12)",
        color: active ? "var(--citrate-green)" : "rgba(205,231,214,0.8)",
        padding: "1px 6px",
        borderRadius: "var(--r-pill)"
      }
    }, it.badge)));
  })));
}
function ProjectSwitcher() {
  return /*#__PURE__*/React.createElement("button", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 10,
      width: "100%",
      padding: "10px 12px",
      background: "rgba(205,231,214,0.06)",
      border: "1px solid rgba(205,231,214,0.15)",
      borderRadius: "var(--r-1)",
      color: "#cde7d6",
      textAlign: "left",
      cursor: "pointer"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 26,
      height: 26,
      borderRadius: "var(--r-1)",
      background: "var(--citrate-yellow)",
      color: "var(--ink)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      fontFamily: "var(--font-display)",
      fontWeight: 480,
      fontSize: 14
    }
  }, "B"), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      fontWeight: 500,
      color: "var(--paper)"
    }
  }, "defense_prime \xB7 Procurement #14"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: ".1em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.6)"
    }
  }, "Tenant \xB7 US-FED")), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "rgba(205,231,214,0.7)"
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "chevdown",
    size: 14
  })));
}
function JurisdictionStrip() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: "1px solid rgba(205,231,214,0.12)",
      padding: "14px 12px",
      marginTop: 14
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.5)",
      marginBottom: 8
    }
  }, "Jurisdiction"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "globe",
    size: 14
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 13,
      color: "var(--paper)"
    }
  }, "United States \xB7 Federal")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 4,
      marginTop: 8,
      flexWrap: "wrap"
    }
  }, ["US-DE", "US-NY", "JP", "EU"].map(j => /*#__PURE__*/React.createElement("span", {
    key: j,
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: ".1em",
      color: "rgba(205,231,214,0.7)",
      border: "1px solid rgba(205,231,214,0.15)",
      padding: "2px 6px",
      borderRadius: "var(--r-pill)"
    }
  }, j)), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      color: "rgba(205,231,214,0.5)",
      padding: "2px 4px"
    }
  }, "+ 83")));
}
function UserCard() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 10,
      padding: "12px",
      borderTop: "1px solid rgba(205,231,214,0.12)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 30,
      height: 30,
      borderRadius: "var(--r-pill)",
      background: "var(--citrate-green)",
      color: "var(--ink)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      fontFamily: "var(--font-sans)",
      fontWeight: 600,
      fontSize: 12
    }
  }, "AR"), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13,
      fontWeight: 500,
      color: "var(--paper)"
    }
  }, "Aleia Rouhani"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: ".1em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.5)"
    }
  }, "Compliance lead")));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/Sidebar.jsx", error: String((e && e.message) || e) }); }

// ui_kits/console/TopBar.jsx
try { (() => {
function TopBar({
  breadcrumbs = [],
  title,
  eyebrow,
  actions
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "20px 32px 0",
      background: "var(--paper)",
      position: "sticky",
      top: 0,
      zIndex: 10
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      marginBottom: 16
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 6,
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      letterSpacing: ".1em",
      textTransform: "uppercase",
      color: "var(--fg-3)"
    }
  }, breadcrumbs.map((b, i) => /*#__PURE__*/React.createElement(React.Fragment, {
    key: i
  }, i > 0 && /*#__PURE__*/React.createElement("span", {
    style: {
      opacity: 0.5
    }
  }, "/"), /*#__PURE__*/React.createElement("a", {
    style: {
      color: i === breadcrumbs.length - 1 ? "var(--fg-1)" : "var(--fg-3)"
    }
  }, b)))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: "relative"
    }
  }, /*#__PURE__*/React.createElement("input", {
    className: "input",
    placeholder: "Search filings, hashes, counterparties\u2026",
    style: {
      width: 320,
      height: 32,
      fontSize: 13,
      paddingLeft: 32,
      background: "var(--paper-pure)"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      left: 10,
      top: 9,
      color: "var(--fg-3)",
      pointerEvents: "none"
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "search",
    size: 14
  }))), /*#__PURE__*/React.createElement("button", {
    className: "btn btn-ghost btn-sm",
    style: {
      height: 32,
      padding: "0 10px"
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "bell",
    size: 14
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "flex-end",
      justifyContent: "space-between",
      paddingBottom: 20,
      borderBottom: "1px solid var(--stone-200)"
    }
  }, /*#__PURE__*/React.createElement("div", null, eyebrow && /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 8
    }
  }, eyebrow), /*#__PURE__*/React.createElement("h1", {
    style: {
      fontFamily: "var(--font-display)",
      fontWeight: 420,
      fontVariationSettings: '"opsz" 36',
      fontSize: 36,
      lineHeight: 1.1,
      margin: 0,
      color: "var(--ink)",
      letterSpacing: "-0.012em"
    }
  }, title)), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8
    }
  }, actions)));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/console/TopBar.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/ContractCard.jsx
try { (() => {
function ContractCard({
  id,
  status,
  title,
  counterparty,
  jurisdiction,
  amount,
  witnesses,
  effective
}) {
  const statusStyles = {
    settled: {
      bg: "#ecf5d4",
      bd: "#4f8a05",
      fg: "#3a6500",
      label: "Settled"
    },
    pending: {
      bg: "#fff1c4",
      bd: "#b07b00",
      fg: "#6e5100",
      label: "Witness pending"
    },
    blocked: {
      bg: "#f6e1de",
      bd: "#a72414",
      fg: "#7a1a10",
      label: "Blocked"
    }
  }[status];
  return /*#__PURE__*/React.createElement("div", {
    className: "contract-card",
    style: {
      background: "var(--paper-pure)",
      border: "1px solid var(--stone-200)",
      borderRadius: "var(--r-2)",
      padding: 24,
      transition: "border-color var(--dur-base) var(--ease-standard), box-shadow var(--dur-base) var(--ease-standard)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "center",
      paddingBottom: 14,
      borderBottom: "1px solid var(--stone-150)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--fg-3)"
    }
  }, "Filing \xB7 ", id, " \xB7 ", jurisdiction), /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 6,
      padding: "3px 9px",
      borderRadius: "var(--r-pill)",
      background: statusStyles.bg,
      border: `1px solid ${statusStyles.bd}`,
      color: statusStyles.fg,
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      fontWeight: 500,
      letterSpacing: "0.14em",
      textTransform: "uppercase"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: "50%",
      background: statusStyles.bd
    }
  }), statusStyles.label)), /*#__PURE__*/React.createElement("h3", {
    style: {
      fontFamily: "var(--font-display)",
      fontWeight: 440,
      fontVariationSettings: '"opsz" 24',
      fontSize: 24,
      lineHeight: 1.2,
      margin: "16px 0 12px",
      color: "var(--ink)"
    }
  }, title), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      rowGap: 12,
      columnGap: 18,
      marginTop: 14
    }
  }, /*#__PURE__*/React.createElement(Field, {
    k: "Counterparty",
    v: counterparty
  }), /*#__PURE__*/React.createElement(Field, {
    k: "Notional",
    v: amount,
    mono: true
  }), /*#__PURE__*/React.createElement(Field, {
    k: "Witnesses",
    v: witnesses
  }), /*#__PURE__*/React.createElement(Field, {
    k: "Effective",
    v: effective,
    mono: true
  })));
}
function Field({
  k,
  v,
  mono
}) {
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--fg-3)"
    }
  }, k), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
      fontSize: mono ? 13 : 14,
      fontWeight: 500,
      color: "var(--ink)",
      marginTop: 4,
      fontFeatureSettings: '"tnum","lnum"'
    }
  }, v));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/ContractCard.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/Footer.jsx
try { (() => {
function Footer() {
  const columns = [{
    heading: "Network",
    links: ["Substrate", "Jurisdictions", "Custodians", "Witness lineage", "AI co-pilot"]
  }, {
    heading: "Console",
    links: ["For procurement", "For compliance", "For audit", "For government", "Operations"]
  }, {
    heading: "Compliance",
    links: ["Continuous attestation", "FAR / DFARS", "ISO 27001", "SOC 2 Type II", "Public auditors"]
  }, {
    heading: "Company",
    links: ["About", "Customers", "Press", "Careers", "Contact"]
  }];
  return /*#__PURE__*/React.createElement("footer", {
    "data-screen-label": "Footer",
    style: {
      background: "var(--deep-evergreen)",
      color: "#cde7d6",
      padding: "96px 0 48px",
      marginTop: 96
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "container"
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1.6fr 1fr 1fr 1fr 1fr",
      gap: 48,
      alignItems: "start"
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("img", {
    src: "assets/citrate_marquee_white.svg",
    alt: "Citrate",
    style: {
      height: 30
    }
  }), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 18',
      fontSize: 18,
      lineHeight: 1.6,
      color: "#cde7d6",
      marginTop: 24,
      maxWidth: 320,
      opacity: 0.86
    }
  }, "The substrate of record for the world's most regulated organizations."), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.55)",
      marginTop: 32,
      lineHeight: 1.8
    }
  }, "Headquartered \xB7 Wilmington, DE", /*#__PURE__*/React.createElement("br", null), "Offices \xB7 Washington \xB7 Brussels \xB7 Tokyo")), columns.map(c => /*#__PURE__*/React.createElement("div", {
    key: c.heading
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      fontWeight: 500,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--citrate-green)",
      marginBottom: 18,
      paddingBottom: 12,
      borderBottom: "1px solid rgba(205,231,214,0.15)"
    }
  }, c.heading), /*#__PURE__*/React.createElement("ul", {
    style: {
      listStyle: "none",
      padding: 0,
      margin: 0,
      display: "flex",
      flexDirection: "column",
      gap: 10
    }
  }, c.links.map(l => /*#__PURE__*/React.createElement("li", {
    key: l
  }, /*#__PURE__*/React.createElement("a", {
    href: "#",
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      color: "#cde7d6",
      opacity: 0.86,
      transition: "color var(--dur-fast) var(--ease-standard)"
    },
    onMouseOver: e => e.target.style.color = "var(--citrate-green)",
    onMouseOut: e => {
      e.target.style.color = "#cde7d6";
    }
  }, l))))))), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 80,
      paddingTop: 28,
      borderTop: "1px solid rgba(205,231,214,0.15)",
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.55)"
    }
  }, "\xA9 2026 Citrate Network, Inc. \xB7 All filings continuously attested."), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.55)",
      display: "flex",
      gap: 24
    }
  }, /*#__PURE__*/React.createElement("a", {
    href: "#"
  }, "Terms"), /*#__PURE__*/React.createElement("a", {
    href: "#"
  }, "Privacy"), /*#__PURE__*/React.createElement("a", {
    href: "#"
  }, "Security"), /*#__PURE__*/React.createElement("a", {
    href: "#"
  }, "Disclosures")))));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/Footer.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/Hero.jsx
try { (() => {
function Hero() {
  const [animKey, setAnimKey] = React.useState(0);
  const [markSvg, setMarkSvg] = React.useState("");
  React.useEffect(() => {
    fetch("assets/citrate_mark_green.svg").then(r => r.text()).then(setMarkSvg);
  }, []);
  React.useEffect(() => {
    const t = setInterval(() => setAnimKey(k => k + 1), 6000);
    return () => clearInterval(t);
  }, []);
  return /*#__PURE__*/React.createElement("section", {
    className: "ledger-grid",
    "data-screen-label": "Hero",
    style: {
      paddingTop: 140,
      paddingBottom: 120,
      position: "relative"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "container"
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1.4fr 1fr",
      gap: 64,
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 28
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--citrate-green-deep)"
    }
  }, "\u25CF "), "Citrate Network \xB7 live in 87 jurisdictions"), /*#__PURE__*/React.createElement("h1", {
    className: "serif-display",
    style: {
      fontSize: 96,
      margin: 0,
      fontWeight: 360,
      textWrap: "balance"
    }
  }, "Procurement on the", /*#__PURE__*/React.createElement("br", null), "public record."), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 22',
      fontSize: 22,
      lineHeight: 1.5,
      color: "var(--fg-2)",
      maxWidth: 580,
      marginTop: 28
    }
  }, "The AI-native substrate connecting the world's most regulated organizations \u2014 primes, governments, and the institutions they answer to. One ledger. Every jurisdiction. Continuously audited."), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 12,
      marginTop: 40
    }
  }, /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-primary",
    style: {
      height: 50,
      padding: "0 26px",
      fontSize: 15
    }
  }, "Request access"), /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-ghost",
    style: {
      height: 50,
      padding: "0 26px",
      fontSize: 15
    }
  }, "Read the compliance brief \u2192"))), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "relative",
      aspectRatio: "1",
      display: "flex",
      alignItems: "center",
      justifyContent: "center"
    }
  }, /*#__PURE__*/React.createElement("div", {
    key: animKey,
    className: "anim-mark",
    style: {
      width: "80%"
    },
    dangerouslySetInnerHTML: {
      __html: markSvg
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      top: 0,
      right: 0,
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--fg-3)",
      textAlign: "right",
      lineHeight: 1.6
    }
  }, "Witness lineage", /*#__PURE__*/React.createElement("br", null), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--ink)"
    }
  }, "0x8e\u2026a019"), /*#__PURE__*/React.createElement("br", null), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--ink)"
    }
  }, "0xcc\u20267b21"), /*#__PURE__*/React.createElement("br", null), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--ink)"
    }
  }, "0x09\u20264f02")), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      bottom: 0,
      left: 0,
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--fg-3)",
      lineHeight: 1.6
    }
  }, "Last attestation", /*#__PURE__*/React.createElement("br", null), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--ink)"
    }
  }, "17 May 2026 \xB7 14:02 UTC"))))), /*#__PURE__*/React.createElement("style", null, `
        .anim-mark svg { width: 100%; height: auto; }
        .anim-mark svg path { opacity: 0; transform-origin: center; transform: translateY(8px) scale(0.94); animation: settle 900ms cubic-bezier(.34,1.18,.4,1) forwards; }
        ${[...Array(9)].map((_, i) => `.anim-mark svg path:nth-child(${i + 1}) { animation-delay: ${i * 70}ms; }`).join("\n")}
        @keyframes settle { to { opacity: 1; transform: translateY(0) scale(1); } }
      `));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/Hero.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/LogoCloud.jsx
try { (() => {
function LogoCloud() {
  // ⚠ Placeholder typographic logos — replace with permissioned customer marks.
  const logos = [{
    name: "defense_prime",
    style: "italic",
    weight: 700,
    font: "var(--font-display)"
  }, {
    name: "Meridian Chemical",
    style: "normal",
    weight: 600,
    font: "var(--font-sans)"
  }, {
    name: "CONTINENTAL",
    style: "normal",
    weight: 500,
    font: "var(--font-sans)",
    tracking: "0.04em"
  }, {
    name: "U.S. TREASURY",
    style: "normal",
    weight: 460,
    font: "var(--font-display)",
    size: 16
  }, {
    name: "METI",
    style: "normal",
    weight: 600,
    font: "var(--font-sans)",
    tracking: "0.18em"
  }, {
    name: "SENTINEL",
    style: "normal",
    weight: 500,
    font: "var(--font-sans)",
    tracking: "0.12em",
    size: 16
  }, {
    name: "Vanguard Systems",
    style: "normal",
    weight: 420,
    font: "var(--font-display)",
    size: 18
  }, {
    name: "GULF ENERGY",
    style: "normal",
    weight: 600,
    font: "var(--font-sans)",
    tracking: "0.16em"
  }];
  return /*#__PURE__*/React.createElement("section", {
    "data-screen-label": "LogoCloud",
    style: {
      padding: "64px 0",
      borderTop: "1px solid var(--stone-200)",
      borderBottom: "1px solid var(--stone-200)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "container"
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      textAlign: "center",
      color: "var(--fg-3)",
      marginBottom: 36
    }
  }, "Trusted by primes, governments, and the institutions they answer to"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "repeat(4, 1fr)",
      rowGap: 32,
      columnGap: 32,
      alignItems: "center",
      justifyItems: "center"
    }
  }, logos.map(l => /*#__PURE__*/React.createElement("div", {
    key: l.name,
    style: {
      fontFamily: l.font,
      fontWeight: l.weight,
      fontStyle: l.style,
      fontSize: l.size || 22,
      letterSpacing: l.tracking || "0",
      color: "var(--stone-700)",
      opacity: 0.78
    }
  }, l.name))), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--fg-3)",
      textAlign: "center",
      marginTop: 36
    }
  }, "Placeholder \xB7 replace with permissioned customer marks")));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/LogoCloud.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/MarketingApp.jsx
try { (() => {
function MarketingApp() {
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement(Nav, null), /*#__PURE__*/React.createElement(Hero, null), /*#__PURE__*/React.createElement(LogoCloud, null), /*#__PURE__*/React.createElement(Section, {
    screenLabel: "Section \xB7 Substrate",
    eyebrow: "The substrate",
    title: "One ledger. Every jurisdiction.",
    body: "Citrate routes filings across regulators in 87 jurisdictions \u2014 federal, state, supranational. Counterparties keep their systems; the substrate keeps the record."
  }, /*#__PURE__*/React.createElement(FeatureList, {
    items: [{
      title: "Continuously attested",
      body: "Every filing carries a queryable lineage of attesting custodians, refreshed at the rate the ledger settles. No quarterly audit theatre — the audit is the system."
    }, {
      title: "AI-native by construction",
      body: "Compute is shared across borders to a global mesh. The model that drafts your filing is the model that witnesses your counterparty's."
    }, {
      title: "Sturdy by design",
      body: "Built to decompose: instruments, witnesses, custodians and jurisdictions are independent surfaces. Replace one without disturbing the others."
    }]
  })), /*#__PURE__*/React.createElement(Section, {
    screenLabel: "Section \xB7 Filings",
    eyebrow: "In the console",
    title: "The instrument of record.",
    body: "Procurement teams sign in to the same view their auditors get. Every filing is a document, a witness lineage, and a settled position in one object."
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 16
    }
  }, /*#__PURE__*/React.createElement(ContractCard, {
    id: "CTR\u20119F4A",
    jurisdiction: "US\u2011FED",
    status: "settled",
    title: "Supply contract \u2014 FAR 52.246",
    counterparty: "defense_prime Procurement #14",
    amount: "$ 2,418,000.00",
    witnesses: "3 of 3 \xB7 Kestrel Advisory",
    effective: "2026\u201105\u201117"
  }), /*#__PURE__*/React.createElement(ContractCard, {
    id: "CTR\u20118B12",
    jurisdiction: "US\u2011DE",
    status: "pending",
    title: "Hazardous materials manifest \u2014 DOT 173",
    counterparty: "Meridian Chemical, Inc.",
    amount: "$ 1,084,200.00",
    witnesses: "2 of 3 \xB7 EY pending",
    effective: "2026\u201105\u201115"
  }), /*#__PURE__*/React.createElement(ContractCard, {
    id: "CTR\u20116D8C",
    jurisdiction: "JP\u2011GOV",
    status: "blocked",
    title: "Procurement exemption \u2014 METI Notice 38",
    counterparty: "METI \xB7 Jurisdiction 38",
    amount: "$ 312,000.00",
    witnesses: "0 of 2 \xB7 custodian unreachable",
    effective: "2026\u201105\u201112"
  }))), /*#__PURE__*/React.createElement(StatRow, null), /*#__PURE__*/React.createElement(Section, {
    screenLabel: "Section \xB7 Compliance",
    eyebrow: "Compliance is the floor",
    title: "Compliant by construction, not by certificate.",
    body: "FAR \xB7 DFARS \xB7 ISO 27001 \xB7 SOC 2 Type II. Every claim Citrate makes about itself is queryable from inside the substrate by every participant \u2014 including the public auditors."
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      gap: 16
    }
  }, [{
    tag: "FAR · DFARS",
    body: "Federal Acquisition Regulation, fully integrated. Treasury and DOD custodians are first-class participants."
  }, {
    tag: "ISO 27001",
    body: "Information-security controls audited continuously, not annually. Findings are public."
  }, {
    tag: "SOC 2 Type II",
    body: "Trust services criteria attested by three independent firms on a rolling basis."
  }, {
    tag: "Public auditors",
    body: "Kestrel Advisory, EY, and Beacon operate witness custodians on the substrate. Their attestations are queryable by any participant."
  }].map(c => /*#__PURE__*/React.createElement("div", {
    key: c.tag,
    style: {
      background: "var(--paper-pure)",
      border: "1px solid var(--stone-200)",
      borderRadius: "var(--r-2)",
      padding: 22
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--citrate-green-deep)"
    }
  }, c.tag), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      lineHeight: 1.55,
      color: "var(--fg-2)",
      margin: "12px 0 0"
    }
  }, c.body))))), /*#__PURE__*/React.createElement(CTABanner, null), /*#__PURE__*/React.createElement(Footer, null));
}
function CTABanner() {
  return /*#__PURE__*/React.createElement("section", {
    "data-screen-label": "CTA",
    style: {
      padding: "120px 0",
      background: "var(--paper-pure)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "container",
    style: {
      textAlign: "center"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 28
    }
  }, "Request access"), /*#__PURE__*/React.createElement("h2", {
    className: "serif-display",
    style: {
      fontSize: 80,
      margin: "0 auto",
      maxWidth: 900,
      fontWeight: 360,
      textWrap: "balance"
    }
  }, "The record your operators trust.", /*#__PURE__*/React.createElement("br", null), "The record your auditors can read."), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 12,
      justifyContent: "center",
      marginTop: 40
    }
  }, /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-primary",
    style: {
      height: 52,
      padding: "0 28px",
      fontSize: 15
    }
  }, "Request access"), /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-ghost",
    style: {
      height: 52,
      padding: "0 28px",
      fontSize: 15
    }
  }, "Talk to compliance \u2192"))));
}
ReactDOM.createRoot(document.getElementById("root")).render(/*#__PURE__*/React.createElement(MarketingApp, null));
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/MarketingApp.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/Nav.jsx
try { (() => {
const {
  useEffect,
  useState
} = React;
function Nav() {
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 40);
    window.addEventListener("scroll", onScroll);
    return () => window.removeEventListener("scroll", onScroll);
  }, []);
  const wrapStyle = {
    position: "fixed",
    top: 0,
    left: 0,
    right: 0,
    zIndex: 50,
    background: scrolled ? "rgba(244,241,234,0.82)" : "transparent",
    backdropFilter: scrolled ? "blur(20px) saturate(120%)" : "none",
    WebkitBackdropFilter: scrolled ? "blur(20px) saturate(120%)" : "none",
    borderBottom: scrolled ? "1px solid var(--stone-200)" : "1px solid transparent",
    transition: "background var(--dur-base) var(--ease-standard), border-color var(--dur-base) var(--ease-standard)"
  };
  const innerStyle = {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    height: 76
  };
  const links = ["Network", "Console", "Compliance", "Customers", "Company"];
  return /*#__PURE__*/React.createElement("nav", {
    style: wrapStyle,
    "data-screen-label": "Nav"
  }, /*#__PURE__*/React.createElement("div", {
    className: "container",
    style: innerStyle
  }, /*#__PURE__*/React.createElement("a", {
    href: "#",
    style: {
      display: "flex",
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "assets/citrate_marquee_black.svg",
    alt: "Citrate",
    style: {
      height: 24
    }
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 32,
      alignItems: "center"
    }
  }, links.map((l, i) => /*#__PURE__*/React.createElement("a", {
    key: l,
    href: "#",
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 500,
      color: i === 0 ? "var(--citrate-green-deep)" : "var(--fg-1)",
      borderBottom: i === 0 ? "1.5px solid var(--citrate-green-deep)" : "1.5px solid transparent",
      paddingBottom: 2,
      transition: "color var(--dur-fast) var(--ease-standard)"
    }
  }, l))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 10,
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-ghost",
    style: {
      height: 36,
      padding: "0 14px",
      fontSize: 13
    }
  }, "Sign in"), /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-primary",
    style: {
      height: 36,
      padding: "0 14px",
      fontSize: 13
    }
  }, "Request access"))));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/Nav.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/Quote.jsx
try { (() => {
function Quote() {
  return /*#__PURE__*/React.createElement("section", {
    "data-screen-label": "Quote",
    style: {
      padding: "120px 0",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "container"
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 2.4fr",
      gap: 64,
      alignItems: "start"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, "Testimony"), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("blockquote", {
    style: {
      margin: 0,
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 48',
      fontWeight: 380,
      fontSize: 44,
      lineHeight: 1.18,
      letterSpacing: "-0.014em",
      color: "var(--ink)",
      textWrap: "balance"
    }
  }, "Citrate moved our procurement record-keeping from an after-the-fact audit problem into a continuous attestation. Our regulators get the same view our operators do \u2014 and they see it the moment we do."), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 36,
      display: "flex",
      alignItems: "center",
      gap: 16
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 48,
      height: 48,
      borderRadius: "50%",
      background: "var(--ink)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      color: "var(--citrate-green)",
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 18',
      fontSize: 18,
      fontWeight: 460
    }
  }, "EM"), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 15,
      fontWeight: 500,
      color: "var(--ink)"
    }
  }, "Eliza Marchetti"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--fg-3)",
      marginTop: 4
    }
  }, "Chief Procurement Officer \xB7 Fortune 50 industrial"))), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 28,
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.14em",
      textTransform: "uppercase",
      color: "var(--fg-3)"
    }
  }, "\u26A0 Interpreted testimony \xB7 replace with a permissioned customer quote")))));
}
function CTA() {
  return /*#__PURE__*/React.createElement("section", {
    "data-screen-label": "CTA",
    style: {
      background: "var(--ink)",
      color: "var(--paper)",
      padding: "120px 0"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "container"
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1.4fr 1fr",
      gap: 64,
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("h2", {
    className: "serif-display",
    style: {
      color: "var(--paper)",
      fontSize: 72,
      fontWeight: 360,
      fontVariationSettings: '"opsz" 60',
      margin: 0,
      letterSpacing: "-0.022em"
    }
  }, "Bring your procurement", /*#__PURE__*/React.createElement("br", null), "onto the record."), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 20',
      fontSize: 20,
      lineHeight: 1.55,
      color: "rgba(244,241,234,0.78)",
      marginTop: 24,
      maxWidth: 520
    }
  }, "Access is granted by jurisdiction. We onboard one prime per government cohort. Counterparty-side onboarding is free for the duration of the cohort.")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 12,
      alignItems: "flex-start"
    }
  }, /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-primary",
    style: {
      height: 56,
      padding: "0 32px",
      fontSize: 16
    }
  }, "Request cohort access"), /*#__PURE__*/React.createElement("a", {
    href: "#",
    className: "btn btn-on-dark",
    style: {
      height: 56,
      padding: "0 32px",
      fontSize: 16
    }
  }, "Download the compliance brief (PDF, 4.2 MB)")))));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/Quote.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/Section.jsx
try { (() => {
function Section({
  eyebrow,
  title,
  body,
  children,
  screenLabel
}) {
  return /*#__PURE__*/React.createElement("section", {
    className: "section",
    "data-screen-label": screenLabel
  }, /*#__PURE__*/React.createElement("div", {
    className: "container"
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1.6fr",
      gap: 64,
      alignItems: "start"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: "sticky",
      top: 120
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow"
  }, eyebrow), /*#__PURE__*/React.createElement("h2", {
    className: "serif-display",
    style: {
      fontSize: 56,
      fontWeight: 380,
      fontVariationSettings: '"opsz" 48',
      margin: "20px 0 24px",
      lineHeight: 1.04,
      letterSpacing: "-0.018em"
    }
  }, title), body && /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 18',
      fontSize: 18,
      lineHeight: 1.6,
      color: "var(--fg-2)",
      margin: 0
    }
  }, body)), /*#__PURE__*/React.createElement("div", null, children))));
}
function FeatureList({
  items
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column"
    }
  }, items.map((it, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      display: "grid",
      gridTemplateColumns: "44px 1fr",
      gap: 24,
      padding: "28px 0",
      borderTop: i === 0 ? "1px solid var(--ink)" : "1px solid var(--stone-200)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 12,
      color: "var(--fg-3)",
      paddingTop: 4,
      fontFeatureSettings: '"tnum","lnum"'
    }
  }, String(i + 1).padStart(2, "0")), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("h3", {
    style: {
      fontFamily: "var(--font-display)",
      fontWeight: 440,
      fontVariationSettings: '"opsz" 24',
      fontSize: 26,
      lineHeight: 1.15,
      margin: 0,
      color: "var(--ink)"
    }
  }, it.title), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 15,
      lineHeight: 1.55,
      color: "var(--fg-2)",
      margin: "10px 0 0",
      maxWidth: 520
    }
  }, it.body)))));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/Section.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/Stat.jsx
try { (() => {
function Stat({
  value,
  suffix,
  label,
  note
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "32px 0",
      borderTop: "1px solid var(--stone-300)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 60',
      fontWeight: 360,
      fontSize: 96,
      lineHeight: 0.95,
      letterSpacing: "-0.025em",
      color: "var(--ink)",
      fontFeatureSettings: '"tnum","lnum"'
    }
  }, value, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--citrate-green-deep)",
      fontSize: 56,
      marginLeft: 4
    }
  }, suffix)), /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginTop: 18
    }
  }, label), note && /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      color: "var(--fg-3)",
      marginTop: 8,
      maxWidth: 280
    }
  }, note));
}
function StatRow() {
  return /*#__PURE__*/React.createElement("section", {
    "data-screen-label": "Stats",
    style: {
      padding: "120px 0",
      background: "var(--paper-pure)",
      borderTop: "1px solid var(--stone-200)",
      borderBottom: "1px solid var(--stone-200)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "container"
  }, /*#__PURE__*/React.createElement("div", {
    className: "eyebrow",
    style: {
      marginBottom: 56
    }
  }, "The record \xB7 year to date \xB7 audited"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "repeat(4, 1fr)",
      gap: 32
    }
  }, /*#__PURE__*/React.createElement(Stat, {
    value: "87",
    suffix: "",
    label: "Jurisdictions",
    note: "Federal, state, and supranational regulators integrated."
  }), /*#__PURE__*/React.createElement(Stat, {
    value: "2.4",
    suffix: "T",
    label: "Notional routed",
    note: "USD-equivalent, settled and on the public record."
  }), /*#__PURE__*/React.createElement(Stat, {
    value: "312",
    suffix: "K",
    label: "Filings witnessed",
    note: "Each carries a queryable lineage of attesting custodians."
  }), /*#__PURE__*/React.createElement(Stat, {
    value: "99.997",
    suffix: "%",
    label: "Attestation uptime",
    note: "Continuously verified by three independent auditors."
  }))));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/Stat.jsx", error: String((e && e.message) || e) }); }

// ui_kits/mobile/MobileApp.jsx
try { (() => {
// Citrate mobile screens
const {
  useState
} = React;
function MobileNav({
  active = "today"
}) {
  const items = [{
    id: "today",
    label: "Today",
    icon: "home"
  }, {
    id: "queue",
    label: "Witness",
    icon: "witness"
  }, {
    id: "ledger",
    label: "Ledger",
    icon: "ledger"
  }, {
    id: "me",
    label: "Profile",
    icon: "me"
  }];
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      bottom: 24,
      left: 12,
      right: 12,
      background: "rgba(244,241,234,0.86)",
      backdropFilter: "blur(20px) saturate(140%)",
      WebkitBackdropFilter: "blur(20px) saturate(140%)",
      borderRadius: 24,
      border: "1px solid rgba(14,15,12,0.08)",
      padding: "8px 6px",
      display: "flex",
      justifyContent: "space-around",
      zIndex: 5
    }
  }, items.map(it => /*#__PURE__*/React.createElement("div", {
    key: it.id,
    style: {
      display: "flex",
      flexDirection: "column",
      alignItems: "center",
      gap: 2,
      padding: "6px 10px",
      borderRadius: 14,
      flex: 1,
      background: active === it.id ? "var(--ink)" : "transparent",
      color: active === it.id ? "var(--citrate-green)" : "var(--ink)"
    }
  }, /*#__PURE__*/React.createElement(NavIcon, {
    name: it.icon,
    size: 20
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 9,
      letterSpacing: "0.12em",
      textTransform: "uppercase"
    }
  }, it.label))));
}
function NavIcon({
  name,
  size = 18
}) {
  const s = {
    width: size,
    height: size,
    strokeWidth: 1.6,
    fill: "none",
    stroke: "currentColor",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  };
  const paths = {
    home: /*#__PURE__*/React.createElement("path", {
      d: "M3 11 L12 3 L21 11 V20 H14 V14 H10 V20 H3 Z"
    }),
    witness: /*#__PURE__*/React.createElement("path", {
      d: "M12 21C12 21 5 15 5 10C5 7.5 7 5.5 9.5 5.5C11 5.5 12 7 12 7C12 7 13 5.5 14.5 5.5C17 5.5 19 7.5 19 10C19 15 12 21 12 21Z"
    }),
    ledger: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("rect", {
      x: "4",
      y: "4",
      width: "16",
      height: "16",
      rx: "2"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M4 9 L20 9 M9 4 L9 20"
    })),
    me: /*#__PURE__*/React.createElement("g", null, /*#__PURE__*/React.createElement("circle", {
      cx: "12",
      cy: "8",
      r: "4"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M4 21C4 16 8 14 12 14C16 14 20 16 20 21"
    })),
    check: /*#__PURE__*/React.createElement("path", {
      d: "M5 12 L10 17 L19 8"
    }),
    chev: /*#__PURE__*/React.createElement("path", {
      d: "M9 6 L15 12 L9 18"
    }),
    pulse: /*#__PURE__*/React.createElement("path", {
      d: "M3 12 H7 L10 5 L14 19 L17 12 H21"
    })
  };
  return /*#__PURE__*/React.createElement("svg", {
    viewBox: "0 0 24 24",
    style: s
  }, paths[name] || null);
}

// ─── Screen 1: Auth ──────────────────────────────────────────
function AuthScreen() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      minHeight: "100%",
      background: "var(--deep-evergreen)",
      color: "#cde7d6",
      display: "flex",
      flexDirection: "column",
      padding: "84px 24px 32px",
      position: "relative"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      inset: 0,
      backgroundImage: "radial-gradient(circle at 1px 1px, rgba(205,231,214,0.16) 1px, transparent 1px)",
      backgroundSize: "20px 20px",
      WebkitMaskImage: "radial-gradient(ellipse at top right, #000 30%, transparent 75%)",
      maskImage: "radial-gradient(ellipse at top right, #000 30%, transparent 75%)",
      opacity: 0.9,
      pointerEvents: "none"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "relative",
      zIndex: 1,
      display: "flex",
      flexDirection: "column",
      flex: 1
    }
  }, /*#__PURE__*/React.createElement("img", {
    src: "assets/citrate_marquee_white.svg",
    style: {
      height: 18,
      alignSelf: "flex-start"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 56,
      flex: 1
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.18em",
      textTransform: "uppercase",
      color: "var(--citrate-green)"
    }
  }, "Sign in"), /*#__PURE__*/React.createElement("h1", {
    style: {
      fontFamily: "var(--font-display)",
      fontVariationSettings: '"opsz" 48',
      fontWeight: 360,
      fontSize: 44,
      lineHeight: 1.04,
      letterSpacing: "-0.022em",
      margin: "16px 0 12px",
      color: "var(--paper)"
    }
  }, "The audit is the system."), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 16,
      lineHeight: 1.5,
      color: "rgba(205,231,214,0.78)",
      margin: 0,
      fontVariationSettings: '"opsz" 16'
    }
  }, "Sign in with your tenant's custodian.")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 10,
      marginBottom: 24
    }
  }, [{
    name: "Face ID · defense_prime",
    j: "US‑FED"
  }, {
    name: "Kestrel SSO",
    j: "Federal"
  }, {
    name: "Government PIV",
    j: "DOD"
  }].map(p => /*#__PURE__*/React.createElement("button", {
    key: p.name,
    className: "m-btn",
    style: {
      background: "rgba(205,231,214,0.06)",
      color: "var(--paper)",
      border: "1px solid rgba(205,231,214,0.18)",
      justifyContent: "space-between",
      padding: "0 18px"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontWeight: 500,
      fontSize: 15
    }
  }, p.name), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: ".14em",
      color: "rgba(205,231,214,0.55)"
    }
  }, p.j)))), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 9.5,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "rgba(205,231,214,0.5)",
      textAlign: "center"
    }
  }, "Continuously attested \xB7 99.997 % \xB7 30 days")));
}

// ─── Screen 2: Today ─────────────────────────────────────────
function TodayScreen() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      minHeight: "100%",
      background: "var(--paper)",
      padding: "64px 16px 100px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 6px 18px",
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline"
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow"
  }, "17 May \xB7 14:02 UTC"), /*#__PURE__*/React.createElement("h1", {
    className: "m-h1",
    style: {
      marginTop: 6
    }
  }, "Good afternoon,", /*#__PURE__*/React.createElement("br", null), "Aleia.")), /*#__PURE__*/React.createElement("div", {
    style: {
      width: 34,
      height: 34,
      borderRadius: "50%",
      background: "var(--citrate-green)",
      color: "var(--ink)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      fontFamily: "var(--font-sans)",
      fontWeight: 600,
      fontSize: 13
    }
  }, "AR")), /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow",
    style: {
      padding: "16px 6px 8px"
    }
  }, "Requires your witness \xB7 2"), /*#__PURE__*/React.createElement("div", {
    className: "m-card",
    style: {
      marginBottom: 10,
      borderLeft: "3px solid var(--ink)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "center",
      marginBottom: 10
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 10,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "var(--fg-3)"
    }
  }, "CTR\u20118B12 \xB7 US\u2011DE"), /*#__PURE__*/React.createElement("span", {
    className: "m-pill pending"
  }, /*#__PURE__*/React.createElement("span", {
    className: "dot"
  }), "Pending")), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 18,
      fontWeight: 440,
      color: "var(--ink)",
      lineHeight: 1.2,
      fontVariationSettings: '"opsz" 18'
    }
  }, "Hazardous materials manifest"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      color: "var(--fg-2)",
      marginTop: 4
    }
  }, "Meridian Chemical \xB7 $ 1.08 M \xB7 DOT 173"), /*#__PURE__*/React.createElement("button", {
    className: "m-btn m-btn-primary",
    style: {
      marginTop: 14,
      height: 44,
      fontSize: 14
    }
  }, /*#__PURE__*/React.createElement(NavIcon, {
    name: "check",
    size: 14
  }), " Witness now")), /*#__PURE__*/React.createElement("div", {
    className: "m-card",
    style: {
      marginBottom: 18,
      borderLeft: "3px solid var(--ink)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "center",
      marginBottom: 10
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 10,
      letterSpacing: ".14em",
      textTransform: "uppercase",
      color: "var(--fg-3)"
    }
  }, "CTR\u20112F44 \xB7 EU"), /*#__PURE__*/React.createElement("span", {
    className: "m-pill pending"
  }, /*#__PURE__*/React.createElement("span", {
    className: "dot"
  }), "Pending")), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 18,
      fontWeight: 440,
      color: "var(--ink)",
      lineHeight: 1.2
    }
  }, "EU procurement filing"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      color: "var(--fg-2)",
      marginTop: 4
    }
  }, "Aerostar Defence \xB7 $ 6.30 M \xB7 EU 2014/24"), /*#__PURE__*/React.createElement("button", {
    className: "m-btn m-btn-primary",
    style: {
      marginTop: 14,
      height: 44,
      fontSize: 14
    }
  }, /*#__PURE__*/React.createElement(NavIcon, {
    name: "check",
    size: 14
  }), " Witness now")), /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow",
    style: {
      padding: "8px 6px 8px"
    }
  }, "Tier 2 producer \xB7 today"), /*#__PURE__*/React.createElement("div", {
    className: "m-card",
    style: {
      marginBottom: 18
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline"
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow"
  }, "Earnings \xB7 today"), /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 28,
      marginTop: 6,
      color: "var(--ink)"
    }
  }, "$ 1,124.20")), /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: "right"
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow"
  }, "Facilities"), /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 18,
      marginTop: 6,
      color: "var(--ink)"
    }
  }, "6 / 9"))), /*#__PURE__*/React.createElement("div", {
    style: {
      height: 1,
      background: "var(--stone-200)",
      margin: "14px 0"
    }
  }), /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 11,
      color: "var(--fg-2)",
      lineHeight: 1.7
    }
  }, "BAYTOWN \xB7 $ 412 \xB7 28 inferences", /*#__PURE__*/React.createElement("br", null), "GUAM \xB7 $ 366 \xB7 22 inferences", /*#__PURE__*/React.createElement("br", null), "NAGOYA \xB7 $ 246 \xB7 18 inferences")), /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow",
    style: {
      padding: "8px 6px 8px"
    }
  }, "Recent activity"), [{
    id: "CTR‑9F4A",
    t: "Supply contract — FAR 52.246",
    w: "Witnessed",
    st: "settled",
    time: "12 min"
  }, {
    id: "CTR‑7AE0",
    t: "Routing instrument",
    w: "Settled",
    st: "settled",
    time: "2 h"
  }].map(a => /*#__PURE__*/React.createElement("div", {
    key: a.id,
    className: "m-card",
    style: {
      marginBottom: 8,
      padding: 14,
      display: "flex",
      justifyContent: "space-between",
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 10,
      letterSpacing: ".12em",
      textTransform: "uppercase",
      color: "var(--fg-3)"
    }
  }, a.id, " \xB7 ", a.time, " ago"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      color: "var(--ink)",
      marginTop: 2,
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap"
    }
  }, a.t)), /*#__PURE__*/React.createElement(NavIcon, {
    name: "chev",
    size: 14
  }))), /*#__PURE__*/React.createElement(MobileNav, {
    active: "today"
  }));
}

// ─── Screen 3: Filing detail (witnessing) ────────────────────
function FilingScreen() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      minHeight: "100%",
      background: "var(--paper)",
      padding: "64px 16px 120px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 6px 16px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "center",
      marginBottom: 12
    }
  }, /*#__PURE__*/React.createElement("a", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      color: "var(--ink)"
    }
  }, "\u2190 Today"), /*#__PURE__*/React.createElement("span", {
    className: "m-pill pending"
  }, /*#__PURE__*/React.createElement("span", {
    className: "dot"
  }), "Witness pending")), /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow"
  }, "CTR\u20118B12 \xB7 US\u2011DE \xB7 DOT 173"), /*#__PURE__*/React.createElement("h1", {
    className: "m-h1",
    style: {
      marginTop: 8,
      fontSize: 28
    }
  }, "Hazardous materials manifest"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      color: "var(--fg-2)",
      marginTop: 6
    }
  }, "Filed by Tom\xE1s Ribeiro \xB7 14 hours ago")), /*#__PURE__*/React.createElement("div", {
    className: "m-card",
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      gap: 14,
      marginBottom: 12
    }
  }, /*#__PURE__*/React.createElement(KV, {
    k: "Counterparty",
    v: "Meridian Chemical"
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Notional",
    v: "$ 1,084,200",
    mono: true
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Effective",
    v: "2026\u201105\u201115",
    mono: true
  }), /*#__PURE__*/React.createElement(KV, {
    k: "Custodian",
    v: "EY"
  })), /*#__PURE__*/React.createElement("div", {
    className: "m-card",
    style: {
      borderLeft: "3px solid var(--ink)",
      marginBottom: 14
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow"
  }, "Part I \u2014 Declaration"), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 14,
      lineHeight: 1.5,
      color: "var(--ink)",
      margin: "10px 0 0",
      fontVariationSettings: '"opsz" 14'
    }
  }, "The party named above (\"Counterparty\"), acting within the jurisdiction identified, hereby files this instrument for continuous attestation through the Citrate substrate.")), /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow",
    style: {
      padding: "12px 6px 10px"
    }
  }, "Witness lineage"), /*#__PURE__*/React.createElement("div", {
    className: "m-card",
    style: {
      position: "relative",
      padding: "16px 18px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      left: 26,
      top: 22,
      bottom: 22,
      width: 1,
      background: "var(--stone-200)"
    }
  }), [{
    who: "Tomás Ribeiro",
    w: "Filed",
    t: "2026‑05‑15 09:14",
    done: true,
    hash: "0x8e…a019"
  }, {
    who: "Kestrel Advisory LLP",
    w: "Witness 1",
    t: "2026‑05‑15 09:14",
    done: true,
    hash: "0xcc…7b21"
  }, {
    who: "Aria Compliance",
    w: "Witness 2",
    t: "2026‑05‑16 11:02",
    done: true,
    hash: "0x09…4f02"
  }, {
    who: "You",
    w: "Witness 3 of 3",
    t: "Awaiting",
    done: false
  }].map((l, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      display: "grid",
      gridTemplateColumns: "20px 1fr",
      gap: 12,
      paddingBottom: i === 3 ? 0 : 14
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 16,
      height: 16,
      borderRadius: "50%",
      background: l.done ? "var(--citrate-green)" : "var(--paper-pure)",
      border: l.done ? "1.5px solid var(--citrate-green-deep)" : "1.5px dashed var(--semantic-warning)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      position: "relative",
      zIndex: 1,
      marginTop: 2
    }
  }, l.done && /*#__PURE__*/React.createElement(NavIcon, {
    name: "check",
    size: 9
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 500,
      color: l.done ? "var(--ink)" : "var(--fg-2)"
    }
  }, l.who), /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 9.5,
      letterSpacing: ".06em",
      color: "var(--fg-3)"
    }
  }, l.t)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 12,
      color: "var(--fg-2)"
    }
  }, l.w), l.hash && /*#__PURE__*/React.createElement("div", {
    className: "m-mono",
    style: {
      fontSize: 10,
      color: "var(--fg-3)",
      marginTop: 2
    }
  }, l.hash))))), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "fixed",
      bottom: 0,
      left: 12,
      right: 12,
      padding: "16px 0 16px",
      background: "linear-gradient(to top, var(--paper) 60%, rgba(244,241,234,0))"
    }
  }, /*#__PURE__*/React.createElement("button", {
    className: "m-btn m-btn-primary"
  }, /*#__PURE__*/React.createElement(NavIcon, {
    name: "check",
    size: 16
  }), " Witness with Face ID")));
}
function KV({
  k,
  v,
  mono
}) {
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow",
    style: {
      fontSize: 9.5
    }
  }, k), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
      fontSize: mono ? 14 : 14,
      fontWeight: 500,
      color: "var(--ink)",
      marginTop: 4,
      fontFeatureSettings: '"tnum","lnum"'
    }
  }, v));
}

// ─── Screen 4: Biometric sheet ───────────────────────────────
function BiometricScreen() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      minHeight: "100%",
      position: "relative",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      opacity: 0.35,
      pointerEvents: "none"
    }
  }, /*#__PURE__*/React.createElement(FilingScreen, null)), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      inset: 0,
      background: "rgba(14,15,12,0.55)"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      left: 12,
      right: 12,
      bottom: 18,
      background: "var(--paper)",
      borderRadius: 24,
      border: "1px solid var(--stone-200)",
      padding: "28px 22px 24px",
      boxShadow: "0 -24px 56px -16px rgba(0,0,0,0.4)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 36,
      height: 4,
      background: "var(--stone-300)",
      borderRadius: 2,
      margin: "0 auto 22px"
    }
  }), /*#__PURE__*/React.createElement("div", {
    className: "m-eyebrow",
    style: {
      textAlign: "center"
    }
  }, "Confirm witness"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-display)",
      fontSize: 22,
      fontWeight: 440,
      color: "var(--ink)",
      textAlign: "center",
      lineHeight: 1.15,
      margin: "10px 0 4px",
      fontVariationSettings: '"opsz" 22'
    }
  }, "Sign filing CTR\u20118B12", /*#__PURE__*/React.createElement("br", null), "with Face ID"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      color: "var(--fg-2)",
      textAlign: "center",
      marginBottom: 22
    }
  }, "Hazardous materials manifest \xB7 $ 1.08 M \xB7 Meridian Chemical"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "center",
      marginBottom: 22
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 72,
      height: 72,
      borderRadius: 18,
      border: "2px solid var(--citrate-green-deep)",
      position: "relative",
      background: "rgba(142,204,9,0.08)"
    }
  }, /*#__PURE__*/React.createElement("svg", {
    viewBox: "0 0 72 72",
    style: {
      position: "absolute",
      inset: 0
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: "M14 26 L14 18 L22 18 M50 18 L58 18 L58 26 M14 46 L14 54 L22 54 M50 54 L58 54 L58 46",
    stroke: "var(--citrate-green-deep)",
    strokeWidth: "2",
    fill: "none",
    strokeLinecap: "round"
  }), /*#__PURE__*/React.createElement("path", {
    d: "M28 30 L28 33 M44 30 L44 33 M30 42 C32 45 40 45 42 42",
    stroke: "var(--citrate-green-deep)",
    strokeWidth: "2",
    fill: "none",
    strokeLinecap: "round"
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement("button", {
    className: "m-btn m-btn-primary"
  }, "Look at camera"), /*#__PURE__*/React.createElement("button", {
    className: "m-btn m-btn-ghost"
  }, "Use passcode instead"))));
}

// ─── Stage ──────────────────────────────────────────────────
function MobileStage() {
  return /*#__PURE__*/React.createElement("div", {
    className: "stage"
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "frame-label"
  }, "01 \xB7 Auth"), /*#__PURE__*/React.createElement("div", {
    className: "frame-shadow"
  }, /*#__PURE__*/React.createElement(IOSDevice, {
    dark: true
  }, /*#__PURE__*/React.createElement(AuthScreen, null)))), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "frame-label"
  }, "02 \xB7 Today"), /*#__PURE__*/React.createElement("div", {
    className: "frame-shadow"
  }, /*#__PURE__*/React.createElement(IOSDevice, null, /*#__PURE__*/React.createElement(TodayScreen, null)))), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "frame-label"
  }, "03 \xB7 Filing detail"), /*#__PURE__*/React.createElement("div", {
    className: "frame-shadow"
  }, /*#__PURE__*/React.createElement(IOSDevice, null, /*#__PURE__*/React.createElement(FilingScreen, null)))), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    className: "frame-label"
  }, "04 \xB7 Confirm witness"), /*#__PURE__*/React.createElement("div", {
    className: "frame-shadow"
  }, /*#__PURE__*/React.createElement(IOSDevice, null, /*#__PURE__*/React.createElement(BiometricScreen, null)))));
}
ReactDOM.createRoot(document.getElementById("root")).render(/*#__PURE__*/React.createElement(MobileStage, null));
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/mobile/MobileApp.jsx", error: String((e && e.message) || e) }); }

// ui_kits/mobile/ios-frame.jsx
try { (() => {
// iOS.jsx — Simplified iOS 26 (Liquid Glass) device frame
// Based on the iOS 26 UI Kit + Figma status bar spec. No assets, no deps.
// Exports: IOSDevice, IOSStatusBar, IOSNavBar, IOSGlassPill, IOSList, IOSListRow, IOSKeyboard

// ─────────────────────────────────────────────────────────────
// Status bar
// ─────────────────────────────────────────────────────────────
function IOSStatusBar({
  dark = false,
  time = '9:41'
}) {
  const c = dark ? '#fff' : '#000';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 154,
      alignItems: 'center',
      justifyContent: 'center',
      padding: '21px 24px 19px',
      boxSizing: 'border-box',
      position: 'relative',
      zIndex: 20,
      width: '100%'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      height: 22,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      paddingTop: 1.5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: '-apple-system, "SF Pro", system-ui',
      fontWeight: 590,
      fontSize: 17,
      lineHeight: '22px',
      color: c
    }
  }, time)), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      height: 22,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      gap: 7,
      paddingTop: 1,
      paddingRight: 1
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "19",
    height: "12",
    viewBox: "0 0 19 12"
  }, /*#__PURE__*/React.createElement("rect", {
    x: "0",
    y: "7.5",
    width: "3.2",
    height: "4.5",
    rx: "0.7",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "4.8",
    y: "5",
    width: "3.2",
    height: "7",
    rx: "0.7",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "9.6",
    y: "2.5",
    width: "3.2",
    height: "9.5",
    rx: "0.7",
    fill: c
  }), /*#__PURE__*/React.createElement("rect", {
    x: "14.4",
    y: "0",
    width: "3.2",
    height: "12",
    rx: "0.7",
    fill: c
  })), /*#__PURE__*/React.createElement("svg", {
    width: "17",
    height: "12",
    viewBox: "0 0 17 12"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M8.5 3.2C10.8 3.2 12.9 4.1 14.4 5.6L15.5 4.5C13.7 2.7 11.2 1.5 8.5 1.5C5.8 1.5 3.3 2.7 1.5 4.5L2.6 5.6C4.1 4.1 6.2 3.2 8.5 3.2Z",
    fill: c
  }), /*#__PURE__*/React.createElement("path", {
    d: "M8.5 6.8C9.9 6.8 11.1 7.3 12 8.2L13.1 7.1C11.8 5.9 10.2 5.1 8.5 5.1C6.8 5.1 5.2 5.9 3.9 7.1L5 8.2C5.9 7.3 7.1 6.8 8.5 6.8Z",
    fill: c
  }), /*#__PURE__*/React.createElement("circle", {
    cx: "8.5",
    cy: "10.5",
    r: "1.5",
    fill: c
  })), /*#__PURE__*/React.createElement("svg", {
    width: "27",
    height: "13",
    viewBox: "0 0 27 13"
  }, /*#__PURE__*/React.createElement("rect", {
    x: "0.5",
    y: "0.5",
    width: "23",
    height: "12",
    rx: "3.5",
    stroke: c,
    strokeOpacity: "0.35",
    fill: "none"
  }), /*#__PURE__*/React.createElement("rect", {
    x: "2",
    y: "2",
    width: "20",
    height: "9",
    rx: "2",
    fill: c
  }), /*#__PURE__*/React.createElement("path", {
    d: "M25 4.5V8.5C25.8 8.2 26.5 7.2 26.5 6.5C26.5 5.8 25.8 4.8 25 4.5Z",
    fill: c,
    fillOpacity: "0.4"
  }))));
}

// ─────────────────────────────────────────────────────────────
// Liquid glass pill — blur + tint + shine
// ─────────────────────────────────────────────────────────────
function IOSGlassPill({
  children,
  dark = false,
  style = {}
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      height: 44,
      minWidth: 44,
      borderRadius: 9999,
      position: 'relative',
      overflow: 'hidden',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      boxShadow: dark ? '0 2px 6px rgba(0,0,0,0.35), 0 6px 16px rgba(0,0,0,0.2)' : '0 1px 3px rgba(0,0,0,0.07), 0 3px 10px rgba(0,0,0,0.06)',
      ...style
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 9999,
      backdropFilter: 'blur(12px) saturate(180%)',
      WebkitBackdropFilter: 'blur(12px) saturate(180%)',
      background: dark ? 'rgba(120,120,128,0.28)' : 'rgba(255,255,255,0.5)'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 9999,
      boxShadow: dark ? 'inset 1.5px 1.5px 1px rgba(255,255,255,0.15), inset -1px -1px 1px rgba(255,255,255,0.08)' : 'inset 1.5px 1.5px 1px rgba(255,255,255,0.7), inset -1px -1px 1px rgba(255,255,255,0.4)',
      border: dark ? '0.5px solid rgba(255,255,255,0.15)' : '0.5px solid rgba(0,0,0,0.06)'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      zIndex: 1,
      display: 'flex',
      alignItems: 'center',
      padding: '0 4px'
    }
  }, children));
}

// ─────────────────────────────────────────────────────────────
// Navigation bar — glass pills + large title
// ─────────────────────────────────────────────────────────────
function IOSNavBar({
  title = 'Title',
  dark = false,
  trailingIcon = true
}) {
  const muted = dark ? 'rgba(255,255,255,0.6)' : '#404040';
  const text = dark ? '#fff' : '#000';
  const pillIcon = content => /*#__PURE__*/React.createElement(IOSGlassPill, {
    dark: dark
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 36,
      height: 36,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center'
    }
  }, content));
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      gap: 10,
      paddingTop: 62,
      paddingBottom: 10,
      position: 'relative',
      zIndex: 5
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      padding: '0 16px'
    }
  }, pillIcon(/*#__PURE__*/React.createElement("svg", {
    width: "12",
    height: "20",
    viewBox: "0 0 12 20",
    fill: "none",
    style: {
      marginLeft: -1
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: "M10 2L2 10l8 8",
    stroke: muted,
    strokeWidth: "2.5",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }))), trailingIcon && pillIcon(/*#__PURE__*/React.createElement("svg", {
    width: "22",
    height: "6",
    viewBox: "0 0 22 6"
  }, /*#__PURE__*/React.createElement("circle", {
    cx: "3",
    cy: "3",
    r: "2.5",
    fill: muted
  }), /*#__PURE__*/React.createElement("circle", {
    cx: "11",
    cy: "3",
    r: "2.5",
    fill: muted
  }), /*#__PURE__*/React.createElement("circle", {
    cx: "19",
    cy: "3",
    r: "2.5",
    fill: muted
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '0 16px',
      fontFamily: '-apple-system, system-ui',
      fontSize: 34,
      fontWeight: 700,
      lineHeight: '41px',
      color: text,
      letterSpacing: 0.4
    }
  }, title));
}

// ─────────────────────────────────────────────────────────────
// Grouped list (inset card, r:26) + row (52px)
// ─────────────────────────────────────────────────────────────
function IOSListRow({
  title,
  detail,
  icon,
  chevron = true,
  isLast = false,
  dark = false
}) {
  const text = dark ? '#fff' : '#000';
  const sec = dark ? 'rgba(235,235,245,0.6)' : 'rgba(60,60,67,0.6)';
  const ter = dark ? 'rgba(235,235,245,0.3)' : 'rgba(60,60,67,0.3)';
  const sep = dark ? 'rgba(84,84,88,0.65)' : 'rgba(60,60,67,0.12)';
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      minHeight: 52,
      padding: '0 16px',
      position: 'relative',
      fontFamily: '-apple-system, system-ui',
      fontSize: 17,
      letterSpacing: -0.43
    }
  }, icon && /*#__PURE__*/React.createElement("div", {
    style: {
      width: 30,
      height: 30,
      borderRadius: 7,
      background: icon,
      marginRight: 12,
      flexShrink: 0
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      color: text
    }
  }, title), detail && /*#__PURE__*/React.createElement("span", {
    style: {
      color: sec,
      marginRight: 6
    }
  }, detail), chevron && /*#__PURE__*/React.createElement("svg", {
    width: "8",
    height: "14",
    viewBox: "0 0 8 14",
    style: {
      flexShrink: 0
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: "M1 1l6 6-6 6",
    stroke: ter,
    strokeWidth: "2",
    fill: "none",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  })), !isLast && /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      bottom: 0,
      right: 0,
      left: icon ? 58 : 16,
      height: 0.5,
      background: sep
    }
  }));
}
function IOSList({
  header,
  children,
  dark = false
}) {
  const hc = dark ? 'rgba(235,235,245,0.6)' : 'rgba(60,60,67,0.6)';
  const bg = dark ? '#1C1C1E' : '#fff';
  return /*#__PURE__*/React.createElement("div", null, header && /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: '-apple-system, system-ui',
      fontSize: 13,
      color: hc,
      textTransform: 'uppercase',
      padding: '8px 36px 6px',
      letterSpacing: -0.08
    }
  }, header), /*#__PURE__*/React.createElement("div", {
    style: {
      background: bg,
      borderRadius: 26,
      margin: '0 16px',
      overflow: 'hidden'
    }
  }, children));
}

// ─────────────────────────────────────────────────────────────
// Device frame
// ─────────────────────────────────────────────────────────────
function IOSDevice({
  children,
  width = 402,
  height = 874,
  dark = false,
  title,
  keyboard = false
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      width,
      height,
      borderRadius: 48,
      overflow: 'hidden',
      position: 'relative',
      background: dark ? '#000' : '#F2F2F7',
      boxShadow: '0 40px 80px rgba(0,0,0,0.18), 0 0 0 1px rgba(0,0,0,0.12)',
      fontFamily: '-apple-system, system-ui, sans-serif',
      WebkitFontSmoothing: 'antialiased'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: 11,
      left: '50%',
      transform: 'translateX(-50%)',
      width: 126,
      height: 37,
      borderRadius: 24,
      background: '#000',
      zIndex: 50
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      zIndex: 10
    }
  }, /*#__PURE__*/React.createElement(IOSStatusBar, {
    dark: dark
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      height: '100%',
      display: 'flex',
      flexDirection: 'column'
    }
  }, title !== undefined && /*#__PURE__*/React.createElement(IOSNavBar, {
    title: title,
    dark: dark
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto'
    }
  }, children), keyboard && /*#__PURE__*/React.createElement(IOSKeyboard, {
    dark: dark
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      bottom: 0,
      left: 0,
      right: 0,
      zIndex: 60,
      height: 34,
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'flex-end',
      paddingBottom: 8,
      pointerEvents: 'none'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 139,
      height: 5,
      borderRadius: 100,
      background: dark ? 'rgba(255,255,255,0.7)' : 'rgba(0,0,0,0.25)'
    }
  })));
}

// ─────────────────────────────────────────────────────────────
// Keyboard — iOS 26 liquid glass
// ─────────────────────────────────────────────────────────────
function IOSKeyboard({
  dark = false
}) {
  const glyph = dark ? 'rgba(255,255,255,0.7)' : '#595959';
  const sugg = dark ? 'rgba(255,255,255,0.6)' : '#333';
  const keyBg = dark ? 'rgba(255,255,255,0.22)' : 'rgba(255,255,255,0.85)';

  // special-key icons
  const icons = {
    shift: /*#__PURE__*/React.createElement("svg", {
      width: "19",
      height: "17",
      viewBox: "0 0 19 17"
    }, /*#__PURE__*/React.createElement("path", {
      d: "M9.5 1L1 9.5h4.5V16h8V9.5H18L9.5 1z",
      fill: glyph
    })),
    del: /*#__PURE__*/React.createElement("svg", {
      width: "23",
      height: "17",
      viewBox: "0 0 23 17"
    }, /*#__PURE__*/React.createElement("path", {
      d: "M7 1h13a2 2 0 012 2v11a2 2 0 01-2 2H7l-6-7.5L7 1z",
      fill: "none",
      stroke: glyph,
      strokeWidth: "1.6",
      strokeLinejoin: "round"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M10 5l7 7M17 5l-7 7",
      stroke: glyph,
      strokeWidth: "1.6",
      strokeLinecap: "round"
    })),
    ret: /*#__PURE__*/React.createElement("svg", {
      width: "20",
      height: "14",
      viewBox: "0 0 20 14"
    }, /*#__PURE__*/React.createElement("path", {
      d: "M18 1v6H4m0 0l4-4M4 7l4 4",
      fill: "none",
      stroke: "#fff",
      strokeWidth: "1.8",
      strokeLinecap: "round",
      strokeLinejoin: "round"
    }))
  };
  const key = (content, {
    w,
    flex,
    ret,
    fs = 25,
    k
  } = {}) => /*#__PURE__*/React.createElement("div", {
    key: k,
    style: {
      height: 42,
      borderRadius: 8.5,
      flex: flex ? 1 : undefined,
      width: w,
      minWidth: 0,
      background: ret ? '#08f' : keyBg,
      boxShadow: '0 1px 0 rgba(0,0,0,0.075)',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      fontFamily: '-apple-system, "SF Compact", system-ui',
      fontSize: fs,
      fontWeight: 458,
      color: ret ? '#fff' : glyph
    }
  }, content);
  const row = (keys, pad = 0) => /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6.5,
      justifyContent: 'center',
      padding: `0 ${pad}px`
    }
  }, keys.map(l => key(l, {
    flex: true,
    k: l
  })));
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      zIndex: 15,
      borderRadius: 27,
      overflow: 'hidden',
      padding: '11px 0 2px',
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      boxShadow: dark ? '0 -2px 20px rgba(0,0,0,0.09)' : '0 -1px 6px rgba(0,0,0,0.018), 0 -3px 20px rgba(0,0,0,0.012)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 27,
      backdropFilter: 'blur(12px) saturate(180%)',
      WebkitBackdropFilter: 'blur(12px) saturate(180%)',
      background: dark ? 'rgba(120,120,128,0.14)' : 'rgba(255,255,255,0.25)'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      inset: 0,
      borderRadius: 27,
      boxShadow: dark ? 'inset 1.5px 1.5px 1px rgba(255,255,255,0.15)' : 'inset 1.5px 1.5px 1px rgba(255,255,255,0.7), inset -1px -1px 1px rgba(255,255,255,0.4)',
      border: dark ? '0.5px solid rgba(255,255,255,0.15)' : '0.5px solid rgba(0,0,0,0.06)',
      pointerEvents: 'none'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 20,
      alignItems: 'center',
      padding: '8px 22px 13px',
      width: '100%',
      boxSizing: 'border-box',
      position: 'relative'
    }
  }, ['"The"', 'the', 'to'].map((w, i) => /*#__PURE__*/React.createElement(React.Fragment, {
    key: i
  }, i > 0 && /*#__PURE__*/React.createElement("div", {
    style: {
      width: 1,
      height: 25,
      background: '#ccc',
      opacity: 0.3
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      textAlign: 'center',
      fontFamily: '-apple-system, system-ui',
      fontSize: 17,
      color: sugg,
      letterSpacing: -0.43,
      lineHeight: '22px'
    }
  }, w)))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      gap: 13,
      padding: '0 6.5px',
      width: '100%',
      boxSizing: 'border-box',
      position: 'relative'
    }
  }, row(['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p']), row(['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'], 20), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 14.25,
      alignItems: 'center'
    }
  }, key(icons.shift, {
    w: 45,
    k: 'shift'
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6.5,
      flex: 1
    }
  }, ['z', 'x', 'c', 'v', 'b', 'n', 'm'].map(l => key(l, {
    flex: true,
    k: l
  }))), key(icons.del, {
    w: 45,
    k: 'del'
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6,
      alignItems: 'center'
    }
  }, key('ABC', {
    w: 92.25,
    fs: 18,
    k: 'abc'
  }), key('', {
    flex: true,
    k: 'space'
  }), key(icons.ret, {
    w: 92.25,
    ret: true,
    k: 'ret'
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      height: 56,
      width: '100%',
      position: 'relative'
    }
  }));
}
Object.assign(window, {
  IOSDevice,
  IOSStatusBar,
  IOSNavBar,
  IOSGlassPill,
  IOSList,
  IOSListRow,
  IOSKeyboard
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/mobile/ios-frame.jsx", error: String((e && e.message) || e) }); }

__ds_ns.Button = __ds_scope.Button;

})();
