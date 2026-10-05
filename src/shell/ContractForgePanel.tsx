// =====================================================================
// citrate-core — the dApp forge panel (HUP-S6.2 / S6.3 / S6.9, US-6.4)
//
// Three steps, each a core command that answers honestly:
//   1. Toolchain: the member's switch for Hermes's forge, slither, aderyn and medusa tools
//      (off by default; applies when Hermes next starts), and which programs were found.
//   2. Start from a template: an OpenZeppelin-Wizard-style form for ERC-20, ERC-721 (and the
//      Solady variant), ERC-1155, Governor and hello mint. Core validates every value and writes
//      the project only inside a folder granted for writing.
//   3. Deploy gate: core reads a Hermes session's toolchain reports for one built contract,
//      parses them itself, holds Medusa to this machine's tier budget, and shows READY or
//      NOT READY. Nothing here signs or deploys; a deploy still stops at the Signature Ceremony.
// =====================================================================
import { useEffect, useState, type CSSProperties } from "react";
import type { ContractsDomain } from "../bridge/domains";
import {
  initialValues,
  medusaLine,
  renderParams,
  toolchainGateRequest,
  type TemplateCatalog,
  type TemplateRenderView,
  type ToolchainGateResult,
  type ToolchainStatus,
} from "../agent/contractForge";
import { DeployGateCard } from "./DeployGateCard";

export type ContractForgeOps = Pick<
  ContractsDomain,
  "templateList" | "templateRender" | "toolchainSettings" | "toolchainSetEnabled" | "gateFromToolchain"
>;

const note: CSSProperties = { fontSize: 11.5, lineHeight: 1.6, color: "var(--tx-2)" };
const head: CSSProperties = { fontSize: 12.5, fontWeight: 500 };
const mono: CSSProperties = { fontFamily: "var(--font-mono)", fontSize: 11 };
const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function ContractForgePanel({ ops }: { ops: ContractForgeOps }) {
  const [tc, setTc] = useState<ToolchainStatus | null>(null);
  const [tcErr, setTcErr] = useState<string | null>(null);
  const [cat, setCat] = useState<TemplateCatalog | null>(null);
  const [catErr, setCatErr] = useState<string | null>(null);
  const [tid, setTid] = useState("erc20");
  const [values, setValues] = useState<Record<string, string>>({});
  const [outDir, setOutDir] = useState("");
  const [rendered, setRendered] = useState<TemplateRenderView | null>(null);
  const [renderErr, setRenderErr] = useState<string | null>(null);
  const [session, setSession] = useState("");
  const [project, setProject] = useState("");
  const [artifact, setArtifact] = useState("");
  const [gate, setGate] = useState<ToolchainGateResult | null>(null);
  const [gateErr, setGateErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    ops.toolchainSettings().then((s) => live && setTc(s), (e) => live && setTcErr(msg(e)));
    ops.templateList().then(
      (c) => {
        if (!live) return;
        setCat(c);
        const first = c.templates.find((t) => t.id === "erc20") ?? c.templates[0];
        if (first) {
          setTid(first.id);
          setValues(initialValues(first));
        }
      },
      (e) => live && setCatErr(msg(e)),
    );
    return () => {
      live = false;
    };
  }, [ops]);

  const tmpl = cat?.templates.find((t) => t.id === tid) ?? null;

  const toggle = async () => {
    if (!tc) return;
    setBusy(true);
    try {
      setTc(await ops.toolchainSetEnabled(!tc.settings.enabled));
      setTcErr(null);
    } catch (e) {
      setTcErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  const render = async () => {
    if (!tmpl) return;
    const { params, missing } = renderParams(tmpl, values);
    if (missing.length) {
      setRenderErr(`Fill in: ${missing.join(", ")}.`);
      return;
    }
    setBusy(true);
    setRenderErr(null);
    setRendered(null);
    try {
      const r = await ops.templateRender({ template: tmpl.id, params, outDir: outDir.trim() });
      setRendered(r);
      setProject(r.contractDir);
      const contract = r.params.contract;
      if (contract) setArtifact(`Token.sol/${contract}.json`);
    } catch (e) {
      setRenderErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  const runGate = async () => {
    setBusy(true);
    setGateErr(null);
    setGate(null);
    try {
      setGate(await ops.gateFromToolchain(toolchainGateRequest(session, project, artifact)));
    } catch (e) {
      setGateErr(msg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="surface" data-testid="contract-forge" style={{ display: "flex", flexDirection: "column", padding: "12px 16px", gap: 8 }}>
      <span style={{ fontSize: 13.5, fontWeight: 500 }}>Build a contract</span>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={head}>1. Toolchain</span>
        {tcErr && <span data-testid="cf-tc-error" style={{ ...note, color: "var(--danger)" }}>{tcErr}</span>}
        {tc && (
          <>
            <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
              <input type="checkbox" data-testid="cf-tc-toggle" checked={tc.settings.enabled} disabled={busy} onChange={() => void toggle()} />
              Let Hermes run forge, slither, aderyn and medusa in folders you granted (applies when Hermes next starts)
            </label>
            <ul style={{ margin: 0, paddingLeft: 18 }}>
              {tc.programs.map((p) => (
                <li key={p.program} data-testid={"cf-tc-" + p.program} style={{ ...note, color: p.path ? "var(--tx-2)" : "var(--danger)" }}>
                  {p.program}: {p.path ? "found" : "not installed (its gate item fails)"}
                </li>
              ))}
            </ul>
            {tc.notices.map((n, i) => (
              <span key={i} style={note}>{n}</span>
            ))}
          </>
        )}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6, borderTop: "1px solid var(--line-1)", paddingTop: 8 }}>
        <span style={head}>2. Start from a template</span>
        {catErr && <span data-testid="cf-cat-error" style={{ ...note, color: "var(--danger)" }}>{catErr}</span>}
        {cat && (
          <>
            <select
              className="input"
              data-testid="cf-template"
              value={tid}
              onChange={(e) => {
                setTid(e.target.value);
                const t = cat.templates.find((x) => x.id === e.target.value);
                if (t) setValues(initialValues(t));
                setRendered(null);
              }}
            >
              {cat.templates.map((t) => (
                <option key={t.id} value={t.id}>{t.title}</option>
              ))}
            </select>
            {tmpl && <span style={note}>{tmpl.description}</span>}
            {tmpl?.fields.map((f) => (
              <label key={f.key} style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                <span style={{ fontSize: 11.5 }}>
                  {f.label}
                  {f.required ? "" : " (optional)"}
                </span>
                <input
                  className="input"
                  data-testid={"cf-field-" + f.key}
                  value={values[f.key] ?? ""}
                  placeholder={f.default ?? ""}
                  onChange={(e) => setValues({ ...values, [f.key]: e.target.value })}
                  style={mono}
                />
                <span style={{ ...note, fontSize: 10.5 }}>
                  {f.help}
                  {f.min != null && f.max != null ? ` From ${f.min} to ${f.max}.` : ""}
                </span>
              </label>
            ))}
            <input className="input" data-testid="cf-outdir" placeholder="New folder inside a folder you granted for writing, e.g. /Users/you/dapps/lemon" value={outDir} onChange={(e) => setOutDir(e.target.value)} style={mono} />
            <span style={note}>
              Tier {cat.tier}: Medusa runs {cat.medusaBudget.test_limit.toLocaleString("en-US")} calls and needs {cat.medusaBudget.min_coverage_pct}% line coverage (starting values, pending owner sign-off).
            </span>
            <button className="btn btn-sm btn-secondary" data-testid="cf-render" disabled={busy || !outDir.trim()} onClick={() => void render()}>
              Create project
            </button>
            {renderErr && <span data-testid="cf-render-error" style={{ ...note, color: "var(--danger)" }}>{renderErr}</span>}
            {rendered && (
              <span data-testid="cf-rendered" style={{ ...mono, whiteSpace: "pre-wrap" }}>
                {`Wrote ${rendered.files.length} files to ${rendered.outDir} (${rendered.template}, tier ${rendered.tier}).`}
                {rendered.deps.some((d) => !d.installed)
                  ? `\nLibraries not installed yet: ${rendered.deps.filter((d) => !d.installed).map((d) => `${d.name} (${d.note ?? "missing"})`).join("; ")}.`
                  : "\nPinned libraries copied into lib/."}
              </span>
            )}
          </>
        )}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6, borderTop: "1px solid var(--line-1)", paddingTop: 8 }}>
        <span style={head}>3. Deploy gate from Hermes's toolchain runs</span>
        <span style={note}>
          After Hermes ran forge_test, slither_scan, aderyn_scan and medusa_fuzz on the project, core reads their raw reports and decides. A fork dry run is still required for READY.
        </span>
        <input className="input" data-testid="cf-session" placeholder="Hermes session id" value={session} onChange={(e) => setSession(e.target.value)} style={mono} />
        <input className="input" data-testid="cf-project" placeholder="Project folder" value={project} onChange={(e) => setProject(e.target.value)} style={mono} />
        <input className="input" data-testid="cf-artifact" placeholder="Artifact under out/, e.g. Token.sol/LemonDrops.json" value={artifact} onChange={(e) => setArtifact(e.target.value)} style={mono} />
        <button className="btn btn-sm btn-secondary" data-testid="cf-gate" disabled={busy || !session.trim() || !project.trim() || !artifact.trim()} onClick={() => void runGate()}>
          Run the deploy gate
        </button>
        {gateErr && <span data-testid="cf-gate-error" style={{ ...note, color: "var(--danger)" }}>{gateErr}</span>}
        {gate && (
          <>
            <DeployGateCard record={gate.record} initcodeHash={gate.record.initcodeHash} />
            <span data-testid="cf-medusa" style={note}>{medusaLine(gate.medusa)}</span>
          </>
        )}
      </div>
    </div>
  );
}
