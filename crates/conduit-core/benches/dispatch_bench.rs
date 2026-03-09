use criterion::{Criterion, black_box, criterion_group, criterion_main};

use conduit_core::DispatchTable;

fn dispatch_echo(c: &mut Criterion) {
    let table = DispatchTable::new();
    table.register("echo", |payload: Vec<u8>| payload);

    c.bench_function("dispatch echo handler", |b| {
        b.iter_batched(
            || b"hello".to_vec(),
            |payload| {
                let resp = table.dispatch(black_box("echo"), payload).unwrap();
                black_box(resp)
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn dispatch_100_commands(c: &mut Criterion) {
    let table = DispatchTable::new();
    for i in 0..100 {
        let name = format!("cmd_{i:03}");
        table.register(name, |payload: Vec<u8>| payload);
    }

    c.bench_function("dispatch 100 commands (lookup)", |b| {
        b.iter_batched(
            || b"benchmark".to_vec(),
            |payload| {
                let resp = table.dispatch(black_box("cmd_099"), payload).unwrap();
                black_box(resp)
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn register_and_dispatch(c: &mut Criterion) {
    c.bench_function("register + dispatch combined", |b| {
        b.iter(|| {
            let table = DispatchTable::new();
            table.register("bench_cmd", |payload: Vec<u8>| payload);
            let resp = table
                .dispatch(black_box("bench_cmd"), black_box(b"data".to_vec()))
                .unwrap();
            black_box(resp);
        });
    });
}

criterion_group!(
    benches,
    dispatch_echo,
    dispatch_100_commands,
    register_and_dispatch,
);
criterion_main!(benches);
