//! The app commands lane L4 registers (conversations).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L4 edits this file. A command added here is also allowed in
//! `permissions/l4-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l4 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            comms::groups_create,
            comms::comms_relay_status,
            comms::groups_list,
            comms::groups_self_address,
            comms::groups_join,
            comms::groups_add_member,
            comms::groups_roster,
            comms::groups_assign_role,
            comms::groups_offboard,
            comms::groups_send,
            comms::groups_messages,
        ])
    };
}
