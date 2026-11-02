//! Finds a seed whose spawn is near a good-sized village, for looking at
//! villages by eye: `cargo run --release -p ourcraft_common --example findvillage`

use ourcraft_common::worldgen::WorldGenerator;
fn main() {
    for seed in 1..400u64 {
        let g = WorldGenerator::new(seed);
        let (sx, sy, sz) = g.find_spawn();
        for cz in -2..2 {
            for cx in -2..2 {
                if let Some(s) = g.village_site(cx, cz) {
                    let d = (((s.x - sx).pow(2) + (s.z - sz).pow(2)) as f32).sqrt();
                    let n = g.village_plan(s).houses.len();
                    if d < 120.0 && n >= 5 {
                        println!(
                            "seed {seed} spawn {sx},{sy},{sz} village {:?} dist {d:.0} houses {n}",
                            s
                        );
                        return;
                    }
                }
            }
        }
    }
}
