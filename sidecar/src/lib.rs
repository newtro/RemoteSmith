//! RemoteSmith tray: presence and connect shortcuts for the RemoteSmith RustDesk fork.
//!
//! Everything except [`tray`] is OS-UI-free and unit tested; `tray` only renders the
//! [`menu_model::MenuModel`] built from the IPC client's [`state::AppState`].

pub mod autostart;
pub mod client;
pub mod fake_fork;
pub mod logging;
pub mod menu_model;
pub mod platform;
pub mod protocol;
pub mod readiness;
pub mod state;
pub mod transport;
pub mod tray;
#[cfg(windows)]
pub mod winsec;
