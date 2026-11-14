#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

pub mod confidence;
pub mod config;

use confidence::{Confidence, LintMetadata};
use config::LinterConfig;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, declare_lint_pass};

dylint_linting::dylint_library!();

declare_lint! {
    /// **What it does:** Detects iteration constructs that perform mutable ledger storage operations
    /// where the loop bounds are not statically determinable.
    pub UNBOUNDED_STORAGE_WRITES,
    Warn,
    "unbounded storage writes inside loops (High confidence)"
}

declare_lint! {
    /// **What it does:** Flags nested loops (`depth >= 2`) that perform ledger read or write operations.
    pub NESTED_STORAGE_ITERATION,
    Warn,
    "nested iteration over persistent storage (High confidence)"
}

declare_lint! {
    /// **What it does:** Identifies repeated invocations of Soroban host functions inside loops.
    pub EXCESSIVE_HOST_CALLS,
    Allow,
    "excessive Soroban host-function calls inside loops (Medium confidence)"
}

declare_lint! {
    /// **What it does:** Warns against creating excessively large vectors, maps, or byte arrays in a single scope.
    pub LARGE_MEMORY_ALLOCATION,
    Allow,
    "large memory allocation in a single contract scope (Low confidence)"
}

/// Registry of all Soroban cost lints paired with their confidence metadata.
pub const REGISTERED_LINTS: &[LintMetadata] = &[
    LintMetadata::new(
        "unbounded_storage_writes",
        UNBOUNDED_STORAGE_WRITES,
        Confidence::High,
    ),
    LintMetadata::new(
        "nested_storage_iteration",
        NESTED_STORAGE_ITERATION,
        Confidence::High,
    ),
    LintMetadata::new(
        "excessive_host_calls",
        EXCESSIVE_HOST_CALLS,
        Confidence::Medium,
    ),
    LintMetadata::new(
        "large_memory_allocation",
        LARGE_MEMORY_ALLOCATION,
        Confidence::Low,
    ),
];

#[derive(Default)]
pub struct ConfiguredCostPass {
    config: LinterConfig,
}

impl ConfiguredCostPass {
    pub fn new() -> Self {
        Self {
            config: LinterConfig::global().clone(),
        }
    }

    pub fn config(&self) -> &LinterConfig {
        &self.config
    }
}

declare_lint_pass!(
    SorobanCostRegistry => [
        UNBOUNDED_STORAGE_WRITES,
        NESTED_STORAGE_ITERATION,
        EXCESSIVE_HOST_CALLS,
        LARGE_MEMORY_ALLOCATION
    ]
);

impl<'tcx> LateLintPass<'tcx> for SorobanCostRegistry {
    fn check_crate(&mut self, _cx: &LateContext<'tcx>) {
        let _ = LinterConfig::global();
    }
}

#[allow(clippy::no_mangle_with_rust_abi)]
#[no_mangle]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    let _cfg = LinterConfig::global();
    lint_store.register_lints(&[
        UNBOUNDED_STORAGE_WRITES,
        NESTED_STORAGE_ITERATION,
        EXCESSIVE_HOST_CALLS,
        LARGE_MEMORY_ALLOCATION,
    ]);
    lint_store.register_late_pass(|_| Box::new(SorobanCostRegistry));
}
