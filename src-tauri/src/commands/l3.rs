//! The app commands lane L3 registers (shell, onboarding, navigation and notifications).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L3 edits this file. A command added here is also allowed in
//! `permissions/l3-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l3 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
        ])
    };
}
