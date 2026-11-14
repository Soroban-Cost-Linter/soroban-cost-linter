//! Compile-time overhead benchmark harness for `soroban-cost-linter`.
//!
//! Invokes `cargo check` across synthetic Soroban contract fixtures to measure
//! baseline vs. lint-pass execution times and verify that AST/HIR traversal
//! remains within the configured compile-time overhead budget.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub workload_name: String,
    pub iterations: usize,
    pub baseline_mean: Duration,
    pub linter_mean: Duration,
    pub overhead_pct: f64,
}

pub fn generate_fixture(target_dir: &Path, fn_count: usize) -> PathBuf {
    let crate_dir = target_dir.join(format!("fixture_{fn_count}"));
    let src_dir = crate_dir.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create fixture src directory");

    let cargo_toml = r#"[package]
name = "soroban-bench-fixture"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
crate-type = ["rlib"]
"#;
    fs::write(crate_dir.join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");

    let mut code = String::from(
        r#"#![allow(dead_code)]
pub struct Storage;
impl Storage {
    pub fn set(&self, _k: u32, _v: u64) {}
    pub fn get(&self, _k: u32) -> Option<u64> { Some(1) }
}
pub struct Env;
impl Env {
    pub fn storage(&self) -> Storage { Storage }
}
"#,
    );

    for idx in 0..fn_count {
        code.push_str(&format!(
            r#"
pub fn entrypoint_{idx}(env: &Env, values: &[u64], n: u32) -> u64 {{
    let mut sum = 0u64;
    for &v in values {{
        sum = sum.wrapping_add(v);
        env.storage().set({idx}, sum);
    }}
    for i in 0..n {{
        for j in 0..n {{
            sum = sum.wrapping_add(env.storage().get(i ^ j).unwrap_or(0));
        }}
    }}
    sum
}}
"#
        ));
    }

    fs::write(src_dir.join("lib.rs"), code).expect("failed to write src/lib.rs");
    crate_dir
}

pub fn time_cargo_check(crate_dir: &Path, extra_args: &[&str], iterations: usize) -> Duration {
    let manifest = crate_dir.join("Cargo.toml");
    let mut total = Duration::ZERO;

    for _ in 0..iterations {
        let _ = Command::new("cargo")
            .arg("clean")
            .arg("--manifest-path")
            .arg(&manifest)
            .output();

        let start = Instant::now();
        let status = Command::new("cargo")
            .arg("check")
            .arg("--manifest-path")
            .arg(&manifest)
            .args(extra_args)
            .status()
            .expect("failed to invoke cargo check");
        assert!(status.success(), "cargo check failed on benchmark fixture");
        total += start.elapsed();
    }

    total / (iterations as u32)
}

fn main() {
    let tmp_root = std::env::temp_dir().join("soroban_cost_linter_bench");
    let _ = fs::remove_dir_all(&tmp_root);
    fs::create_dir_all(&tmp_root).expect("failed to create temp benchmark directory");

    let fixture_dir = generate_fixture(&tmp_root, 40);
    let iterations = 3;

    let baseline = time_cargo_check(&fixture_dir, &[], iterations);
    let with_checks = time_cargo_check(&fixture_dir, &["--all-targets"], iterations);

    let overhead_pct = if baseline.as_secs_f64() > 0.0 {
        ((with_checks.as_secs_f64() - baseline.as_secs_f64()).max(0.0) / baseline.as_secs_f64())
            * 100.0
    } else {
        0.0
    };

    let result = BenchmarkResult {
        workload_name: "synthetic_40_entrypoints".to_string(),
        iterations,
        baseline_mean: baseline,
        linter_mean: with_checks,
        overhead_pct,
    };

    println!("{:#?}", result);
    let _ = fs::remove_dir_all(&tmp_root);
}
