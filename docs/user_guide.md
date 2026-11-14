# Soroban Cost Linter — Developer User Guide & Installation Reference

`soroban-cost-linter` is a static, compile-time analysis plugin built on [Dylint](https://github.com/trailofbits/dylint) for Stellar Soroban smart contracts. It inspects Rust AST/HIR during compilation to flag resource-expensive patterns—such as unbounded ledger storage writes, repetitive WASM-to-host crossings inside loops, nested storage iteration, and oversized single-scope memory allocations—before you deploy or run integration tests.

---

## 1. Prerequisites & CLI Installation

Because Dylint plugins link against `rustc_private` compiler internals, you need `rustup` along with the `cargo-dylint` runner and `dylint-link` linker wrapper.

### Step 1.1: Install `cargo-dylint` and `dylint-link`

```bash
cargo install cargo-dylint dylint-link
```

### Step 1.2: Install the Required Nightly Toolchain Components

When building `soroban-cost-linter` from source, `rust-toolchain.toml` automatically selects the pinned nightly compiler along with `rustc-dev` and `llvm-tools-preview`. You can also install them explicitly:

```bash
rustup toolchain install nightly-2023-10-06 --component rustc-dev llvm-tools-preview
```

### Step 1.3: Build the Linter Plugin Locally (Optional)

```bash
git clone https://github.com/Soroban-Cost-Linter/soroban-cost-linter.git
cd soroban-cost-linter
cargo build --release
```

The compiled dynamic library (`libsoroban_cost_linter@...`) will be emitted under `target/release/`.

---

## 2. Project & Workspace Configuration

### 2.1. Configuring `.cargo/config.toml`

When developing or building the Dylint library itself (or configuring a local workspace that links Dylint plugins), add `.cargo/config.toml` in your repository root so Cargo uses `dylint-link` and enables unstable compiler options:

```toml
[build]
rustflags = ["-Z", "unstable-options"]

[target.'cfg(all())']
linker = "dylint-link"
```

> **Tip:** If you want to pass custom lint flags across your Soroban contract workspace during standard checks, you can also configure `DYLINT_RUSTFLAGS` or workspace metadata as shown below.

### 2.2. Registering the Plugin in Your Contract's `Cargo.toml`

Add `workspace.metadata.dylint` to your Soroban project's root `Cargo.toml` so `cargo dylint` automatically discovers and compiles `soroban-cost-linter`:

```toml
[workspace.metadata.dylint]
libraries = [
    { git = "https://github.com/Soroban-Cost-Linter/soroban-cost-linter", branch = "main" },
]
```

Or point `DYLINT_LIBRARY_PATH` to a pre-built binary directory:

```bash
export DYLINT_LIBRARY_PATH=/path/to/soroban-cost-linter/target/release
cargo dylint --all -- --all-targets
```

### 2.3. Running the Linter

Run all registered cost lints across your Soroban workspace:

```bash
cargo dylint --all
```

To run only `soroban-cost-linter` against a specific contract crate:

```bash
cargo dylint --lib soroban_cost_linter -- --manifest-path contracts/my-contract/Cargo.toml
```

---

## 3. Core Cost Lints: Flagged vs. Optimized Examples

### 3.1. `unbounded_storage_writes` (Default: `Warn`, High Confidence)

**Why it matters:** Every call to `env.storage().persistent().set(...)` (or `.temporary().set(...)` / `.instance().set(...)`) crosses the WASM-to-host boundary, serializes key/value pairs to XDR, and mutates ledger state. In Soroban, transactions are strictly capped at `50` write entries and billed per write entry and per kilobyte written. Performing storage writes inside a loop bounded by dynamic user input causes linear fee inflation and runtime transaction failures.

#### ❌ Flagged Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol, Vec};

#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    pub fn record_payouts(env: Env, payouts: Vec<i128>) {
        let key = Symbol::new(&env, "total_paid");
        let mut running_total: i128 = env.storage().persistent().get(&key).unwrap_or(0);

        for amount in payouts.iter() {
            running_total += amount;
            // WARNING [soroban_cost::unbounded_storage_writes]:
            // Mutable persistent storage write inside a dynamically bounded loop.
            env.storage().persistent().set(&key, &running_total);
        }
    }
}
```

#### ✅ Optimized Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol, Vec};

#[contract]
pub struct PayrollContract;

#[contractimpl]
impl PayrollContract {
    pub fn record_payouts(env: Env, payouts: Vec<i128>) {
        let key = Symbol::new(&env, "total_paid");
        let mut running_total: i128 = env.storage().persistent().get(&key).unwrap_or(0);

        // Accumulate in WASM local memory first
        for amount in payouts.iter() {
            running_total += amount;
        }

        // Perform a single ledger write after the loop completes
        env.storage().persistent().set(&key, &running_total);
    }
}
```

---

### 3.2. `excessive_host_calls` (Default: `Warn`, Medium/High Confidence)

**Why it matters:** Soroban host functions (`env.crypto().sha256()`, `env.crypto().ed25519_verify()`, `env.invoke_contract()`, `address.require_auth()`, `env.events().publish()`) incur WASM-to-host context switching overhead, `Val` marshaling costs, and calibrated CPU instruction charges on every invocation.

#### ❌ Flagged Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, Vec};

#[contract]
pub struct BatchVerifier;

#[contractimpl]
impl BatchVerifier {
    pub fn hash_chunks(env: Env, caller: Address, chunks: Vec<Bytes>) -> BytesN<32> {
        let mut latest = BytesN::from_array(&env, &[0u8; 32]);
        for chunk in chunks.iter() {
            // WARNING [soroban_cost::excessive_host_calls]:
            // Host authorization and cryptographic calls executed repeatedly inside loop.
            caller.require_auth();
            latest = env.crypto().sha256(&chunk).into();
        }
        latest
    }
}
```

#### ✅ Optimized Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, Vec};

#[contract]
pub struct BatchVerifier;

#[contractimpl]
impl BatchVerifier {
    pub fn hash_chunks(env: Env, caller: Address, chunks: Vec<Bytes>) -> BytesN<32> {
        // Authorize once outside the loop
        caller.require_auth();

        // Concatenate payload in a single buffer and invoke sha256 once
        let mut combined = Bytes::new(&env);
        for chunk in chunks.iter() {
            combined.append(&chunk);
        }
        env.crypto().sha256(&combined).into()
    }
}
```

---

### 3.3. `nested_storage_iteration` (Default: `Warn`, High Confidence)

**Why it matters:** Even when a ledger entry is already warm in the transaction cache, calling `env.storage().persistent().get(...)` or `.set(...)` inside nested loops ($\text{depth} \ge 2$) triggers repeated XDR deserialization/serialization (`ValDeser`/`ValSer`) and allocates new host objects in the non-garbage-collected bump arena on every inner iteration ($\mathcal{O}(M \times N)$).

#### ❌ Flagged Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol};

#[contract]
pub struct GridContract;

#[contractimpl]
impl GridContract {
    pub fn accumulate_grid(env: Env, rows: u32, cols: u32) {
        let key = Symbol::new(&env, "acc");
        for r in 0..rows {
            for c in 0..cols {
                // WARNING [soroban_cost::nested_storage_iteration]:
                // Persistent storage read/write at loop nesting depth 2.
                let current: u32 = env.storage().persistent().get(&key).unwrap_or(0);
                env.storage().persistent().set(&key, &(current + r + c));
            }
        }
    }
}
```

#### ✅ Optimized Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol};

#[contract]
pub struct GridContract;

#[contractimpl]
impl GridContract {
    pub fn accumulate_grid(env: Env, rows: u32, cols: u32) {
        let key = Symbol::new(&env, "acc");
        // Read once before entering nested loops
        let mut cached: u32 = env.storage().persistent().get(&key).unwrap_or(0);

        for r in 0..rows {
            for c in 0..cols {
                cached = cached.saturating_add(r + c);
            }
        }

        // Write once after nested loops complete
        env.storage().persistent().set(&key, &cached);
    }
}
```

---

### 3.4. `large_memory_allocation` (Default: `Allow`, Low/Medium Confidence)

**Why it matters:** Soroban enforces a strict `40 MiB` (`txMemoryLimit`) RAM ceiling, a `128 KB` ledger write ceiling, and an `8 KB` return-value/events XDR ceiling. Allocating oversized buffers or monolithic vectors inside a single function scope risks hitting linear memory expansion costs or transaction size limits.

#### ❌ Flagged Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Bytes, Env};

#[contract]
pub struct BufferContract;

#[contractimpl]
impl BufferContract {
    pub fn allocate_scratchpad(env: Env) -> u32 {
        // WARNING [soroban_cost::large_memory_allocation]:
        // Large static allocation (65,536 bytes) in a single contract scope.
        let scratch = [0u8; 65_536];
        let bytes = Bytes::from_slice(&env, &scratch);
        bytes.len()
    }
}
```

#### ✅ Optimized Code

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, Bytes, Env};

const MAX_CHUNK_BYTES: usize = 1_024;

#[contract]
pub struct BufferContract;

#[contractimpl]
impl BufferContract {
    pub fn allocate_scratchpad(env: Env) -> u32 {
        // Process or allocate bounded, paginated chunks within safe limits
        let scratch = [0u8; MAX_CHUNK_BYTES];
        let bytes = Bytes::from_slice(&env, &scratch);
        bytes.len()
    }
}
```

---

## 4. Suppressing Warnings & Opting In (`#[allow(...)]` / `#[warn(...)]`)

Sometimes a bounded loop write or host call is intentional and guarded by a domain invariant (for example, a multi-sig threshold capped at 5 signers). You can silence any lint at the crate, module, function, or statement level using standard Rust attributes:

### 4.1. Bypassing a Specific Warning with `#[allow(...)]`

```rust
#[allow(soroban_cost::unbounded_storage_writes)]
pub fn initialize_fixed_tiers(env: Env, tiers: soroban_sdk::Vec<u32>) {
    assert!(tiers.len() <= 5, "maximum 5 tiers allowed");
    for (idx, tier) in tiers.iter().enumerate() {
        env.storage().instance().set(&(idx as u32), &tier);
    }
}
```

You can also suppress multiple cost lints at once:

```rust
#[allow(
    soroban_cost::unbounded_storage_writes,
    soroban_cost::excessive_host_calls
)]
pub fn admin_batch_migration(env: Env) {
    // ...
}
```

### 4.2. Opting Into Low/Medium-Confidence Lints with `#![warn(...)]`

Lints with `Low` or `Medium` confidence (such as `large_memory_allocation`) default to `allow` so they never introduce noisy false positives. To enable strict checking across your contract crate, add the attribute at the top of `src/lib.rs`:

```rust
#![no_std]
#![warn(soroban_cost::large_memory_allocation)]
#![deny(soroban_cost::unbounded_storage_writes)]
#![deny(soroban_cost::nested_storage_iteration)]
```
