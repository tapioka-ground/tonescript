//! 楽器ごとの音量を並べる。持ち替えたときに段差が出ないか見るため。
use tonescript_dsp::{kit, recipe};
fn main() {
    let win = 48_000 * 2 / 10; // 頭の 0.2 秒。撥弦も伸ばす音も、まだ鳴っている
    let which = std::env::args().nth(1).unwrap_or_else(|| "kit".into());
    let names: Vec<&str> = if which == "builtin" {
        tonescript_dsp::patch::NAMES.to_vec()
    } else {
        kit::NAMES.to_vec()
    };
    let cfg = tonescript_dsp::patch::Cfg::default();
    let mut v: Vec<(f32, f32, &str)> = names
        .iter()
        .map(|n| {
            let one = |hz: f32| match kit::get(n) {
                Some(r) => recipe::render(&r, hz, 48_000, 1.0, 7),
                None => tonescript_dsp::patch::render(n, &cfg, hz, 48_000, 1.0, 7).unwrap(),
            };
            // 実効値は真ん中の高さで。**ピークは低い音でいちばん大きくなる**
            // ことがあるので、低い音から高い音まで見る
            let mid = one(261.63);
            let head = &mid[..win.min(mid.len())];
            let rms = (head.iter().map(|x| x * x).sum::<f32>() / head.len() as f32).sqrt();
            let peak = [82.41f32, 261.63, 880.0]
                .iter()
                .map(|hz| one(*hz).iter().fold(0.0f32, |a, b| a.max(b.abs())))
                .fold(0.0f32, f32::max);
            (rms, peak, *n)
        })
        .collect();
    v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    if which == "csv" {
        // 直す側が読む用
        for (r, p, n) in &v {
            println!("{n},{r:.6},{p:.6}");
        }
        return;
    }
    println!("{:<16} {:>8} {:>8}", "楽器", "実効", "ピーク");
    for (r, p, n) in &v {
        println!("{n:<16} {r:>8.4} {p:>8.3}");
    }
    println!("\n最大/最小 = {:.1} 倍", v[0].0 / v[v.len() - 1].0);
}
