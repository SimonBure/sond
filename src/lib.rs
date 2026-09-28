//! Sond: append-only research logs, stored as plain Markdown next to the code.

pub mod chunk;
pub mod clock;
pub mod editor;
#[cfg(feature = "ask")]
pub mod embed;
pub mod index;
pub mod log;
pub mod template;
