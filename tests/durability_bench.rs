use salsa::{Database, Durability, Setter};
use std::time::Instant;

#[salsa::input]
struct StableInput {
    value: u32,
}

#[salsa::input]
struct VolatileInput {
    value: u32,
}

#[salsa::tracked]
fn stable_only(db: &dyn Database, stable: StableInput) -> u32 {
    let mut acc = stable.value(db);
    for i in 0..200 {
        acc = acc.wrapping_mul(1_664_525).wrapping_add(i);
    }
    acc
}

fn run_scenario(stable_durability: Durability, iterations: u32) -> u128 {
    let mut db = salsa::DatabaseImpl::new();
    let stable = StableInput::new(&db, 1);
    stable
        .set_value(&mut db)
        .with_durability(stable_durability)
        .to(1);

    let volatile = VolatileInput::new(&db, 0);
    volatile
        .set_value(&mut db)
        .with_durability(Durability::LOW)
        .to(0);

    let baseline = stable_only(&db, stable);
    let start = Instant::now();
    for i in 0..iterations {
        volatile.set_value(&mut db).to(i);
        assert_eq!(stable_only(&db, stable), baseline);
    }
    start.elapsed().as_micros()
}

#[test]
#[ignore = "measurement harness; run with --ignored --nocapture"]
fn durability_revalidation_bench() {
    let iterations = 5_000;
    let high_us = run_scenario(Durability::HIGH, iterations);
    let low_us = run_scenario(Durability::LOW, iterations);

    eprintln!(
        "durability bench: iterations={iterations}, HIGH={}us, LOW={}us",
        high_us, low_us
    );
}
