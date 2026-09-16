//! Python 版と同じ仕事をさせて時間を測る。
use tonescript_dsp::{filter, osc};
use std::time::Instant;

fn main() {
    let n = 48_000; // 1秒ぶん
    let reps = 100;

    let t = Instant::now();
    let mut sink = 0.0f32;
    for i in 0..reps {
        let y = osc::saw(220.0 + i as f32, n, 0.0);
        sink += y[n / 2];
    }
    println!("osc_saw   {:>8.2} ms  ({} 回 x 1秒)", t.elapsed().as_secs_f64() * 1e3, reps);

    let x = osc::saw(220.0, n, 0.0);
    let cut = vec![1200.0f32];
    let t = Instant::now();
    for _ in 0..reps {
        sink += filter::ladder_classic(&x, &cut, 0.3)[n / 2];
    }
    println!("ladder    {:>8.2} ms  ({} 回 x 1秒)", t.elapsed().as_secs_f64() * 1e3, reps);

    let t = Instant::now();
    for _ in 0..reps {
        sink += filter::highpass(&x, 400.0)[n / 2];
    }
    println!("highpass  {:>8.2} ms  ({} 回 x 1秒)", t.elapsed().as_secs_f64() * 1e3, reps);
    if sink.is_nan() { println!("{sink}"); }
}
