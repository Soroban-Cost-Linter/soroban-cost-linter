#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

pub mod nested_storage_iteration;

dylint_linting::dylint_library!();

#[allow(clippy::no_mangle_with_rust_abi)]
#[no_mangle]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[nested_storage_iteration::NESTED_STORAGE_ITERATION]);
    lint_store.register_late_pass(|_| {
        Box::new(nested_storage_iteration::NestedStorageIteration::default())
    });
}
