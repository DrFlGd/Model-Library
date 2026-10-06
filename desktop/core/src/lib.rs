//! Core of the Model Library desktop app, kept free of any GUI so it builds and
//! tests on its own (and powers `modlib-cli`, including `serve`: the app's
//! backend over HTTP, used by the tests now and by the Docker build later).
//!
//! - [`config`]: what the app keeps on this computer (open library, preferences)
//! - [`library`]: the portable library folder
//! - [`api`]: the commands the page calls

pub mod api;
pub mod config;
pub mod library;

pub use library::Library;

/// The app's version. The repository keeps `<major>.<minor>.0`; release builds
/// get their number from CI (tools/set_version.py), one higher each release.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
