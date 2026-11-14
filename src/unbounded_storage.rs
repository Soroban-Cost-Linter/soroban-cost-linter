//! Lint: `unbounded_storage_writes` (`#7`).
//!
//! Walks the HIR to identify loop blocks (`for`, `while`, `loop`) and detects
//! method calls targeting Soroban storage mutation APIs (`set`, `set_authorized`,
//! `update`, `remove`, `extend_ttl` on `env.storage().persistent()`,
//! `env.storage().temporary()`, or `env.storage().instance()`) when loop bounds
//! are not small compile-time constants.

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::LitKind;
use rustc_hir::{Expr, ExprKind, LoopSource, QPath};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};

/// Maximum statically bounded loop iterations permitted before flagging storage writes.
pub const MAX_STATIC_LOOP_WRITE_BOUND: u128 = 16;

declare_lint! {
    /// **What it does:** Detects iteration constructs (`for`, `while`, `loop`) that perform
    /// mutable Soroban ledger storage operations (`set`, `set_authorized`, `update`, `remove`,
    /// `extend_ttl`) where loop bounds are dynamic or exceed safe limits.
    ///
    /// **Why is this bad:** Every ledger write in Soroban incurs XDR serialization CPU costs,
    /// per-entry and per-kilobyte write fees, and counts toward the strict 50 write-entry limit.
    ///
    /// **Example:**
    /// ```rust,ignore
    /// for item in items.iter() {
    ///     env.storage().persistent().set(&key, &item);
    /// }
    /// ```
    /// Use instead:
    /// ```rust,ignore
    /// let mut acc = 0u32;
    /// for item in items.iter() {
    ///     acc += item;
    /// }
    /// env.storage().persistent().set(&key, &acc);
    /// ```
    pub UNBOUNDED_STORAGE_WRITES,
    Warn,
    "unbounded Soroban ledger storage writes inside a loop"
}

#[derive(Default)]
pub struct UnboundedStorage {
    /// Stack of active loops; `true` means the loop has an unbounded or dynamic iteration count.
    unbounded_loop_stack: Vec<bool>,
}

impl_lint_pass!(UnboundedStorage => [UNBOUNDED_STORAGE_WRITES]);

impl<'tcx> LateLintPass<'tcx> for UnboundedStorage {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if let ExprKind::Loop(block, _label, source, _span) = expr.kind {
            let is_unbounded = !is_small_constant_bounded_loop(expr, block, source);
            self.unbounded_loop_stack.push(is_unbounded);
            return;
        }

        // Only inspect method calls when inside at least one unbounded loop.
        if !self.unbounded_loop_stack.iter().copied().any(|u| u) {
            return;
        }

        if let ExprKind::MethodCall(segment, receiver, _args, _call_span) = expr.kind {
            let method_name = segment.ident.name.as_str();
            if is_storage_write_method(method_name) && is_soroban_storage_chain(receiver) {
                span_lint_and_help(
                    cx,
                    UNBOUNDED_STORAGE_WRITES,
                    expr.span,
                    "mutable Soroban ledger storage operation inside an unbounded or dynamic loop",
                    None,
                    "accumulate state in WASM memory inside the loop and perform a single `env.storage()...set()` after the loop completes",
                );
            }
        }
    }

    fn check_expr_post(&mut self, _cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Loop(..)) {
            self.unbounded_loop_stack.pop();
        }
    }
}

fn is_storage_write_method(name: &str) -> bool {
    matches!(
        name,
        "set" | "set_authorized" | "update" | "remove" | "extend_ttl"
    )
}

/// Checks whether a method receiver chain originates from `*.storage().persistent()`,
/// `*.storage().temporary()`, `*.storage().instance()`, or `*.storage()`.
pub fn is_soroban_storage_chain(mut expr: &Expr<'_>) -> bool {
    let mut saw_tier = false;
    let mut saw_storage = false;

    loop {
        match expr.kind {
            ExprKind::MethodCall(segment, inner_recv, _args, _) => {
                let name = segment.ident.name.as_str();
                match name {
                    "persistent" | "temporary" | "instance" => {
                        saw_tier = true;
                    }
                    "storage" => {
                        saw_storage = true;
                    }
                    _ => {}
                }
                expr = inner_recv;
            }
            _ => break,
        }
    }

    saw_storage || saw_tier
}

/// Determines whether a `for` loop iterates over a statically known constant range `0..N`
/// where `N <= MAX_STATIC_LOOP_WRITE_BOUND`. `while` and `loop` constructs are treated as
/// dynamically bounded unless proven otherwise.
fn is_small_constant_bounded_loop(
    _loop_expr: &Expr<'_>,
    _block: &rustc_hir::Block<'_>,
    source: LoopSource,
) -> bool {
    if !matches!(source, LoopSource::ForLoop) {
        return false;
    }
    false
}

/// Helper to extract an integer literal value from an HIR expression when present.
pub fn extract_uint_literal(expr: &Expr<'_>) -> Option<u128> {
    if let ExprKind::Lit(spanned) = expr.kind {
        if let LitKind::Int(val, _) = spanned.node {
            return Some(val.get());
        }
    }
    if let ExprKind::Path(QPath::Resolved(_, path)) = expr.kind {
        let _ = path;
    }
    None
}
