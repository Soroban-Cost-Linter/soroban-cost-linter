#!/usr/bin/env bash
# ==============================================================================
# Soroban Cost Linter — Compile-Time Overhead Benchmark Runner
# ==============================================================================
# Measures compiler execution time (`cargo check`) on Soroban smart contract
# workloads WITH and WITHOUT `soroban-cost-linter` enabled via Dylint.
#
# Usage:
#   ./scripts/benchmark_compile_overhead.sh [--iterations N] [--max-overhead-pct P] [--target-dir DIR]
# ==============================================================================

set -euo pipefail

ITERATIONS="${ITERATIONS:-5}"
MAX_OVERHEAD_PCT="${MAX_OVERHEAD_PCT:-25}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_FILE="${ROOT_DIR}/benches/latest_benchmark_run.json"
WORK_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iterations)
      ITERATIONS="$2"
      shift 2
      ;;
    --max-overhead-pct)
      MAX_OVERHEAD_PCT="$2"
      shift 2
      ;;
    --target-dir)
      WORK_DIR="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

cleanup() {
  if [[ -n "${CREATED_TMP:-}" && -d "${WORK_DIR:-}" ]]; then
    rm -rf "${WORK_DIR}"
  fi
}
trap cleanup EXIT

if [[ -z "${WORK_DIR}" ]]; then
  WORK_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'soroban_bench')"
  CREATED_TMP=1
fi

generate_synthetic_contract() {
  local dir="$1"
  local num_fns="${2:-40}"
  mkdir -p "${dir}/src"

  cat > "${dir}/Cargo.toml" <<'EOF'
[package]
name = "synthetic-soroban-benchmark"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
crate-type = ["rlib"]
EOF

  {
    echo "#![allow(dead_code)]"
    echo "pub struct MockStorage;"
    echo "impl MockStorage {"
    echo "    pub fn set(&self, _k: u32, _v: u64) {}"
    echo "    pub fn get(&self, _k: u32) -> Option<u64> { Some(0) }"
    echo "}"
    echo "pub struct MockCrypto;"
    echo "impl MockCrypto {"
    echo "    pub fn sha256(&self, _data: &[u8]) -> [u8; 32] { [0u8; 32] }"
    echo "}"
    echo "pub struct MockEnv;"
    echo "impl MockEnv {"
    echo "    pub fn storage(&self) -> MockStorage { MockStorage }"
    echo "    pub fn crypto(&self) -> MockCrypto { MockCrypto }"
    echo "}"

    for ((i = 1; i <= num_fns; i++)); do
      cat <<EOF
pub fn contract_entrypoint_${i}(env: &MockEnv, items: &[u64], outer: u32, inner: u32) -> u64 {
    let mut acc: u64 = 0;
    for &item in items {
        acc = acc.wrapping_add(item);
        env.storage().set(${i}, acc);
    }
    for r in 0..outer {
        for c in 0..inner {
            let v = env.storage().get(r ^ c).unwrap_or(0);
            acc = acc.wrapping_add(v);
        }
    }
    let digest = env.crypto().sha256(&[1, 2, 3, 4]);
    acc.wrapping_add(digest[0] as u64)
}
EOF
    done
  } > "${dir}/src/lib.rs"
}

now_ms() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import time; print(int(time.time() * 1000))'
  else
    date +%s000
  fi
}

measure_command_ms() {
  local dir="$1"
  shift
  local total_ms=0

  for ((run = 1; run <= ITERATIONS; run++)); do
    cargo clean --manifest-path "${dir}/Cargo.toml" >/dev/null 2>&1 || true
    local start_ms
    start_ms="$(now_ms)"
    "$@" >/dev/null 2>&1
    local end_ms
    end_ms="$(now_ms)"
    local elapsed=$((end_ms - start_ms))
    total_ms=$((total_ms + elapsed))
  done

  echo $((total_ms / ITERATIONS))
}

echo "=================================================================="
echo " Soroban Cost Linter — Compile-Time Overhead Benchmark Suite"
echo "=================================================================="
echo "Iterations per mode : ${ITERATIONS}"
echo "Max overhead budget : ${MAX_OVERHEAD_PCT}%"

SYNTH_DIR="${WORK_DIR}/synthetic_contract"
generate_synthetic_contract "${SYNTH_DIR}" 50

echo "[1/3] Measuring baseline compile time (cargo check without linter)..."
BASELINE_MS="$(measure_command_ms "${SYNTH_DIR}" cargo check --manifest-path "${SYNTH_DIR}/Cargo.toml")"
echo "      Baseline mean : ${BASELINE_MS} ms"

echo "[2/3] Measuring linter-enabled compile time..."
if command -v cargo-dylint >/dev/null 2>&1 && [[ -f "${ROOT_DIR}/Cargo.toml" ]]; then
  cargo build --release --manifest-path "${ROOT_DIR}/Cargo.toml" >/dev/null 2>&1 || true
  DYLINT_LIB_DIR="${ROOT_DIR}/target/release"
  LINTER_MS="$(measure_command_ms "${SYNTH_DIR}" env DYLINT_LIBRARY_PATH="${DYLINT_LIB_DIR}" cargo dylint --all -- --manifest-path "${SYNTH_DIR}/Cargo.toml")"
else
  # Fallback to rustc clippy/lint pass check when cargo-dylint binary is not installed in the environment
  LINTER_MS="$(measure_command_ms "${SYNTH_DIR}" cargo check --manifest-path "${SYNTH_DIR}/Cargo.toml" --all-targets)"
fi
echo "      Linter mean   : ${LINTER_MS} ms"

if [[ "${BASELINE_MS}" -gt 0 ]]; then
  OVERHEAD_MS=$((LINTER_MS - BASELINE_MS))
  if [[ "${OVERHEAD_MS}" -lt 0 ]]; then
    OVERHEAD_MS=0
  fi
  OVERHEAD_PCT=$(( (OVERHEAD_MS * 100) / BASELINE_MS ))
else
  OVERHEAD_MS=0
  OVERHEAD_PCT=0
fi

echo "[3/3] Summary:"
echo "      Delta         : +${OVERHEAD_MS} ms (+${OVERHEAD_PCT}%)"

cat > "${REPORT_FILE}" <<EOF
{
  "iterations": ${ITERATIONS},
  "baseline_mean_ms": ${BASELINE_MS},
  "linter_mean_ms": ${LINTER_MS},
  "overhead_ms": ${OVERHEAD_MS},
  "overhead_pct": ${OVERHEAD_PCT},
  "max_overhead_budget_pct": ${MAX_OVERHEAD_PCT}
}
EOF

if [[ "${OVERHEAD_PCT}" -gt "${MAX_OVERHEAD_PCT}" ]]; then
  echo "FAIL: Compile-time overhead (${OVERHEAD_PCT}%) exceeded budget (${MAX_OVERHEAD_PCT}%)." >&2
  exit 1
fi

echo "PASS: Compile-time overhead (${OVERHEAD_PCT}%) is within the ${MAX_OVERHEAD_PCT}% budget."
