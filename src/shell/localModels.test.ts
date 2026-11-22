// The webview's copy of the pinned onboarding models must match the Rust pins the download uses.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { LOCAL_MODELS, DISK_MARGIN_BYTES, diskTooSmall, etaText, speedText } from "./localModels";

const rust = readFileSync(resolve(__dirname, "../../src-tauri/src/model.rs"), "utf8");
const digits = (n: number) => n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, "_");

describe("LOCAL_MODELS mirrors model.rs", () => {
  it.each(Object.values(LOCAL_MODELS))("$key", (m) => {
    expect(rust).toContain(m.sha256);
    expect(rust).toContain(m.repo);
    if (m.key === "default") {
      expect(rust).toContain(`pub const MODEL_SIZE_BYTES: u64 = ${digits(m.sizeBytes)};`);
      expect(rust).toContain(`pub const MODEL_FILE: &str = "${m.file}";`);
    } else {
      expect(rust).toContain(`size_bytes: ${digits(m.sizeBytes)},`);
      expect(rust).toContain(`file: "${m.file}",`);
    }
  });

  it("the disk margin matches", () => {
    expect(rust).toContain("pub const DISK_MARGIN_BYTES: u64 = 256 << 20;");
    expect(DISK_MARGIN_BYTES).toBe(256 << 20);
  });
});

describe("helpers", () => {
  it("disk check counts what is already downloaded", () => {
    const need = LOCAL_MODELS.default.sizeBytes + DISK_MARGIN_BYTES;
    expect(diskTooSmall(null, "default")).toBe(false);
    expect(diskTooSmall(need - 1, "default")).toBe(true);
    expect(diskTooSmall(need - 1, "default", 10)).toBe(false);
  });
  it("speed and time left read plainly", () => {
    expect(speedText(12_500_000)).toBe("13 MB/s");
    expect(speedText(2_400_000)).toBe("2.4 MB/s");
    expect(speedText(40_000)).toBe("40 KB/s");
    expect(etaText(30)).toBe("less than a minute left");
    expect(etaText(3 * 3600 + 20 * 60)).toBe("about 3 h 20 min left");
  });
});
