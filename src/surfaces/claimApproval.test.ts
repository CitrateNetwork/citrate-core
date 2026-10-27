// Re-review: an approval whose invite isn't confirmed closed at the relay doesn't add the claimant
// yet (the app adds them once the relay confirms); an invalid one adds nobody.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { approvalStep } from "./claimApproval";

describe("approvalStep", () => {
  it("adds now only when approved", () => {
    expect(approvalStep("approved").add).toBe(true);
    expect(approvalStep("pending").add).toBe(false);
    expect(approvalStep("invalid").add).toBe(false);
  });

  it("says a pending approval waits for the relay", () => {
    expect(approvalStep("pending").toast).toBe("Approved. They'll be added once the relay confirms the invite is closed.");
    expect(approvalStep("invalid").toast).toBe("That request's invite is invalid or already used.");
  });

  it("Groups adds a claimant only behind approvalStep", () => {
    const src = readFileSync(join(__dirname, "Groups.tsx"), "utf8");
    expect(src.match(/verifyConsume\(/g)?.length).toBe(2);
    expect(src.match(/if \(step\.add\) void addMemberToGroup\(/g)?.length).toBe(2);
  });
});
