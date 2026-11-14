//! Confidence-based default lint level infrastructure (`#13`).
//!
//! Maps each Soroban cost lint's empirical confidence rating (`High`, `Medium`, `Low`)
//! to its default compiler `rustc_lint::Level`:
//! - `Confidence::High`   -> `Level::Warn` (enabled by default)
//! - `Confidence::Medium` -> `Level::Allow` (opt-in via `#![warn(soroban_cost::...)]`)
//! - `Confidence::Low`    -> `Level::Allow` (opt-in via `#![warn(soroban_cost::...)]`)

use rustc_lint::{Level, Lint};

/// Confidence rating attached to a registered Soroban cost lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// High-certainty heuristic with minimal false positives; warns by default.
    High,
    /// Moderate-certainty heuristic; allowed by default so developers can opt in.
    Medium,
    /// Conservative/advisory heuristic; allowed by default.
    Low,
}

impl Confidence {
    /// Parse a confidence level from a string in `soroban_cost.toml`.
    pub fn from_str_lossy(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "high" => Some(Self::High),
            "medium" | "med" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    /// Returns the default `rustc_lint::Level` corresponding to this confidence score.
    pub const fn default_lint_level(self) -> Level {
        match self {
            Self::High => Level::Warn,
            Self::Medium | Self::Low => Level::Allow,
        }
    }

    /// Human-readable label included in compiler diagnostic notes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
        }
    }
}

/// Metadata descriptor pairing a `rustc_lint::Lint` with its [`Confidence`] score.
#[derive(Debug, Clone, Copy)]
pub struct LintMetadata {
    pub name: &'static str,
    pub lint: &'static Lint,
    pub confidence: Confidence,
}

impl LintMetadata {
    pub const fn new(name: &'static str, lint: &'static Lint, confidence: Confidence) -> Self {
        Self {
            name,
            lint,
            confidence,
        }
    }

    /// Effective default compiler level derived from the lint's confidence score.
    pub const fn effective_default_level(&self) -> Level {
        self.confidence.default_lint_level()
    }
}
