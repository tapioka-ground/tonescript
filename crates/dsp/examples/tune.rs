//! 弦の音程がどれだけ合っているか。
use tonescript_dsp::osc::SR;
use tonescript_dsp::string::{pluck, Pluck};

fn pitch_of(x: &[f32], near: f32) -> f32 {
    let lo = ((SR / (near * 1.5)) as usize).max(2);
    let hi = ((SR / (near * 0.67)) as usize).min(x.len() / 2);
    let score = |lag: usize| -> f32 {
        let mut sum = 0.0f32;
        for i in 0..x.len() - lag {
            sum += x[i] * x[i + lag];
        }
        sum / (x.len() - lag) as f32
    };
    let mut best = (lo, f32::MIN);
    for lag in lo..hi {
        let s = score(lag);
        if s > best.1 { best = (lag, s); }
    }
    // 山の頂点は整数サンプルの間にある。3点から放物線で頂点を出さないと、
    // 測る側の粗さ（880Hz で 17 セント）が結果に乗ってしまう
    let (l, p) = (best.0, best.1);
    if l > lo && l + 1 < hi {
        let (a, c) = (score(l - 1), score(l + 1));
        let d = 2.0 * (2.0 * p - a - c);
        if d.abs() > 1e-12 {
            let off = (a - c) / d;
            return SR / (l as f32 + off);
        }
    }
    SR / l as f32
}

/// 小数の遅れで自己相関を取る。整数の遅れしか見ないと、周期が 27
/// サンプルしかない高い音では測る側の粗さが 30 セントにもなる。
fn fine_pitch(x: &[f32], near: f32) -> f32 {
    let at = |t: f32| -> f32 {
        let k = t as usize;
        let f = t - k as f32;
        if k + 1 >= x.len() { return 0.0; }
        x[k] * (1.0 - f) + x[k + 1] * f
    };
    let center = SR / near;
    let (mut best, mut score) = (center, f32::MIN);
    let mut lag = center * 0.94;
    while lag < center * 1.06 {
        let n = x.len() - (lag as usize) - 2;
        let mut sum = 0.0f32;
        let mut ea = 0.0f32;
        let mut eb = 0.0f32;
        for i in 0..n {
            let a = x[i];
            let b = at(i as f32 + lag);
            sum += a * b;
            ea += a * a;
            eb += b * b;
        }
        let s = sum / (ea.sqrt() * eb.sqrt()).max(1e-12);
        if s > score { score = s; best = lag; }
        lag += 0.002;
    }
    SR / best
}

fn main() {
    println!("狙い   1周期   管   混ぜ比  フィルタ遅れ  合計");
    for hz in [82.41f32, 440.0, 880.0, 1760.0] {
        let damp = 0.5 - 0.45 * 0.5;
        let (li, frac) = tonescript_dsp::string::tune(hz, damp);
        let fd = tonescript_dsp::string::phase_delay(damp, hz);
        println!("{hz:>7.1} {:>7.2} {li:>4} {frac:>7.3} {fd:>12.3} {:>6.2}",
            SR / hz, li as f32 + frac + fd);
    }
    println!();
    println!("狙い      出た音      ずれ（セント）");
    // ギターの開放弦 6本 + 高いところ
    for hz in [82.41f32, 110.0, 146.83, 196.0, 246.94, 329.63, 440.0, 880.0, 1760.0] {
        let w = pluck(hz, 24_000, Pluck::default(), 1);
        let got = fine_pitch(&w, hz);
        let cents = 1200.0 * (got / hz).log2();
        println!("{hz:>8.2}  {got:>8.2}  {cents:>+8.1}");
    }
}
