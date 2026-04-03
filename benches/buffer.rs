use criterion::{Criterion, criterion_group, criterion_main};

fn buffer_benchmarks(_c: &mut Criterion) {
    // Benchmarks will be added as buffer module is implemented
}

criterion_group!(benches, buffer_benchmarks);
criterion_main!(benches);
