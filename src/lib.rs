#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_ast;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

pub mod unbounded_storage;

dylint_linting::dylint_library!();

#[allow(clippy::no_mangle_with_rust_abi)]
#[no_mangle]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[unbounded_storage::UNBOUNDED_STORAGE_WRITES]);
    lint_store.register_late_pass(|_| Box::new(unbounded_storage::UnboundedStorage::default()));
}
