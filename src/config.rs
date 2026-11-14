//! Configuration file parser (`soroban_cost.toml`) for `soroban-cost-linter` (`#11`).
//!
//! Reads threshold parameters (`max_memory_alloc_bytes`, `max_nested_loop_depth`,
//! `max_loop_storage_writes`) and per-lint confidence overrides from `soroban_cost.toml`
//! in the target project root.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::confidence::Confidence;

/// Default maximum memory allocation in bytes before `large_memory_allocation` triggers (16 KiB).
pub const DEFAULT_MAX_MEMORY_ALLOC_BYTES: u64 = 16_384;

/// Default nested loop depth threshold for `nested_storage_iteration`.
pub const DEFAULT_MAX_NESTED_LOOP_DEPTH: usize = 2;

/// Default maximum statically bounded loop iterations allowed to write storage.
pub const DEFAULT_MAX_LOOP_STORAGE_WRITES: u64 = 16;

/// Parsed configuration for all Soroban cost lints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinterConfig {
    pub max_memory_alloc_bytes: u64,
    pub max_nested_loop_depth: usize,
    pub max_loop_storage_writes: u64,
    pub unbounded_storage_writes_confidence: Confidence,
    pub nested_storage_iteration_confidence: Confidence,
    pub excessive_host_calls_confidence: Confidence,
    pub large_memory_allocation_confidence: Confidence,
}

impl Default for LinterConfig {
    fn default() -> Self {
        Self {
            max_memory_alloc_bytes: DEFAULT_MAX_MEMORY_ALLOC_BYTES,
            max_nested_loop_depth: DEFAULT_MAX_NESTED_LOOP_DEPTH,
            max_loop_storage_writes: DEFAULT_MAX_LOOP_STORAGE_WRITES,
            unbounded_storage_writes_confidence: Confidence::High,
            nested_storage_iteration_confidence: Confidence::High,
            excessive_host_calls_confidence: Confidence::Medium,
            large_memory_allocation_confidence: Confidence::Low,
        }
    }
}

impl LinterConfig {
    /// Load configuration once per process from `SOROBAN_COST_CONFIG`, `CARGO_MANIFEST_DIR`,
    /// or the current working directory, falling back to [`LinterConfig::default`].
    pub fn global() -> &'static LinterConfig {
        static CONFIG: OnceLock<LinterConfig> = OnceLock::new();
        CONFIG.get_or_init(Self::load_from_env)
    }

    pub fn load_from_env() -> Self {
        if let Ok(explicit_path) = env::var("SOROBAN_COST_CONFIG") {
            let path = PathBuf::from(explicit_path);
            if let Ok(contents) = fs::read_to_string(&path) {
                return Self::parse_toml(&contents);
            }
        }

        if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
            if let Some(cfg) = Self::find_and_load(Path::new(&manifest_dir)) {
                return cfg;
            }
        }

        if let Ok(cwd) = env::current_dir() {
            if let Some(cfg) = Self::find_and_load(&cwd) {
                return cfg;
            }
        }

        Self::default()
    }

    fn find_and_load(start_dir: &Path) -> Option<Self> {
        let mut current = Some(start_dir);
        while let Some(dir) = current {
            let candidate = dir.join("soroban_cost.toml");
            if candidate.is_file() {
                if let Ok(contents) = fs::read_to_string(&candidate) {
                    return Some(Self::parse_toml(&contents));
                }
            }
            current = dir.parent();
        }
        None
    }

    /// Lightweight, dependency-free parser for `soroban_cost.toml`.
    pub fn parse_toml(contents: &str) -> Self {
        let mut config = Self::default();

        for raw_line in contents.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('[') {
                continue;
            }

            if let Some((raw_key, raw_val)) = line.split_once('=') {
                let key = raw_key.trim();
                let val = raw_val.trim().trim_matches('"').trim_matches('\'');

                match key {
                    "max_memory_alloc_bytes" => {
                        if let Ok(parsed) = val.replace('_', "").parse::<u64>() {
                            config.max_memory_alloc_bytes = parsed;
                        }
                    }
                    "max_nested_loop_depth" => {
                        if let Ok(parsed) = val.replace('_', "").parse::<usize>() {
                            config.max_nested_loop_depth = parsed;
                        }
                    }
                    "max_loop_storage_writes" => {
                        if let Ok(parsed) = val.replace('_', "").parse::<u64>() {
                            config.max_loop_storage_writes = parsed;
                        }
                    }
                    "unbounded_storage_writes" => {
                        if let Some(c) = Confidence::from_str_lossy(val) {
                            config.unbounded_storage_writes_confidence = c;
                        }
                    }
                    "nested_storage_iteration" => {
                        if let Some(c) = Confidence::from_str_lossy(val) {
                            config.nested_storage_iteration_confidence = c;
                        }
                    }
                    "excessive_host_calls" => {
                        if let Some(c) = Confidence::from_str_lossy(val) {
                            config.excessive_host_calls_confidence = c;
                        }
                    }
                    "large_memory_allocation" => {
                        if let Some(c) = Confidence::from_str_lossy(val) {
                            config.large_memory_allocation_confidence = c;
                        }
                    }
                    _ => {}
                }
            }
        }

        config
    }
}
