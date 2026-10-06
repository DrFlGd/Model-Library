//! Core of the Model Library desktop app, kept free of any GUI so it builds and
//! tests on its own (and powers `modlib-cli`, including `serve`: the app's
//! backend over HTTP, used by the tests now and by the Docker build later).
//!
//! - [`config`]: what the app keeps on this computer (open library, preferences)
//! - [`library`]: the portable library folder
//! - [`schema`]: schemas (categories), with their tree of subcategory folders, model folder names and fields
//! - [`model`]: one model folder and its model.json
//! - [`index`]: every model in the library, searched in memory, cached on this computer
//! - [`archive`], [`mesh`], [`thumb`]: inside ZIPs, 3D files as triangles, thumbnails
//! - [`import`]: proposing models from folders, moving or copying them in, and between categories
//! - [`sort`]: the sorting workspace: a folder tree read as it is, sorted into the library over several sittings
//! - [`relayout`]: renaming, merging and editing categories, with folders moved to match (journalled, undoable)
//! - [`api`]: the commands the page calls

pub mod api;
pub mod archive;
pub mod config;
pub mod docs;
pub mod import;
pub mod index;
pub mod library;
pub mod mesh;
pub mod model;
pub mod relayout;
pub mod schema;
pub mod sort;
pub mod thumb;

pub use library::Library;

/// The app's version. The repository keeps `<major>.<minor>.0`; release builds
/// get their number from CI (tools/set_version.py), one higher each release.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
