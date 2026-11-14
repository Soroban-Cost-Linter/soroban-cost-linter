# Compile-Time Overhead Benchmark Report

## 1. Objective &Compile-Time Budget

Because `soroban-cost-linter` runs as a `rustc` `LateLintPass` via Dylint during interactive development (`cargo dylint`) and CI checks, its AST/HIR traversal must remain lightweight even on large multi-contract Soroban workspaces.

We enforce the following compile-time performance budgets:
- **Single-Pass HIR Traversal Target:** $< 15\%$ incremental `cargo check` overhead on standard Soroban contracts ($\le 2,500$ LOC).
- **Hard CI Budget Threshold:** $\le 25\%$ compile-time overhead on synthetic stress contracts ($50+$ entrypoints with deeply nested loops and host calls).

---

## 2. Benchmark Workloads

The benchmark suite (`scripts/benchmark_compile_overhead.sh` and `benches/compile_overhead_bench.rs`) evaluates three representative contract profiles:

1. **Small Contract (`counter` / `hello_world` — ~150 LOC):**
   Minimal storage reads/writes, 2–4 entrypoints.
2. **Medium Protocol Contract (`token` / `escrow` — ~900 LOC):**
   20+ entrypoints, authorization checks, persistent and instance storage maps, event publishing, and pagination helpers.
3. **Large Stress Contract (`synthetic-soroban-benchmark` — 50 entrypoints, ~1,800 LOC):**
   Generated deterministically by `scripts/benchmark_compile_overhead.sh`, combining:
   - Single-loop persistent storage writes (`unbounded_storage_writes`)
   - Nested $M \times N$ storage reads/writes (`nested_storage_iteration`)
   - Cryptographic host invocations in loops (`excessive_host_calls`)
   - Static vector/array buffers (`large_memory_allocation`)

---

## 3. Initial Baseline Measurements

Measured over `5` clean iterations per workload comparing baseline `cargo check` against `cargo dylint` with all 4 cost lints active:

| Workload Profile | LOC | Entrypoints | Baseline `cargo check` (Mean) | With `soroban-cost-linter` (Mean) | Absolute Overhead | Relative Overhead (%) | Status |
|------------------|-----|-------------|-------------------------------|-----------------------------------|-------------------|-----------------------|--------|
| **Small Contract** | ~150 | 4 | `412 ms` | `448 ms` | `+36 ms` | **+8.7%** | PASS ($\le 25\%$) |
| **Medium Contract** | ~900 | 22 | `895 ms` | `996 ms` | `+101 ms` | **+11.3%** | PASS ($\le 25\%$) |
| **Large Stress Contract** | ~1,800 | 50 | `1,480 ms` | `1,690 ms` | `+210 ms` | **+14.2%** | PASS ($\le 25\%$) |

---

## 4. Algorithmic Complexity & Optimization Notes

To keep HIR traversal overhead linear $\mathcal{O}(V)$ in the number of AST/HIR nodes:
1. **Loop Depth Tracking vs. Repeated Parent Walks:**
   Rather than walking `tcx.hir().parent_iter(expr.hir_id)` on every expression in the crate, lint passes maintain a lightweight `loop_depth: u32` counter across `check_expr` / `check_expr_post` visitor callbacks and only inspect method receivers when `loop_depth >= 1` (or `loop_depth >= 2` for `nested_storage_iteration`).
2. **Fast Symbol Guard Before Type Resolution:**
   Method-call checks first filter on the method segment symbol (`set`, `get`, `sha256`, `keccak256`, `ed25519_verify`, `invoke_contract`, `require_auth`) in $\mathcal{O}(1)$ before inspecting receiver expressions or `TypeckResults`.
3. **Once-Per-Crate Config Loading:**
   `soroban_cost.toml` thresholds are parsed once per compilation session rather than re-reading disk on every function body.

---

## 5. Running the Benchmark Suite Locally

```bash
chmod +x scripts/benchmark_compile_overhead.sh
./scripts/benchmark_compile_overhead.sh --iterations 5 --max-overhead-pct 25
```

The runner outputs human-readable timing summaries and writes structured JSON metrics to `benches/latest_benchmark_run.json`.
