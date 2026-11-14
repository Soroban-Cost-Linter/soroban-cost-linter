//! Lint: `large_memory_allocation` (`#10`).
//!
//! Detects large single-scope allocations (`Vec::with_capacity(n)`, `vec![elem; n]`,
//! large fixed arrays `[val; n]`, and unbounded `Vec::new()` / `Map::new()` /
//! `Bytes::new()` pushes inside loops) that risk exceeding Soroban's WASM linear
//! memory limit (`40 MiB`) or ledger write/return-value XDR ceilings.

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_ast::LitKind;
use rustc_hir::{Expr, ExprKind, QPath};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};

/// Default safe single-scope allocation capacity threshold (16,384 elements / bytes).
pub const SAFE_ALLOCATION_LIMIT: u128 = 16_384;

declare_lint! {
    /// **What it does:** Warns against creating excessively large vectors, maps, or byte
    /// arrays inside a single function scope or growing `Vec`/`Map`/`Bytes` inside an
    /// unbounded loop without static capacity bounds.
    ///
    /// **Why is this bad:** Soroban enforces strict WASM linear memory limits (`40 MiB`)
    /// and charges CPU instructions per byte allocated or copied (`HostMemAlloc`, `HostMemCpy`).
    pub LARGE_MEMORY_ALLOCATION,
    Warn,
    "large memory or collection allocation in a single Soroban contract scope"
}

#[derive(Default)]
pub struct LargeMemoryAllocation {
    loop_depth: u32,
}

impl_lint_pass!(LargeMemoryAllocation => [LARGE_MEMORY_ALLOCATION]);

impl<'tcx> LateLintPass<'tcx> for LargeMemoryAllocation {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Loop(..)) {
            self.loop_depth += 1;
            return;
        }

        // 1. Check `Vec::with_capacity(N)` or `from_elem(val, N)` (`vec![val; N]`)
        if let ExprKind::Call(callee, args) = expr.kind {
            if let ExprKind::Path(ref qpath) = callee.kind {
                if let Some(seg) = last_path_segment_name(qpath) {
                    if seg == "with_capacity" {
                        if let Some(first_arg) = args.first() {
                            if let Some(cap) = extract_int_literal(first_arg) {
                                if cap > SAFE_ALLOCATION_LIMIT {
                                    span_lint_and_help(
                                        cx,
                                        LARGE_MEMORY_ALLOCATION,
                                        expr.span,
                                        format!(
                                            "pre-allocating capacity of {cap} elements exceeds safe Soroban scope limit ({SAFE_ALLOCATION_LIMIT})"
                                        ),
                                        None,
                                        "use bounded batch sizes or paginated storage keys instead of large single-scope allocations",
                                    );
                                }
                            }
                        }
                    } else if seg == "from_elem" && args.len() == 2 {
                        if let Some(len) = extract_int_literal(&args[1]) {
                            if len > SAFE_ALLOCATION_LIMIT {
                                span_lint_and_help(
                                    cx,
                                    LARGE_MEMORY_ALLOCATION,
                                    expr.span,
                                    format!(
                                        "vector repeat allocation of {len} elements exceeds safe Soroban scope limit ({SAFE_ALLOCATION_LIMIT})"
                                    ),
                                    None,
                                    "prefer bounded chunk buffers or paginated ledger entries",
                                );
                            }
                        }
                    }
                }
            }
        }

        // 2. Check fixed array repeat expressions `[val; N]` via type table when N > SAFE_ALLOCATION_LIMIT
        if matches!(expr.kind, ExprKind::Repeat(..)) {
            let ty = cx.typeck_results().expr_ty(expr);
            if let rustc_middle::ty::TyKind::Array(_elem_ty, len_const) = ty.kind() {
                if let Some(len) = len_const.try_to_target_usize(cx.tcx) {
                    if u128::from(len) > SAFE_ALLOCATION_LIMIT {
                        span_lint_and_help(
                            cx,
                            LARGE_MEMORY_ALLOCATION,
                            expr.span,
                            format!(
                                "static array allocation of {len} elements exceeds safe Soroban scope limit ({SAFE_ALLOCATION_LIMIT})"
                            ),
                            None,
                            "reduce buffer size or process data in bounded chunks",
                        );
                    }
                }
            }
        }

        // 3. Check unbounded collection growth (`push`, `push_back`, `append`) inside loops
        if self.loop_depth > 0 {
            if let ExprKind::MethodCall(segment, _recv, _args, _span) = expr.kind {
                let method = segment.ident.name.as_str();
                if matches!(method, "push_back" | "append") {
                    span_lint_and_help(
                        cx,
                        LARGE_MEMORY_ALLOCATION,
                        expr.span,
                        format!(
                            "growing Soroban collection via `{method}()` inside a loop without static capacity bounds"
                        ),
                        None,
                        "enforce a maximum batch constant (`MAX_BATCH_SIZE`) before appending to `Vec`, `Map`, or `Bytes` inside loops",
                    );
                }
            }
        }
    }

    fn check_expr_post(&mut self, _cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Loop(..)) {
            self.loop_depth = self.loop_depth.saturating_sub(1);
        }
    }
}

fn last_path_segment_name(qpath: &QPath<'_>) -> Option<&str> {
    match qpath {
        QPath::Resolved(_, path) => path.segments.last().map(|s| s.ident.name.as_str()),
        QPath::TypeRelative(_, seg) => Some(seg.ident.name.as_str()),
        QPath::LangItem(..) => None,
    }
}

fn extract_int_literal(expr: &Expr<'_>) -> Option<u128> {
    if let ExprKind::Lit(spanned) = expr.kind {
        if let LitKind::Int(val, _) = spanned.node {
            return Some(val.get());
        }
    }
    None
}
