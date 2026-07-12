// Loads this design system into the template. In a consuming project, point
// base at the bound DS folder relative to this file (e.g. '_ds/<folder>' at
// the project root, '../_ds/<folder>' one level down) — one line to edit.
(() => {
  const base = '_ds/citrate-design-system-244c4cdf-0fbf-406e-9d19-bc9db7d7b48a';
  for (const p of ["colors_and_type.css","pitch_deck/colors_and_type.css","preview/_card.css","ui_kits/console/colors_and_type.css","ui_kits/console/kit.css","ui_kits/marketing/colors_and_type.css","ui_kits/marketing/kit.css","ui_kits/mobile/colors_and_type.css","ui_kits/mobile/kit.css","ui_kits/tablet/colors_and_type.css"]) {
    const l = document.createElement('link');
    l.rel = 'stylesheet'; l.href = base + '/' + p;
    document.head.appendChild(l);
  }
  const s = document.createElement('script');
  s.src = base + '/_ds_bundle.js';
  s.onerror = () => console.error('ds-base.js: failed to load ' + s.src + ' — if this is a consuming project, point the base line in ds-base.js at the bound _ds/<folder> tree relative to this page (e.g. _ds/<folder> at the project root, ../_ds/<folder> one level down); in a fresh design system this can just mean the bundle is not compiled yet');
  document.head.appendChild(s);
})();
