//! Framework-agnostic core for riki. Must not depend on axum; the `core-guard` otto task enforces it.

pub mod delete;
pub mod index;
pub mod move_page;
pub mod page;
pub mod path;
pub mod redirect;
pub mod render;
pub mod runtime;
pub mod save;
pub mod search;
pub mod slug;
pub mod store;
pub mod wiki;
pub mod write;

/// Commit and blob ids, as used throughout riki-core's API.
pub use git2::Oid;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
