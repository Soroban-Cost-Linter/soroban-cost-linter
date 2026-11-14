#![allow(dead_code)]

// True Positive: Oversized static array and Vec::with_capacity in single scope
pub fn unoptimized_allocations() -> usize {
    let big_buf = [0u8; 65_536];
    let big_vec: Vec<u8> = Vec::with_capacity(100_000);
    big_buf.len() + big_vec.capacity()
}

// False Positive Baseline: Bounded allocation within safe limits
pub fn optimized_allocations() -> usize {
    let small_buf = [0u8; 256];
    let small_vec: Vec<u8> = Vec::with_capacity(64);
    small_buf.len() + small_vec.capacity()
}

fn main() {
    let _ = unoptimized_allocations();
    let _ = optimized_allocations();
}
