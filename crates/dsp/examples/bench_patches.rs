use tonescript_dsp::patch::{self, Cfg};
use std::time::Instant;
fn main() {
    let cfg = Cfg::default();
    let n = 24_000; // 0.5秒の音符
    let reps = 20;
    let t = Instant::now();
    let mut sink = 0.0f32;
    for _ in 0..reps {
        for name in patch::NAMES {
            sink += patch::render(name, &cfg, 220.0, n, 0.9, 3).unwrap()[n / 2];
        }
    }
    let ms = t.elapsed().as_secs_f64() * 1e3;
    println!("46音色 x {reps}回 = {} ノート : {:.1} ms  ({:.3} ms/ノート)",
             46 * reps, ms, ms / (46.0 * reps as f64));
    if sink.is_nan() { println!("{sink}"); }
}
