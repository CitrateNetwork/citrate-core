// =====================================================================
// citrate-core — group role changes (#495).
//
// GC-2: a role change is a `grantRole` request. The 0.6.0 comms-member-daemon signs the grant
// with this device's comms key and every member checks it. `src-tauri/src/comms.rs`
// (`groups_assign_role`) uses the same older-daemon reason; groupsAssignRole.test.tsx keeps the
// two in step.
// =====================================================================

/** The member-facing reason an older daemon can't change roles, word for word the Rust
 *  `ROLE_GRANTS_NEED_NEWER_DAEMON`. */
export const ROLE_CHANGES_NEED_NEWER_DAEMON = "Role changes need the 0.6.0 comms daemon; the one this app runs is older.";

// Who may change whose role is `roleActions` in groups/memberRoles.ts (the daemon's rule: owners and
// admins assign roles, only the owner makes or unmakes an admin).
