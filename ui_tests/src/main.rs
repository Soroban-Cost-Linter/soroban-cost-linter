#![allow(dead_code)]

pub struct PersistentStorage;
impl PersistentStorage {
    pub fn get(&self, _key: &str) -> Option<u32> {
        Some(0)
    }
    pub fn set(&self, _key: &str, _val: &u32) {}
}

pub struct Storage;
impl Storage {
    pub fn persistent(&self) -> PersistentStorage {
        PersistentStorage
    }
}

pub struct Env;
impl Env {
    pub fn storage(&self) -> Storage {
        Storage
    }
}

// True Positive: Storage read and write inside nested loops (depth = 2)
pub fn unoptimized_nested_compute(env: &Env, outer: u32, inner: u32) {
    for i in 0..outer {
        for j in 0..inner {
            let val = env.storage().persistent().get("val").unwrap_or(0);
            env.storage().persistent().set("val", &(val + i + j));
        }
    }
}

// False Positive Baseline: Cache before outer loop and write once after outer loop
pub fn optimized_nested_compute(env: &Env, outer: u32, inner: u32) {
    let mut cached = env.storage().persistent().get("val").unwrap_or(0);
    for i in 0..outer {
        for j in 0..inner {
            cached += i + j;
        }
    }
    env.storage().persistent().set("val", &cached);
}

fn main() {
    let env = Env;
    unoptimized_nested_compute(&env, 4, 4);
    optimized_nested_compute(&env, 4, 4);
}
