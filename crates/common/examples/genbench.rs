//! Times terrain generation: `cargo run --release -p ourcraft_common --example genbench`
use ourcraft_common::world::ColumnPos;
use ourcraft_common::worldgen::WorldGenerator;

fn main() {
    let g = WorldGenerator::new(1234);
    let start = std::time::Instant::now();
    let n = 100;
    let mut solid = 0;
    for i in 0..n {
        let col = g.generate_column(ColumnPos::new(i % 10, i / 10));
        solid += col.iter().map(|c| c.solid_count()).sum::<usize>();
    }
    let per = start.elapsed() / n as u32;
    println!("{per:?} per column ({solid} solid blocks total)");
}
