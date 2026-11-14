#![allow(dead_code)]

pub struct Crypto;
impl Crypto {
    pub fn sha256(&self, _bytes: &[u8]) -> [u8; 32] {
        [0u8; 32]
    }
}

pub struct Address;
impl Address {
    pub fn require_auth(&self) {}
}

pub struct Env;
impl Env {
    pub fn crypto(&self) -> Crypto {
        Crypto
    }
    pub fn invoke_contract(&self, _contract: &Address, _fn_name: &str) {}
}

// True Positives: Repetitive host calls inside a loop
pub fn unoptimized_verify(env: &Env, caller: &Address, payloads: &[&[u8]]) {
    for payload in payloads {
        caller.require_auth();
        let _digest = env.crypto().sha256(payload);
    }
}

// False Positive Baseline: Host calls hoisted outside loop
pub fn optimized_verify(env: &Env, caller: &Address, payloads: &[&[u8]]) {
    caller.require_auth();
    if let Some(last) = payloads.last() {
        let _digest = env.crypto().sha256(last);
    }
}

fn main() {
    let env = Env;
    let caller = Address;
    unoptimized_verify(&env, &caller, &[b"a", b"b"]);
    optimized_verify(&env, &caller, &[b"a", b"b"]);
}
