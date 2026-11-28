// L9 route and surface declarations (planset v060, lanes/L0_INTEGRATION_SEAMS.md 3.5, L0-WP08).
// L9 owns this file (.agentile/v60-ownership.map); no other lane edits it. A screen is declared
// with registerSurfaces([{ route, lane: "L9", render, tab?, register? }]) from ./registry, which
// refuses a built-in, taken or sub-strip route (registry.test.ts fails any lane but L9 here). render
// runs as a component body, so it may call hooks. A tab with no content yet renders NotAvailableYet
// (Rule 1); never import ../PlaceView here (it loads this file). In dev, a hot reload of this file
// registers its routes again and the registry refuses them as taken: reload the page. No routes yet.
export {};
