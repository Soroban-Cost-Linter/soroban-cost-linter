#![allow(dead_code)]

pub struct PersistentStorage;
impl PersistentStorage {
    pub fn set(&self, _key: &str, _val: &u32) {}
    pub fn get(&self, _key: &str) -> Option<u32> {
        Some(0)
    }
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

// True Positive: Unbounded storage write inside a dynamic loop
pub fn unoptimized_loop_write(env: &Env, items: &[u32]) {
    for item in items {
        env.storage().persistent().set("data", item);
    }
}

// False Positive Baseline (Optimized): Accumulate in memory and write once outside the loop
pub fn optimized_single_write(env: &Env, items: &[u32]) {
    let mut sum = 0u32;
    for &item in items {
        sum = sum.saturating_add(item);
    }
    env.storage().persistent().set("sum", &sum);
}

fn main() {
    let env = Env;
    unoptimized_loop_write(&env, &[1, 2, 3]);
    optimized_single_write(&env, &[1, 2, 3]);
}
