use std::collections::HashMap;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rayon::prelude::*;
use uuid::Uuid;

#[cfg(feature = "jemalloc")]
use tikv_jemallocator::Jemalloc;

#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

const ENTRY_COUNT: usize = 10_000;
const THREAD_COUNT: usize = 10;

fn single_thread() {
    let mut map = HashMap::new();

    for i in 0..ENTRY_COUNT {
        map.insert(format!("{}-{}", i, Uuid::new_v4()), i);
    }

    let mut sum = 0;
    for i in 0..ENTRY_COUNT {
        sum += map.get(&format!("{}-{}", i, Uuid::new_v4())).unwrap_or(&0);
    }

    black_box(sum);
}

fn multi_thread() {
    let mut map: HashMap<String, usize> = HashMap::new();
    for i in 0..ENTRY_COUNT {
        map.insert(format!("{}-{}", i, Uuid::new_v4()), i);
    }

    let sum = (0..THREAD_COUNT)
        .into_par_iter()
        .map(|_| {
            (0..ENTRY_COUNT)
                .map(|i| map.get(&format!("{}-{}", i, Uuid::new_v4())).unwrap_or(&0))
                .sum::<usize>()
        })
        .sum::<usize>();

    black_box(sum);
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("single thread", |b| b.iter(single_thread));
    c.bench_function("multi thread", |b| b.iter(multi_thread));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
