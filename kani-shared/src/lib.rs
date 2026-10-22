//! Contracts shared by Kani's native host and WASM extension guests.
//!
//! Feature gates keep guest builds free of host-only serialization and runtime dependencies. The
//! WIT package is the authoritative component boundary; the Rust APIs provide domain types,
//! extension traits, request construction, extraction unpacking, and safe wrappers around it.

pub mod types;
pub use types::*;

/// Generated Rust bindings for the `kani-extension` WIT world.
///
/// Host builds add Serde derives to shared WIT values; guest builds retain the dependency-minimal
/// component bindings.
pub mod bindings {
    // When the `host` feature is enabled (kani-app, kani-core) the generated WIT types get serde
    // derives so the service layer can serialize/deserialize them for the JSON cache. WASM
    // extension crates never enable `host`, so they compile without any serde dependency.
    #[cfg(feature = "host")]
    wit_bindgen::generate!({
        path: "../kani-core/wit/kani.wit",
        world: "kani-extension",
        additional_derives: [serde::Serialize, serde::Deserialize],
        pub_export_macro: true,
        default_bindings_module: "kani_shared::bindings",
        with: {
            "kani:extension/types/manga-status": crate::types::MangaStatus
        }
    });

    #[cfg(not(feature = "host"))]
    wit_bindgen::generate!({
        path: "../kani-core/wit/kani.wit",
        world: "kani-extension",
        pub_export_macro: true,
        default_bindings_module: "kani_shared::bindings",
        with: {
            "kani:extension/types/manga-status": crate::types::MangaStatus
        }
    });
}

pub use bindings::kani::extension::types as wit_types;
pub use bindings::kani::extension::{html, http, scripting, utility};

#[cfg(target_family = "wasm")]
pub use wit_bindgen::{StreamReader, StreamResult, spawn_local};

#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
pub use serde_json;

pub mod extension;
pub use extension::*;

pub mod host_abi;

pub mod encoding;
pub use encoding::{decode_manga_id, encode_manga_id};

pub mod ast;
pub use ast::{OffsetType, PaginationConfig};

pub mod filters;
pub mod request;
pub mod unpack;
pub use filters::{ApplyFilters, ArrayFormat, FilterGroups};

#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
pub mod filter_fetch;
#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
pub use filter_fetch::FilterFetchDef;

#[cfg(target_family = "wasm")]
pub use talc::TalckWasm as __TalckWasm;

#[macro_export]
/// Installs Kani's WASM allocator for an extension guest.
///
/// Invoke this once at crate scope. It expands to a global allocator only for a WASM target and
/// has no effect in native builds.
macro_rules! guest_alloc {
    () => {
        #[cfg(target_family = "wasm")]
        #[global_allocator]
        static ALLOCATOR: $crate::__TalckWasm = unsafe { $crate::__TalckWasm::new_global() };
    };
}
