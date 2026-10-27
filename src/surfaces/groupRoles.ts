// =====================================================================
// citrate-core — group role changes (#495).
//
// The pinned comms-member-daemon accepts a role change only with a signed RoleAssertion, and no
// part of core or the daemon signs one yet. Until a signer exists, Assign role is shown as not
// available with this reason, and no request is sent. `src-tauri/src/comms.rs`
// (`groups_assign_role`) returns the same reason; groupsAssignRole.test.tsx keeps the two in step.
// =====================================================================

/** Whether a member's role can be changed in this build. */
export const ROLE_CHANGES_AVAILABLE: boolean = false;

/** The member-facing reason, word for word the Rust `ROLE_GRANTS_NOT_AVAILABLE`. */
export const ROLE_CHANGES_NOT_AVAILABLE = "Role changes aren't available yet: signed role grants aren't built.";
