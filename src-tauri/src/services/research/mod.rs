//! Deep-research agentic search (RFC 0037).
//!
//! Rust orchestrates a bounded loop over typed research primitives; the LLM is
//! called *inside* primitives and only parameterizes them. This module hosts the
//! pure core (dedup/diff/filter/budget), the seams (planner, candidate source,
//! clock), the loop, and the background manager.
//!
//! Built layer by layer; submodules land as the RFC is implemented.

// Pure-core APIs are consumed by the loop/manager layers that land next; the
// `allow(dead_code)` markers come off once those are wired.
#[allow(dead_code)]
pub mod agent;
#[allow(dead_code)]
pub mod budget;
#[allow(dead_code)]
pub mod clock;
pub mod controller;
#[allow(dead_code)]
pub mod dedup;
#[allow(dead_code)]
pub mod error;
#[allow(dead_code)]
pub mod filter;
pub mod manager;
#[allow(dead_code)]
pub mod planner;
pub mod reconciliation;
pub mod scheduler;
#[allow(dead_code)]
pub mod source;
