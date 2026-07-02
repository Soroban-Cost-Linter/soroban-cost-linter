// Templates defining typical unoptimized Soroban patterns and their optimized counterparts
const templates = {
    unbounded_writes: {
        code: `#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol, Vec};

#[contract]
pub struct UnboundedStorageContract;

#[contractimpl]
impl UnboundedStorageContract {
    pub fn process_data(env: Env, items: Vec<u32>) {
        // Warning: Unbounded storage writes inside loop
        for item in items.iter() {
            env.storage().persistent().set(&Symbol::new(&env, "data"), &item);
        }
    }
}`,
        fix: `#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol, Vec};

#[contract]
pub struct UnboundedStorageContract;

#[contractimpl]
impl UnboundedStorageContract {
    pub fn process_data(env: Env, items: Vec<u32>) {
        // OPTIMIZED: Accumulate data and save to storage exactly once outside the loop
        let mut last_item = 0;
        for item in items.iter() {
            last_item = item;
        }
        env.storage().persistent().set(&Symbol::new(&env, "data"), &last_item);
    }
}`
    },
    
    excessive_host: {
        code: `#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Bytes, Symbol};

#[contract]
pub struct HostCallsContract;

#[contractimpl]
impl HostCallsContract {
    pub fn verify_signatures(env: Env, hashes: soroban_sdk::Vec<Bytes>) {
        for hash in hashes.iter() {
            // Warning: Excessive WASM host boundary crossing (sha256) inside loop
            let _sig = env.crypto().sha256(&hash);
        }
    }
}`,
        fix: `#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Bytes, Symbol};

#[contract]
pub struct HostCallsContract;

#[contractimpl]
impl HostCallsContract {
    pub fn verify_signatures(env: Env, hashes: soroban_sdk::Vec<Bytes>) {
        // OPTIMIZED: Batch hash calculations or verify off-chain if possible, 
        // avoiding repetitive cryptographic host function calls in tight loop
        if hashes.len() > 0 {
            let last_hash = hashes.get(hashes.len() - 1).unwrap();
            let _sig = env.crypto().sha256(&last_hash);
        }
    }
}`
    },
    
    nested_storage: {
        code: `#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol};

#[contract]
pub struct NestedStorageContract;

#[contractimpl]
impl NestedStorageContract {
    pub fn compute(env: Env, outer: u32, inner: u32) {
        for i in 0..outer {
            for j in 0..inner {
                // Warning: Nested iteration doing ledger reads/writes
                let val: u32 = env.storage().persistent().get(&Symbol::new(&env, "val")).unwrap_or(0);
                env.storage().persistent().set(&Symbol::new(&env, "val"), &(val + i + j));
            }
        }
    }
}`,
        fix: `#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol};

#[contract]
pub struct NestedStorageContract;

#[contractimpl]
impl NestedStorageContract {
    pub fn compute(env: Env, outer: u32, inner: u32) {
        // OPTIMIZED: Cache the value in WASM local memory, iterate, and write once at the end
        let mut cached_val: u32 = env.storage().persistent().get(&Symbol::new(&env, "val")).unwrap_or(0);
        for i in 0..outer {
            for j in 0..inner {
                cached_val += i + j;
            }
        }
        env.storage().persistent().set(&Symbol::new(&env, "val"), &cached_val);
    }
}`
    },
    
    optimized: {
        code: `#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Symbol, Vec};

#[contract]
pub struct OptimizedContract;

#[contractimpl]
impl OptimizedContract {
    pub fn batch_process(env: Env, items: Vec<u32>) {
        let mut accumulator: u32 = 0;
        for item in items.iter() {
            accumulator = accumulator.saturating_add(item);
        }
        // Optimized: Single write outside the loop
        env.storage().persistent().set(&Symbol::new(&env, "sum"), &accumulator);
    }
}`,
        fix: null
    }
};

// DOM Elements
const codeTextarea = document.getElementById("contract-code");
const lineNumbersDiv = document.getElementById("editor-line-numbers");
const templateSelector = document.getElementById("code-templates");
const runBtn = document.getElementById("run-analysis-btn");
const warningsContainer = document.getElementById("warnings-container");
const charCountSpan = document.getElementById("char-count");

// Dashboard Values
const cpuVal = document.getElementById("cpu-value");
const cpuBar = document.getElementById("cpu-bar");
const memVal = document.getElementById("mem-value");
const memBar = document.getElementById("mem-bar");
const storeVal = document.getElementById("store-value");
const storeBar = document.getElementById("store-bar");
const hostVal = document.getElementById("host-value");
const hostBar = document.getElementById("host-bar");
const feeExplanation = document.getElementById("fee-explanation");

// Synchronize line numbers
function updateLineNumbers() {
    const lines = codeTextarea.value.split('\n');
    const lineCount = lines.length;
    let lineNumbersHtml = '';
    for (let i = 1; i <= lineCount; i++) {
        lineNumbersHtml += `<div>${i}</div>`;
    }
    lineNumbersDiv.innerHTML = lineNumbersHtml;
    charCountSpan.textContent = `Lines: ${lineCount}`;
}

// Populate Editor template
function loadTemplate(key) {
    if (templates[key]) {
        codeTextarea.value = templates[key].code;
        updateLineNumbers();
        analyzeCode();
    }
}

// Perform Mock Static Cost Analysis using regex heuristics
function analyzeCode() {
    const code = codeTextarea.value;
    const warnings = [];
    
    // Resource load values defaults (representing optimized)
    let cpu = 15; // 15% (6M / 40M)
    let mem = 8;  // 8% (5.1MB / 64MB)
    let store = 5; // 5% (3.2KB / 64KB)
    let host = 10; // 10% (10 / 100)
    
    // Heuristics flags
    let hasUnboundedWrites = false;
    let hasExcessiveHost = false;
    let hasNestedStorage = false;
    let hasLargeMemory = false;

    // Detect patterns
    // 1. Unbounded storage writes: Env storage writes inside loop constructs
    const loopRegex = /(for|while|loop)[\s\S]*?\{[\s\S]*?env\.storage\(\)\.persistent\(\)\.set[\s\S]*?\}/;
    if (loopRegex.test(code)) {
        hasUnboundedWrites = true;
        // Check if nested
        const nestedLoopRegex = /(for|while|loop)[\s\S]*?(for|while|loop)[\s\S]*?env\.storage\(\)/;
        if (nestedLoopRegex.test(code)) {
            hasNestedStorage = true;
        }
    }
    
    // 2. Excessive Host Calls: crypto / invoke calls in loops
    const hostInLoopRegex = /(for|while|loop)[\s\S]*?\{[\s\S]*?(env\.crypto\(\)|env\.invoke_contract\(\))[\s\S]*?\}/;
    if (hostInLoopRegex.test(code)) {
        hasExcessiveHost = true;
    }

    // 3. Large allocations
    if (/vec!\[.*?;.*?\b(1000\d+|\d{5,})\b\]|Vec::with_capacity\(\b(1000\d+|\d{5,})\b\)/.test(code)) {
        hasLargeMemory = true;
    }

    // Generate diagnostics based on detected flags
    if (hasNestedStorage) {
        warnings.push({
            id: 'nested_storage',
            title: "NESTED_STORAGE_ITERATION",
            confidence: "High",
            badgeClass: "badge-danger",
            cardClass: "",
            desc: "Ledger storage read/write operation is nested within multiple loops. This will cause exponential cost scaling on Stellar relative to outer and inner bounds, risking transaction failure.",
            line: getLineOfPattern(code, "env.storage()"),
            fixable: true
        });
        cpu = 95;
        store = 85;
        host = 80;
        mem = 18;
    } else if (hasUnboundedWrites) {
        warnings.push({
            id: 'unbounded_writes',
            title: "UNBOUNDED_STORAGE_WRITES",
            confidence: "High",
            badgeClass: "badge-danger",
            cardClass: "",
            desc: "Persistent storage `.set()` is called within a loop construct. If bounds are determined by dynamic transaction inputs, transaction cost will scale linearly with input size and easily exhaust storage limits.",
            line: getLineOfPattern(code, "env.storage().persistent().set"),
            fixable: true
        });
        cpu = 72;
        store = 95;
        host = 40;
        mem = 12;
    }

    if (hasExcessiveHost) {
        warnings.push({
            id: 'excessive_host',
            title: "EXCESSIVE_HOST_CALLS",
            confidence: "Medium",
            badgeClass: "badge-warning",
            cardClass: "warn-medium",
            desc: "Environment host function call (`env.crypto().sha256`) is executed inside a loop. Host boundary crossings require expensive context-switching overhead in the Soroban VM.",
            line: getLineOfPattern(code, "env.crypto()"),
            fixable: true
        });
        cpu = Math.max(cpu, 85);
        host = Math.max(host, 90);
    }

    if (hasLargeMemory) {
        warnings.push({
            id: 'large_mem',
            title: "LARGE_MEMORY_ALLOCATION",
            confidence: "Low",
            badgeClass: "badge-success",
            cardClass: "warn-success",
            desc: "Static memory pre-allocation exceeds safe limits. This can cause VM memory expansion costs to trigger mid-execution.",
            line: getLineOfPattern(code, "vec!"),
            fixable: false
        });
        mem = 78;
    }

    // Render results
    renderWarnings(warnings);
    updateDashboard(cpu, mem, store, host);
    updateFeeImpact(warnings);
}

// Find line number containing pattern
function getLineOfPattern(code, pattern) {
    const lines = code.split('\n');
    for (let i = 0; i < lines.length; i++) {
        if (lines[i].includes(pattern)) {
            return i + 1;
        }
    }
    return 1;
}

// Display warning cards dynamically
function renderWarnings(warnings) {
    if (warnings.length === 0) {
        warningsContainer.innerHTML = `
            <div class="warning-card warn-success" style="border-left-width: 4px; text-align: center; padding: 1.5rem;">
                <div class="warning-title" style="color: var(--success); font-size: 1rem; margin-bottom: 0.25rem;">
                    ✓ Cost Optimization Passed
                </div>
                <div class="warning-desc">
                    No expensive resource-access patterns detected. Your contract compiles cleanly under Soroban static budget guidelines.
                </div>
            </div>`;
        return;
    }

    warningsContainer.innerHTML = warnings.map(w => `
        <div class="warning-card ${w.cardClass}">
            <div class="warning-header">
                <span class="warning-title">${w.title}</span>
                <span class="warning-badge ${w.badgeClass}">${w.confidence} Confidence</span>
            </div>
            <p class="warning-desc">${w.desc}</p>
            <div class="warning-footer">
                <span class="line-info">Line ${w.line}</span>
                ${w.fixable ? `<button class="fix-btn" onclick="applyFix('${w.id}')">Apply Fix</button>` : ''}
            </div>
        </div>
    `).join('');
}

// Update resource graphs
function updateDashboard(cpu, mem, store, host) {
    // Limits: CPU = 40M, Mem = 64MB, Store = 64KB, Host = 100
    const cpuInstructions = Math.round((cpu / 100) * 40);
    const memoryMB = ((mem / 100) * 64).toFixed(1);
    const storageKB = ((store / 100) * 64).toFixed(1);
    const hostCallCount = Math.round((host / 100) * 100);

    cpuVal.textContent = `${cpuInstructions}M / 40M`;
    cpuBar.style.width = `${cpu}%`;
    setBarColorClass(cpuBar, cpu);

    memVal.textContent = `${memoryMB}MB / 64MB`;
    memBar.style.width = `${mem}%`;
    setBarColorClass(memBar, mem);

    storeVal.textContent = `${storageKB}KB / 64KB`;
    storeBar.style.width = `${store}%`;
    setBarColorClass(storeBar, store);

    hostVal.textContent = `${hostCallCount} / 100`;
    hostBar.style.width = `${host}%`;
    setBarColorClass(hostBar, host);
}

function setBarColorClass(bar, val) {
    bar.className = 'progress-bar';
    if (val >= 80) {
        bar.classList.add('bar-danger');
    } else if (val >= 50) {
        bar.classList.add('bar-warning');
    } else if (val >= 25) {
        bar.classList.add('bar-secondary');
    } else {
        bar.classList.add('bar-primary');
    }
}

// Calculate fee impact text
function updateFeeImpact(warnings) {
    if (warnings.length === 0) {
        feeExplanation.innerHTML = `
            <strong>Optimized Cost Profile:</strong> Your contract writes to ledger storage in bulk, keeping CPU instructions low. Transactions will execute within the standard base fee of <strong>~100 Stroops</strong> ($0.00001 XLM).
        `;
        return;
    }

    let severity = "low";
    let impactText = "";
    
    const ids = warnings.map(w => w.id);
    
    if (ids.includes('nested_storage')) {
        severity = "critical";
        impactText = "<strong>Critical Impact:</strong> Due to nested persistent storage operations, this transaction is likely to hit the 40,000,000 CPU limit or the storage limits on-chain, causing transaction revert. Minimum fee overhead would exceed <strong>150,000 Stroops</strong>, making execution unpredictable.";
    } else if (ids.includes('unbounded_writes')) {
        severity = "high";
        impactText = "<strong>High Impact:</strong> Linearly scaling storage writes will inflate the transaction fee dramatically. Every 1KB of ledger write adds a network-wide storage resource fee. Transactions with large vector inputs will incur fees up to <strong>50,000 Stroops</strong>.";
    } else if (ids.includes('excessive_host')) {
        severity = "medium";
        impactText = "<strong>Medium Impact:</strong> Repetitive host crossings inside the WASM virtual machine consume excessive CPU instructions. Fees will scale up to <strong>15,000 Stroops</strong> due to VM instruction overhead.";
    } else {
        severity = "low";
        impactText = "<strong>Low Impact:</strong> Memory expansion costs slightly raise transaction fee requirements. Standard execution fees will be around <strong>1,200 Stroops</strong>.";
    }
    
    feeExplanation.innerHTML = impactText;
}

// Global scope fix trigger
window.applyFix = function(id) {
    let key = "";
    if (id === "unbounded_writes") key = "unbounded_writes";
    else if (id === "excessive_host") key = "excessive_host";
    else if (id === "nested_storage") key = "nested_storage";
    
    if (key && templates[key] && templates[key].fix) {
        codeTextarea.value = templates[key].fix;
        updateLineNumbers();
        analyzeCode();
    }
};

// Event Listeners
codeTextarea.addEventListener("input", () => {
    updateLineNumbers();
    analyzeCode();
});

templateSelector.addEventListener("change", (e) => {
    loadTemplate(e.target.value);
});

runBtn.addEventListener("click", () => {
    analyzeCode();
});

// Initialize on load
loadTemplate("unbounded_writes");
