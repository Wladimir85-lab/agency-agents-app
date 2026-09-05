//! Tauri command surface. One sub-module per cluster of related commands.
//!
//! `lib.rs` re-exports these via `commands::*` and registers them in
//! `tauri::generate_handler![]`.

pub mod github;
pub mod knowledge;
pub mod settings;
pub mod updater;

// Re-export every command in flat form so `invoke_handler!` can take them.
pub use github::*;
pub use knowledge::*;
pub use settings::*;
pub use updater::*;
