//! Pantheon battler: an auto battler played on the MyHordes forum.
//! This library is the rules engine; the `pantheon-battler` binary adds the balance simulator
//! with bot players, and the `wasm` crate exposes the Arena challenge to the web page.

/// Appends a formatted line to an `Option<&mut Vec<String>>` log, if there is one.
macro_rules! say {
    ($log:expr, $($arg:tt)*) => {
        if let Some(l) = $log.as_deref_mut() {
            l.push(format!($($arg)*));
        }
    };
}

/// Version of the engine, shown with every challenge result.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "api")]
pub mod api;
pub mod arena;
pub mod bot;
pub mod challenge;
pub mod compo;
pub mod data;
pub mod fight;
pub mod forum;
pub mod game;
pub mod report;
pub mod shop;
pub mod state;
