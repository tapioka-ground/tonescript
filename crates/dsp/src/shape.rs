//! 音を作るときの小道具。歪み、倍音足し、ビブラート、息、ディレイ。
//!
//! Python 版で `np.cumsum` や内包表記で書かれていた所は、
//! ここでは素直なループにしてある。配列を何本も作らないぶん速い。

use crate::env;
use crate::filter;
use crate::osc::SR;
use crate::rng::{noise, Pcg64};

const TAU: f32 = std::f32::consts::TAU;

/// 歪み。`tanh(x * amount) / tanh(amount)`。
#[inline]
pub fn drive(x: &mut [f32], amount: f32) {
    let norm = 1.0 / amount.tanh();
    for v in x.iter_mut() {
        *v = (*v * amount).tanh() * norm;
    }
}

/// 歪ませた新しい配列を返す版。
pub fn driven(x: &[f32], amount: f32) -> Vec<f32> {
    let mut out = x.to_vec();
    drive(&mut out, amount);
    out
}

/// 音程が毎サンプル変わるサイン波。
///
/// Python の `sin(2π * cumsum(pitch) / SR)` と同じ。位相の積み上げは
/// f64 で行う。f32 だと長い音で位相がずれていく。
pub fn sine_sweep(pitch: &[f32], phase0: f32) -> Vec<f32> {
    let mut ph = phase0 as f64;
    let inv = 1.0 / SR as f64;
    pitch
        .iter()
        .map(|&p| {
            ph += p as f64 * inv;
            (std::f64::consts::TAU * ph).sin() as f32
        })
        .collect()
}

/// 一定の周波数のサイン波。
pub fn sine(freq: f32, n: usize, phase0: f32) -> Vec<f32> {
    (0..n)
        .map(|i| (TAU * freq * (i as f32 / SR) + phase0).sin())
        .collect()
}

/// 倍音を1本ずつ足す。table は (倍率, 音量, 減衰秒)。
///
/// 倍率が整数でないものを混ぜると鐘や銅鑼になる。整数だけなら楽音。
pub fn partials(freq: f32, n: usize, table: &[(f32, f32, f32)], seed: u64, detune: f32) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let mut out = vec![0.0f32; n];
    for &(mul, amp, dec) in table {
        let f = freq * mul * (1.0 + detune * (rng.next_f64() as f32 - 0.5));
        let phase = rng.next_f64() as f32 * 6.283;
        if f >= SR * 0.48 {
            continue;
        }
        let d = dec.max(1e-3);
        for (i, o) in out.iter_mut().enumerate() {
            let t = i as f32 / SR;
            *o += (TAU * f * t + phase).sin() * (-t / d).exp() * amp;
        }
    }
    out
}

/// 倍音を足すが、周波数が高すぎたらそこで打ち切る版。
///
/// Python 側で `break` していた所。`continue` と結果が変わるので分けてある。
pub fn partials_break(
    freq: f32,
    n: usize,
    table: &[(f32, f32, f32)],
    rng: &mut Pcg64,
    limit: f32,
) -> Vec<f32> {
    let mut out = vec![0.0f32; n];
    for &(mul, amp, dec) in table {
        let f = freq * mul;
        if f > limit {
            break;
        }
        let phase = rng.next_f64() as f32 * 6.283;
        let d = dec.max(1e-3);
        for (i, o) in out.iter_mut().enumerate() {
            let t = i as f32 / SR;
            *o += (TAU * f * t + phase).sin() * (-t / d).exp() * amp;
        }
    }
    out
}

/// ビブラート。すぐには掛からず、伸ばしているうちに深くなる。
pub fn vib(n: usize, rate: f32, depth: f32, delay_s: f32, grow: f32, seed: u64) -> Vec<f32> {
    let ph = Pcg64::new(seed).next_f64() as f32 * 6.283;
    let g = grow.max(1e-3);
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let amt = ((t - delay_s) / g).clamp(0.0, 1.0);
            1.0 + depth * amt * (TAU * rate * t + ph).sin()
        })
        .collect()
}

/// 息の音。笛の「シュー」。
///
/// 笛は弦と違って、鳴っている間ずっと空気が乱れている。この雑音が
/// 無いと、どれだけ倍音を並べても「笛の形をしたシンセ」にしかならない。
pub fn breath(n: usize, f0: f32, seed: u64, low: f32, high: f32, q: f32) -> Vec<f32> {
    let x = noise(n, seed);
    let c = (f0 * 3.0).clamp(low, high);
    let a = filter::bandpass(&x, c, q);
    let b = filter::bandpass(&x, c * 2.1, q * 1.4);
    let mixed: Vec<f32> = a.iter().zip(&b).map(|(u, v)| u + v * 0.5).collect();
    filter::highpass(&mixed, 600.0f32.max(f0 * 1.2))
}

/// テンポ同期ディレイ。
pub fn delay(x: &[f32], time_s: f32, feedback: f32, mix: f32, taps: usize) -> Vec<f32> {
    let d = (time_s * SR) as usize;
    if d < 1 {
        return x.to_vec();
    }
    let mut out = x.to_vec();
    let mut g = 1.0f32;
    for k in 1..=taps {
        g *= feedback;
        if g < 0.01 || k * d >= x.len() {
            break;
        }
        let shift = k * d;
        let amount = g * mix;
        for i in shift..x.len() {
            out[i] += x[i - shift] * amount;
        }
    }
    out
}

/// 2 本を足す。長さは左に合わせる。
#[inline]
pub fn add_scaled(dst: &mut [f32], src: &[f32], gain: f32) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d += s * gain;
    }
}

/// 掛ける。
#[inline]
pub fn mul_in_place(dst: &mut [f32], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d *= s;
    }
}

/// 定数倍。
#[inline]
pub fn scale(dst: &mut [f32], g: f32) {
    for d in dst.iter_mut() {
        *d *= g;
    }
}

/// 高い方だけを残した雑音に、エンベロープを掛けたもの。
/// 打点の「カチッ」を作るのに何度も出てくる形。
pub fn click(n: usize, seed: u64, hp: f32, attack: f32, decay: f32, curve: f32) -> Vec<f32> {
    let mut x = noise(n, seed);
    let e = env::ad(n, attack, decay, curve);
    mul_in_place(&mut x, &e);
    filter::highpass(&x, hp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_is_bounded_and_unity_at_one() {
        let mut x = vec![1.0f32, -1.0, 0.0, 5.0];
        drive(&mut x, 2.6);
        // 入力 1.0 はちょうど 1.0 に写る（正規化の定義）
        assert!((x[0] - 1.0).abs() < 1e-6);
        assert!((x[1] + 1.0).abs() < 1e-6);
        assert!(x[2].abs() < 1e-9);
        // 大きな入力も頭打ちになる
        assert!(x[3] < 1.05);
    }

    #[test]
    fn sine_sweep_tracks_pitch() {
        // 一定の音程を渡せば、ふつうのサイン波と同じになるはず
        let n = 4800;
        let p = vec![440.0f32; n];
        let a = sine_sweep(&p, 0.0);
        // cumsum は「先に足してから」なので 1 サンプルぶん進んでいる
        let b = sine(440.0, n + 1, 0.0);
        for i in 0..100 {
            assert!((a[i] - b[i + 1]).abs() < 1e-4, "i={i}: {} {}", a[i], b[i + 1]);
        }
    }

    #[test]
    fn delay_adds_echo() {
        let mut x = vec![0.0f32; 48_000];
        x[0] = 1.0;
        let y = delay(&x, 0.25, 0.5, 1.0, 4);
        let d = (0.25 * SR) as usize;
        assert!((y[d] - 0.5).abs() < 1e-6, "1回目の返し: {}", y[d]);
        assert!((y[2 * d] - 0.25).abs() < 1e-6, "2回目の返し: {}", y[2 * d]);
    }

    #[test]
    fn vib_starts_flat() {
        let v = vib(48_000, 5.4, 0.01, 0.25, 0.35, 0);
        assert!((v[0] - 1.0).abs() < 1e-6, "頭から揺れている");
        let late = &v[40_000..];
        let span = late.iter().fold(0.0f32, |m, x| m.max((x - 1.0).abs()));
        assert!(span > 0.005, "伸ばしても揺れていない: {span}");
    }

    #[test]
    fn breath_is_high_passed() {
        let b = breath(24_000, 440.0, 3, 1200.0, 5000.0, 0.9);
        assert_eq!(b.len(), 24_000);
        assert!(b.iter().all(|v| v.is_finite()));
    }
}
