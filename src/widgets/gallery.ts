// =====================================================================
// citrate-core — the widget gallery's starter templates (HUP-S10.3)
//
// Ready-made widgets a member can add in one click. Each one is plain HTML + inline JS that reads
// its data ONLY through `citrate.query(...)` for the queries it declares, and refreshes itself on a
// timer. They load nothing from the network (their document's CSP forbids it anyway).
// =====================================================================
import type { WidgetQuery } from "./catalog";

export interface WidgetTemplate {
  key: string;
  name: string;
  description: string;
  queries: WidgetQuery[];
  html: string;
}

const STYLE = `<style>.v{font-size:26px;font-weight:500;line-height:1.2}.s{font-size:11px;opacity:.65;margin-top:2px}.e{font-size:11px;color:#dd7259}</style>`;

/** A widget body that renders `render(data)` from one query every `everySec` seconds. */
function poller(query: WidgetQuery, render: string, everySec = 15): string {
  return `${STYLE}<div id="out"><div class="s">loading…</div></div>
<script>
(function () {
  var out = document.getElementById("out");
  function show(d) { ${render} }
  function tick() {
    citrate.query(${JSON.stringify(query)}).then(show, function (e) {
      out.innerHTML = "";
      var p = document.createElement("div"); p.className = "e"; p.textContent = "unavailable: " + e.message; out.appendChild(p);
    });
  }
  tick();
  setInterval(tick, ${everySec * 1000});
})();
</script>`;
}

/** Set `out` to a big value line and a small caption line, as text (never HTML). */
const twoLines = (value: string, caption: string) =>
  `out.innerHTML = ""; var a = document.createElement("div"); a.className = "v"; a.textContent = ${value}; var b = document.createElement("div"); b.className = "s"; b.textContent = ${caption}; out.appendChild(a); out.appendChild(b);`;

export const WIDGET_TEMPLATES: readonly WidgetTemplate[] = [
  {
    key: "block-height",
    name: "Block height",
    description: "Your node's current block height and sync state.",
    queries: ["node.status"],
    html: poller("node.status", twoLines(`Number(d.height).toLocaleString()`, `d.state + " · " + d.peers + " peers"`)),
  },
  {
    key: "salt-balance",
    name: "SALT at a glance",
    description: "Liquid, staked and claimable SALT (no address).",
    queries: ["wallet.summary"],
    html: poller(
      "wallet.summary",
      twoLines(`Number(d.liquidSalt).toLocaleString(undefined, { maximumFractionDigits: 2 }) + " SALT"`, `"staked " + Number(d.stakedSalt).toLocaleString() + " · claimable " + Number(d.claimableSalt).toLocaleString(undefined, { maximumFractionDigits: 2 })`),
      30,
    ),
  },
  {
    key: "active-model",
    name: "Hermes model",
    description: "The model Hermes is using right now.",
    queries: ["model.active"],
    html: poller("model.active", twoLines(`d.label`, `"active model"`), 30),
  },
  {
    key: "daemons",
    name: "Daemons",
    description: "How many daemons you have, and how many are running or out of budget.",
    queries: ["daemons.summary"],
    html: poller(
      "daemons.summary",
      twoLines(`d.total + (d.total === 1 ? " daemon" : " daemons")`, `(d.allPaused ? "all paused · " : "") + d.running + " running · " + d.paused + " paused · " + d.budgetUsedUp + " out of budget today"`),
    ),
  },
];
