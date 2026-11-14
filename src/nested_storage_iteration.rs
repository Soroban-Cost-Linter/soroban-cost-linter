//! Lint: `nested_storage_iteration` (`#9`).
//!
//! Analyzes nested loop hierarchies (`depth >= 2`) and flags inner-loop ledger
//! storage reads (`get`, `has`, `try_get`) or writes (`set`, `set_authorized`,
//! `update`, `remove`, `extend_ttl`) on `env.storage().persistent()`,
//! `env.storage().temporary()`, or `env.storage().instance()`.

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};

/// Default minimum loop nesting depth that triggers `nested_storage_iteration`.
pub const MIN_NESTED_LOOP_DEPTH: u32 = 2;

declare_lint! {
    /// **What it does:** Flags nested loops (`depth >= 2`) that perform Soroban ledger
    /// storage reads (`get`, `has`) or writes (`set`, `update`, `remove`, `extend_ttl`).
    ///
    /// **Why is this bad:** Even when a ledger entry is cached in memory during a transaction,
    /// calling `env.storage().persistent().get()` or `.set()` inside nested loops triggers
    /// quadratic $\mathcal{O}(M \cdot N)$ XDR serialization/deserialization (`ValSer`/`ValDeser`)
    /// and bump-arena memory growth, rapidly exhausting Soroban's CPU and RAM limits.
    pub NESTED_STORAGE_ITERATION,
    Warn,
    "Soroban ledger storage read or write inside nested loops (depth >= 2)"
}

#[derive(Default)]
pub struct NestedStorageIteration {
    loop_depth: u32,
}

impl_lint_pass!(NestedStorageIteration => [NESTED_STORAGE_ITERATION]);

impl<'tcx> LateLintPass<'tcx> for NestedStorageIteration {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Loop(..)) {
            self.loop_depth += 1;
            return;
        }

        if self.loop_depth < MIN_NESTED_LOOP_DEPTH {
            return;
        }

        if let ExprKind::MethodCall(segment, receiver, _args, _span) = expr.kind {
            let method = segment.ident.name.as_str();
            if is_storage_access_method(method) && is_soroban_storage_receiver(receiver) {
                span_lint_and_help(
                    cx,
                    NESTED_STORAGE_ITERATION,
                    expr.span,
                    format!(
                        "Soroban ledger storage `{method}()` inside nested loop (depth = {}) causes quadratic XDR and CPU scaling",
                        self.loop_depth
                    ),
                    None,
                    "load the ledger entry once into a local variable before the outer loop, compute in WASM memory, and write once after the outer loop finishes",
                );
            }
        }
    }

    fn check_expr_post(&mut self, _cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Loop(..)) {
            self.loop_depth = self.loop_depth.saturating_sub(1);
        }
    }
}

fn is_storage_access_method(method: &str) -> bool {
    matches!(
        method,
        "get" | "try_get" | "has" | "set" | "set_authorized" | "update" | "remove" | "extend_ttl"
    )
}

fn is_soroban_storage_receiver(mut expr: &Expr<'_>) -> bool {
    let mut saw_storage = false;
    let mut saw_tier = false;

    loop {
        match expr.kind {
            ExprKind::MethodCall(seg, inner, _, _) => {
                match seg.ident.name.as_str() {
                    "persistent" | "temporary" | "instance" => saw_tier = true,
                    "storage" => saw_storage = true,
                    _ => {}
                }
                expr = inner;
            }
            _ => break,
        }
    }

    saw_storage || saw_tier
}
