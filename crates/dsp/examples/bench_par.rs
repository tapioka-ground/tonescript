//! ノート合成は互いに独立なので、まるごと並列にできる。
//! Python は GIL があるのでここが取れない。Rust の一番大きい取り分。
use tonescript_dsp::patch::{self, Cfg};
use rayon::prelude::*;
use std::time::Instant;

fn main() {
    let cfg = Cfg::default();
    let n = 24_000;
    let reps = 20;
    // 920 ノートぶんの仕事を用意する
    let jobs: Vec<&str> = (0..reps).flat_map(|_| patch::NAMES.iter().copied()).collect();

    let t = Instant::now();
    let a: f32 = jobs.iter().map(|nm| patch::render(nm, &cfg, 220.0, n, 0.9, 3).unwrap()[n / 2]).sum();
    let seq = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let b: f32 = jobs.par_iter().map(|nm| patch::render(nm, &cfg, 220.0, n, 0.9, 3).unwrap()[n / 2]).sum();
    let par = t.elapsed().as_secs_f64() * 1e3;

    println!("CPU 論理コア数: {}", rayon::current_num_threads());
    println!("逐次  {:>8.1} ms", seq);
    println!("並列  {:>8.1} ms   ({:.1}倍)", par, seq / par);
    assert!((a - b).abs() < 1e-3, "並列で結果が変わった");
    println!("結果は逐次と一致");
}
