//! Cloudflare Worker entry point and transport-independent Access validation.
//!
//! The native target contains the parser and policy tests. The Wasm target adds
//! the Workers runtime, D1 and Workers AI bindings.

#![forbid(unsafe_code)]

pub mod access_token;
pub mod build_policy;
pub mod list_pagination;
#[cfg(any(target_arch = "wasm32", test))]
mod public_cache;
#[cfg(any(target_arch = "wasm32", test))]
mod public_catalog;
pub mod public_media;
pub mod web_visibility;

#[cfg(target_arch = "wasm32")]
mod runtime;
