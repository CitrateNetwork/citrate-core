// 0.6.0 L3-WP00 characterization: what app quit does today (citrate-core a826e441), pinned before
// L3-WP16 (tray / menu-bar agent, close-to-tray) changes it. A failure here means the quit path
// changed: update the expectation only together with that reviewed change.
//
//   1. The app's run loop calls shutdown_all_sidecars on ExitRequested and on Exit, and does
//      nothing else; shutdown_all_sidecars stops each sidecar in today's order, every managed-state
//      block stopping its own supervisor.
//   2. Nothing intercepts a quit or the main window's close: closing the window ends the app.
//
// The Tauri run loop cannot run in a unit test, so the pins read the real Rust sources. They live
// here, in an L3-owned path, rather than in src-tauri (whose test files and lib.rs are L0's).
// Each source check is also shown failing on the regressions it exists to catch.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";

const LIB = readFileSync("src-tauri/src/lib.rs", "utf8");
const POPOUT = readFileSync("src-tauri/src/popout.rs", "utf8");

const RUN_OPEN = ".run(|app_handle, event| {";
const SHUTDOWN_OPEN = "pub(crate) fn shutdown_all_sidecars(app: &tauri::AppHandle) {";

/** Rust source with whole-line `//` comments dropped and whitespace collapsed. */
const flatRust = (src: string) => src.replace(/^[ \t]*\/\/.*$/gm, " ").replace(/\s+/g, " ").trim();

/** Today's run loop handler body, flattened. */
const RUN_TODAY = "if matches!( event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit ) { shutdown_all_sidecars(app_handle); }";

/** Today's shutdown order: the importer, each managed supervisor (its own stop), the lazy daemons. */
const MANAGED = ["node::NodeState", "agent::AgentState", "memory::MemoryState", "serve::ServeState", "ipfs::IpfsState"];
const SHUTDOWN_TODAY = [
  "use tauri::Manager;",
  "knowledge_import::shutdown();",
  ...MANAGED.map((t) => `if let Some(s) = app.try_state::<${t}>() { s.0.stop(); }`),
  "comms::shutdown();",
  "cluster::shutdown();",
  "hermes::shutdown();",
].join(" ");

/** Problems with the quit path in a copy of lib.rs (empty when it matches today's). */
function quitPathProblems(lib: string): string[] {
  const problems: string[] = [];
  if (lib.split(RUN_OPEN).length !== 2) return ["lib.rs must have exactly one run loop handler"];
  const handler = flatRust(lib.split(RUN_OPEN)[1].split("\n        });\n")[0]);
  if (handler !== RUN_TODAY) problems.push("run handler: " + handler);
  if (lib.split(SHUTDOWN_OPEN).length !== 2) return [...problems, "lib.rs must define shutdown_all_sidecars once"];
  const body = flatRust(lib.split(SHUTDOWN_OPEN)[1].split("\n}\n")[0]);
  if (body !== SHUTDOWN_TODAY) problems.push("shutdown_all_sidecars: " + body);
  return problems;
}

describe("L3-WP00 characterization: app quit (src-tauri lib.rs)", () => {
  it("exit_requested_stops_all_sidecars", () => {
    expect(quitPathProblems(LIB)).toEqual([]);
  });

  it("quit_path_check_catches_a_lost_stop_or_a_moved_call", () => {
    const nodeBlock = "try_state::<node::NodeState>() {\n        s.0.stop();\n    }";
    const agentBlock = "try_state::<agent::AgentState>() {\n        s.0.stop();\n    }";
    const call = "            ) {\n                shutdown_all_sidecars(app_handle);\n            }";
    const mutants = [
      // The node block loses its stop while another block stops twice (the total count is unchanged).
      LIB.replace(nodeBlock, "try_state::<node::NodeState>() {\n    }").replace(
        agentBlock,
        "try_state::<agent::AgentState>() {\n        s.0.stop();\n        s.0.stop();\n    }",
      ),
      // The shutdown call moves out of the exit branch.
      LIB.replace(call, "            ) {\n            }\n            shutdown_all_sidecars(app_handle);"),
      // Exit alone, ExitRequested no longer handled.
      LIB.replace("tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit", "tauri::RunEvent::Exit"),
      // Two sidecars swap their stop order.
      LIB.replace("    comms::shutdown();\n    cluster::shutdown();", "    cluster::shutdown();\n    comms::shutdown();"),
    ];
    for (const m of mutants) {
      expect(m).not.toBe(LIB);
      expect(quitPathProblems(m).length).toBeGreaterThan(0);
    }
  });

  it("no_window_close_or_exit_is_intercepted", () => {
    for (const [name, src] of [
      ["lib.rs", LIB],
      ["popout.rs", POPOUT],
    ] as const) {
      expect(src, `${name} intercepts a close`).not.toContain("prevent_close");
      expect(src, `${name} intercepts a quit`).not.toContain("prevent_exit");
    }
    // The only main-window hook closes the pop-outs when the main window is destroyed.
    expect(LIB.split("on_window_event").length - 1).toBe(0);
    expect(POPOUT).toContain("if matches!(event, WindowEvent::Destroyed) {");
  });
});
