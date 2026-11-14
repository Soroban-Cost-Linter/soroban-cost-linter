//! Lint: `excessive_host_calls` (`#8`).
//!
//! Detects repeated crossing of the WASM-to-host boundary inside loops (`for`,
//! `while`, `loop`), including cryptographic host functions (`sha256`, `keccak256`,
//! `ed25519_verify`, `secp256k1_recover`), cross-contract calls (`invoke_contract`,
//! `try_invoke_contract`), authorization checks (`require_auth`,
//! `require_auth_for_args`), and event emissions (`publish` on `env.events()`).

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};

declare_lint! {
    /// **What it does:** Identifies repeated invocations of Soroban host functions
    /// (cryptographic primitives, cross-contract invocations, authorization checks,
    /// or event publications) inside loops.
    ///
    /// **Why is this bad:** Every Soroban host call crosses the WASM-to-host boundary,
    /// marshals `Val` handles, and incurs calibrated CPU instruction overhead.
    pub EXCESSIVE_HOST_CALLS,
    Warn,
    "repetitive Soroban host-function calls inside a loop"
}

#[derive(Default)]
pub struct ExcessiveHostCalls {
    loop_depth: u32,
}

impl_lint_pass!(ExcessiveHostCalls => [EXCESSIVE_HOST_CALLS]);

impl<'tcx> LateLintPass<'tcx> for ExcessiveHostCalls {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Loop(..)) {
            self.loop_depth += 1;
            return;
        }

        if self.loop_depth == 0 {
            return;
        }

        if let ExprKind::MethodCall(segment, receiver, _args, _span) = expr.kind {
            let method = segment.ident.name.as_str();
            if let Some((category, help)) = classify_host_call(method, receiver) {
                span_lint_and_help(
                    cx,
                    EXCESSIVE_HOST_CALLS,
                    expr.span,
                    format!(
                        "Soroban {category} host call `{method}()` executed repeatedly inside a loop"
                    ),
                    None,
                    help,
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

fn classify_host_call(
    method: &str,
    receiver: &Expr<'_>,
) -> Option<(&'static str, &'static str)> {
    match method {
        "sha256" | "keccak256" | "ed25519_verify" | "secp256k1_recover" => {
            if is_crypto_receiver(receiver) {
                return Some((
                    "cryptographic",
                    "batch or pre-aggregate inputs before hashing/verifying so the host boundary is crossed once outside the loop",
                ));
            }
        }
        "invoke_contract" | "try_invoke_contract" => {
            return Some((
                "cross-contract",
                "avoid invoking external contracts inside a loop; expose a batch entrypoint on the target contract instead",
            ));
        }
        "require_auth" | "require_auth_for_args" => {
            return Some((
                "authorization",
                "hoist `require_auth` outside the loop so authorization is verified once per entrypoint",
            ));
        }
        "publish" => {
            if is_events_receiver(receiver) {
                return Some((
                    "event publication",
                    "emit a single summary event after the loop completes to avoid exhausting the 8 KB contract events limit",
                ));
            }
        }
        _ => {}
    }
    None
}

fn is_crypto_receiver(mut expr: &Expr<'_>) -> bool {
    loop {
        match expr.kind {
            ExprKind::MethodCall(seg, inner, _, _) => {
                if seg.ident.name.as_str() == "crypto" {
                    return true;
                }
                expr = inner;
            }
            _ => return false,
        }
    }
}

fn is_events_receiver(mut expr: &Expr<'_>) -> bool {
    loop {
        match expr.kind {
            ExprKind::MethodCall(seg, inner, _, _) => {
                if seg.ident.name.as_str() == "events" {
                    return true;
                }
                expr = inner;
            }
            _ => return false,
        }
    }
}
