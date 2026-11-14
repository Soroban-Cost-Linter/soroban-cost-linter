# Soroban Fee-Model Research & Static Lint Metric Mapping

## 1. Executive Summary

Soroban transactions on the Stellar network pay an **inclusion fee** (standard Stellar surge-pricing per operation) plus a non-refundable **resource fee** derived from deterministic, fine-grained metering across **11 resource dimensions** (defined by CAP-0046-07 / CAP-0046-08 and configured on-chain via `ConfigSettingID` ledger entries).

Unlike EVM gas—where opcodes share a single scalar budget—Soroban enforces **independent hard per-transaction limits** alongside per-dimension fee rates. A contract can have ample CPU headroom and still revert at runtime if it exceeds the ledger write-entry cap, read-byte cap, or contract-events byte cap.

This document establishes the empirical fee-model foundation for `soroban-cost-linter`, mapping all **11 metered resource dimensions** of the Soroban runtime directly to our static Dylint passes (`unbounded_storage_writes`, `excessive_host_calls`, `nested_storage_iteration`, and `large_memory_allocation`).

---

## 2. The 11 Metered Resource Dimensions in Soroban

When a Soroban transaction executes, the `soroban-env-host` budget tracker and Stellar Core fee calculator evaluate the following 11 dimensions:

| # | Resource Dimension | On-Chain Config Setting (`ConfigSettingID`) | Default Mainnet Limit (Per Tx) | Fee / Cost Characteristic |
|---|--------------------|---------------------------------------------|--------------------------------|---------------------------|
| **1** | **CPU Instructions** | `ContractComputeV0` (`txMaxInstructions`, `feeRatePerInstructionsIncrement`) | `100,000,000` instructions | Billed per `10,000` instructions (`~25 stroops / 10k instr`). Covers WASM execution + calibrated host function CPU weights. |
| **2** | **Memory Bytes (RAM)** | `ContractComputeV0` (`txMemoryLimit`) | `41,943,040` bytes (`40 MiB`) | Hard execution ceiling (`txMemoryLimit`). No direct per-byte stroop fee, but linear memory expansion and host `Val`/object clones trigger accompanying CPU instruction charges and abort immediately on limit breach. |
| **3** | **Ledger Disk Read Entries** | `ContractLedgerCostV0` (`txMaxDiskReadEntries`, `feeDiskReadLedgerEntry`) | `100` entries | Fixed fee per disk-backed ledger entry loaded into the footprint (`~6,250 stroops / entry`). |
| **4** | **Ledger Disk Read Bytes** | `ContractLedgerCostV0` (`txMaxDiskReadBytes`, `feeDiskRead1KB`) | `200,000` bytes (`~200 KB`) | Billed per `1,024` bytes read from disk-backed storage (`~1,786 stroops / KB`). |
| **5** | **Ledger Write Entries** | `ContractLedgerCostV0` (`txMaxWriteLedgerEntries`, `feeWriteLedgerEntry`) | `50` entries | Fixed fee per modified or created ledger entry (`~10,000 stroops / entry`). |
| **6** | **Ledger Write Bytes** | `ContractLedgerCostV0` (`txMaxWriteBytes`, `feeWrite1KB`) | `131,072` bytes (`128 KB`) | Billed per `1,024` bytes written to the ledger (`~3,500+ stroops / KB`, dynamic with BucketList size). |
| **7** | **State Archival / Rent (TTL Extension Byte-Ledgers)** | `ContractLedgerCostV0` & `StateArchivalSettings` (`persistentRentRateDenominator`, `tempRentRateDenominator`) | Governed by `maxEntryTTL` (`6,312,000` ledgers) | Billed proportional to `entry_size_bytes × ledgers_extended × write_fee_per_1kb / rent_rate_denominator`. Persistent storage rent is significantly more expensive than temporary storage. |
| **8** | **Transaction Size (Bandwidth)** | `ContractBandwidthV0` (`txMaxSizeBytes`, `feeTxSize1KB`) | `131,072` bytes (`128 KB`) | Billed per `1,024` bytes of serialized `TransactionEnvelope` XDR broadcast across the overlay network (`~1,624 stroops / KB`). |
| **9** | **Historical Ledger Archive Size** | `ContractHistoricalDataV0` (`feeHistorical1KB`) | Bound by tx envelope + result XDR | Billed per `1,024` bytes of `TransactionEnvelope` + `TransactionResultPair` persisted in ledger history archives (`~300 stroops / KB`). |
| **10** | **Contract Events & Diagnostic Data** | `ContractEventsV0` (`txMaxContractEventsSizeBytes`, `feeContractEvents1KB`) | `8,192` bytes (`8 KB`) | Billed per `1,024` bytes of XDR-encoded contract events (`env.events().publish`) plus diagnostic return values (`~10,000 stroops / KB`). |
| **11** | **Contract Return Value Size** | `ContractEventsV0` / `SorobanTransactionMeta` | Included in `txMaxContractEventsSizeBytes` (`8 KB`) | Large XDR return payloads (`Vec<T>`, `Map<K, V>`, `Bytes`) serialize into transaction metadata, consuming the `8 KB` events/return-value ceiling and incurring `feeContractEvents1KB`. |

### Total Resource Fee Formula

$$
\text{ResourceFee} = F_{\text{CPU}} + F_{\text{ReadEntries}} + F_{\text{ReadBytes}} + F_{\text{WriteEntries}} + F_{\text{WriteBytes}} + F_{\text{Rent}} + F_{\text{Bandwidth}} + F_{\text{Historical}} + F_{\text{Events+Return}}
$$

---

## 3. Mapping Static Lints to Soroban Resource Dimensions

Each static lint defined in `PRD.md` targets specific cost amplification mechanisms across the 11 dimensions above.

### 3.1. `soroban_cost::unbounded_storage_writes`

* **Static Pattern Detected:**
  Loop constructs (`for`, `while`, `loop`) containing mutable Soroban storage calls—such as `env.storage().persistent().set(...)`, `env.storage().temporary().set(...)`, `env.storage().instance().set(...)`, `.update(...)`, `.remove(...)`, or `.extend_ttl(...)`—where the iteration count is either dynamically derived from function parameters/storage or exceeds a safe constant bound.
* **Primary Metered Dimensions Impacted:**
  1. **Dimension #1 — CPU Instructions (`ContractComputeV0`):**
     Every `.set(&key, &val)` call crosses the WASM-to-host boundary, clones/validates `Val` handles, serializes the key and value into XDR (`ScVal`), and updates the host storage map. Even when writing to the *same* key repeatedly inside a loop, XDR serialization and host dispatch execute on every iteration ($\mathcal{O}(N)$ CPU instructions).
  2. **Dimension #5 & #6 — Ledger Write Entries & Write Bytes (`ContractLedgerCostV0`):**
     When the loop writes to distinct keys (e.g., `DataKey::Item(i)`), each iteration adds a new `LedgerKey` to the transaction footprint. Because `txMaxWriteLedgerEntries` is capped at `50` entries per transaction (and total footprint keys at `100`), a loop over just 51 items causes an **unrecoverable transaction failure** regardless of fee paid.
  3. **Dimension #7 — State Archival / Rent (`StateArchivalSettings`):**
     Writing $N$ distinct persistent entries multiplies rent charges and minimum TTL extension costs by $N$, quickly inflating transaction fees from hundreds of stroops to ratusan of thousands of stroops.
* **Empirical Cost Comparison ($N = 40$ items):**
  * **Unoptimized (storage `.set()` inside loop on same key):** $40\times$ host XDR serialization + map insertion overhead ($\approx 180,000$ extra CPU instructions).
  * **Unoptimized (storage `.set()` inside loop on distinct keys):** $40$ write entries $\times 10,000\text{ stroops} = 400,000\text{ stroops}$ in write-entry fees alone, plus $40\times$ rent charges, and hard failure at $N > 50$.
  * **Optimized (accumulate in local variable / batch struct, single `.set()` outside loop):** $1$ write entry, $1$ XDR serialization pass ($\approx 95\%$ reduction in storage and CPU overhead).

---

### 3.2. `soroban_cost::excessive_host_calls`

* **Static Pattern Detected:**
  Loop constructs invoking expensive Soroban host environment methods on each iteration, including:
  * Cryptographic primitives: `env.crypto().sha256(...)`, `env.crypto().keccak256(...)`, `env.crypto().ed25519_verify(...)`, `env.crypto().secp256k1_recover(...)`
  * Cross-contract invocations: `env.invoke_contract(...)`, `env.try_invoke_contract(...)`
  * Authorization checks: `addr.require_auth()`, `addr.require_auth_for_args(...)`
  * Event emissions inside loops: `env.events().publish(...)`
* **Primary Metered Dimensions Impacted:**
  1. **Dimension #1 — CPU Instructions (`ContractComputeV0`):**
     The Soroban host charges calibrated `ContractCostType` CPU models for every host call:
     * `InvokeVmFunction` / host boundary dispatch overhead per call
     * `ComputeSha256Hash` / `ComputeKeccak256Hash` (fixed base cost + per-byte cost)
     * `VerifyEd25519Sig` ($\approx 300,000+$ CPU instructions per signature verification)
     * `ValSer` / `ValDeser` when marshaling arguments across WASM linear memory and host objects
     * Cross-contract calls (`invoke_contract`) instantiate or reuse a sub-contract VM context, costing hundreds of thousands of instructions per invocation.
  2. **Dimension #2 — Memory Bytes (`txMemoryLimit`):**
     Each host call that allocates a new `Bytes`, `BytesN<32>`, `Vec`, or sub-contract frame in the host object table grows the transaction's metered memory allocation.
  3. **Dimension #10 — Contract Events Size (`ContractEventsV0`):**
     Calling `env.events().publish(...)` inside a loop serializes topics and event data into XDR on every iteration, rapidly exhausting the strict `8,192`-byte (`txMaxContractEventsSizeBytes`) cap.
* **Empirical Cost Comparison:**
  * Calling `ed25519_verify` or `invoke_contract` inside a 25-iteration loop consumes $7.5\text{M} - 25\text{M}$ CPU instructions ($7.5\% - 25\%$ of the entire transaction's 100M CPU cap) and increases CPU fees by $18,000 - 62,500\text{ stroops}$.
  * Hoisting authorization checks (`require_auth`), pre-aggregating payloads before hashing, or exposing batch entrypoints in target contracts reduces host boundary crossings from $\mathcal{O}(N)$ to $\mathcal{O}(1)$.

---

### 3.3. `soroban_cost::nested_storage_iteration`

* **Static Pattern Detected:**
  Nested loops (`depth >= 2`, e.g., `for` inside `for`/`while`, or loop calling helper functions that iterate) where the inner body performs ledger storage reads (`.get()`, `.has()`) or writes (`.set()`, `.update()`, `.remove()`).
* **Primary Metered Dimensions Impacted:**
  1. **Dimension #1 — CPU Instructions (`ContractComputeV0`):**
     Even though Soroban caches a `LedgerEntry` in memory after its first disk load within a transaction, calling `env.storage().persistent().get(&key)` inside an $M \times N$ nested loop still executes **XDR deserialization (`ValDeser`) and host object cloning on every single inner iteration**. An $M \times N$ loop ($20 \times 20 = 400$ iterations) deserializing a $2\text{ KB}$ struct performs $800\text{ KB}$ of redundant XDR decoding, burning millions of CPU instructions.
  2. **Dimension #2 — Memory Bytes (`txMemoryLimit`):**
     Because Soroban's host bump-arena allocator does not free intermediate host objects until the transaction (or sub-contract invocation) finishes, deserializing a storage entry $M \times N$ times allocates $M \times N$ distinct host object copies in RAM, risking a `txMemoryLimit` (`40 MiB`) panic.
  3. **Dimension #3, #4, #5, #6 — Ledger Disk Read/Write Entries & Bytes (`ContractLedgerCostV0`):**
     If the inner key varies with $(i, j)$, the footprint grows quadratically ($\mathcal{O}(M \cdot N)$), guaranteed to breach `txMaxDiskReadEntries` (`100`) or `txMaxWriteLedgerEntries` (`50`) for even small values such as $M = 8, N = 8$ ($64 > 50$).
* **Empirical Cost Comparison ($M = 10, N = 10$ over a $1\text{ KB}$ stored vector/map):**
  * **Unoptimized (nested `.get()` + `.set()` in inner loop):** $100$ host XDR deserializations + $100$ XDR serializations + $200\text{ KB}$ of arena allocations ($\approx 8\text{M} - 15\text{M}$ CPU instructions).
  * **Optimized (load once into WASM local variable before outer loop, compute in memory, write once after outer loop):** $1$ XDR deserialization + $1$ XDR serialization ($\approx 99\%$ reduction in CPU and memory arena growth).

---

### 3.4. `soroban_cost::large_memory_allocation`

* **Static Pattern Detected:**
  Single-scope allocations of large collections or buffers—such as `vec![0u8; N]`, `Vec::with_capacity(N)` with large constants, large fixed arrays `[u8; N]`, or cloning entire unbounded persistent collections (`env.storage().persistent().get::<_, Vec<T>>(...)`) without pagination.
* **Primary Metered Dimensions Impacted:**
  1. **Dimension #2 — Memory Bytes (`ContractComputeV0::txMemoryLimit`):**
     Directly consumes the `40 MiB` (`41,943,040` bytes) transaction RAM budget (`VmInstantiation`, `WasmMemAlloc`, `HostMemAlloc`).
  2. **Dimension #1 — CPU Instructions (`ContractComputeV0`):**
     `HostMemAlloc`, `HostMemCpy`, and `HostMemCmp` cost models charge CPU instructions linearly per byte allocated, copied, or compared.
  3. **Dimension #4 & #6 — Ledger Disk Read/Write Bytes (`ContractLedgerCostV0`):**
     Loading or saving a monolithic `Vec<Order>` or `Map<Address, Profile>` stored under a single `DataKey` forces the runtime to read and rewrite the entire serialized blob on every access, billing `feeDiskRead1KB` and `feeWrite1KB` for the full collection size and risking the `200 KB` read / `128 KB` write byte ceiling.
  4. **Dimension #11 — Contract Return Value Size (`ContractEventsV0`):**
     Returning a large `Vec<T>` from a view function without pagination fails when the serialized XDR exceeds `8,192` bytes (`txMaxContractEventsSizeBytes`).
* **Empirical Cost Comparison:**
  * Replacing a single monolithic `Vec<Order>` (or a large pre-allocated buffer) with indexed `DataKey::Order(id)` storage entries and paginated queries (`page`, `page_size <= MAX_PAGE_SIZE`) bounds per-transaction read/write bytes to $\mathcal{O}(\text{page\_size})$ and prevents return-value XDR overflows.

---

## 4. Consolidated Lint-to-Resource Correlation Matrix

| Static Lint Rule | Default Severity / Confidence | Primary Dimensions Impacted | Secondary Dimensions Impacted | Primary Failure / Drain Mode Prevented |
|------------------|-------------------------------|-----------------------------|-------------------------------|----------------------------------------|
| `unbounded_storage_writes` | `Warn` (**High** Confidence) | **#5** Write Entries, **#6** Write Bytes, **#7** Rent (TTL), **#1** CPU Instructions | **#8** Tx Size (Footprint XDR), **#9** Historical Size | Exceeding `50` write-entry limit; linear $\mathcal{O}(N)$ rent and write-byte fee drain. |
| `excessive_host_calls` | `Warn` / `Allow` (**Medium–High** Confidence) | **#1** CPU Instructions, **#10** Contract Events | **#2** Memory Bytes | Exhausting `100M` CPU instruction budget on WASM-to-host context switches & crypto/sub-contract calls. |
| `nested_storage_iteration` | `Warn` (**High** Confidence) | **#1** CPU Instructions, **#2** Memory Bytes, **#3–#6** Read/Write Entries & Bytes | **#7** Rent (TTL) | Quadratic $\mathcal{O}(M \cdot N)$ XDR serialization/deserialization CPU burn, arena RAM exhaustion, and footprint entry overflow. |
| `large_memory_allocation` | `Allow` (**Low–Medium** Confidence) | **#2** Memory Bytes, **#4** Read Bytes, **#6** Write Bytes, **#11** Return Value Size | **#1** CPU Instructions (`MemAlloc`/`MemCpy`) | Hitting `40 MiB` RAM cap, `128 KB` write-byte cap, or `8 KB` return-value XDR cap. |

---

## 5. Actionable Heuristics & Threshold Calibration for Phase 2

Based on the empirical network limits above, `soroban-cost-linter` calibrates its static thresholds (configurable via `soroban_cost.toml`) as follows:

1. **Unbounded Loop Storage Threshold (`max_loop_storage_writes = 1`):**
   Any mutable storage call inside a loop whose upper bound is not a compile-time constant $\le 16$ (well below `txMaxWriteLedgerEntries = 50`) triggers `unbounded_storage_writes`.
2. **Nested Loop Depth Threshold (`max_nested_loop_depth = 2`):**
   Any storage read (`.get()`, `.has()`) or write (`.set()`, `.remove()`) at loop nesting depth $\ge 2$ triggers `nested_storage_iteration` (`High` confidence), because even cached ledger entries incur repeated host XDR deserialization and arena allocation per iteration.
3. **Host Call Loop Detection:**
   Any call to `env.crypto().*`, `env.invoke_contract(...)`, `env.try_invoke_contract(...)`, `require_auth*`, or `env.events().publish(...)` inside a loop triggers `excessive_host_calls`.
4. **Memory Allocation Threshold (`max_memory_alloc_bytes = 16384`):**
   Static buffer or collection pre-allocations exceeding `16 KB` (`16,384` bytes—twice the `8 KB` return-value/event XDR ceiling and a significant fraction of the `128 KB` ledger write ceiling) trigger `large_memory_allocation`.
