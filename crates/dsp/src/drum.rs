//! ドラム。11 種。
//!
//! Python 版（synth.py のドラム節）からの移植。層の構成も数字も
//! そのまま持ってきてある。なぜその値なのかは Python 側のコメントに残る。

use crate::env;
use crate::filter::highpass;
use crate::osc::SR;
use crate::rng::noise;
use crate::shape::{add_scaled, drive, mul_in_place, scale, sine, sine_sweep};

/// キックの性格。曲ファイルの `KICK` から上書きされる。
#[derive(Clone, Copy, Debug)]
pub struct KickCfg {
    /// 低音の量。上げると重くなる
    pub weight: f32,
    /// 120〜350Hz の胴鳴り。上げると「厚み」が出る
    pub body: f32,
    /// 頭の硬さ。上げすぎると軽く、カチカチになる
    pub click: f32,
    /// 伸ばし。上げると余韻が長くなる
    pub length: f32,
    /// テールの音程。低いほど沈み、高いほど前に出る
    pub tail_hz: f32,
}

impl Default for KickCfg {
    fn default() -> Self {
        Self {
            weight: 1.0,
            body: 1.0,
            click: 1.0,
            length: 1.0,
            tail_hz: 78.0,
        }
    }
}

/// EDM のキック。3層で作る。
///
///   アタック  頭の「カチッ」。これが無いと小さいスピーカーで消える
///   ボディ    150Hz から一気に落ちる打撃感。ドン の「ド」
///   サブ      45Hz 前後を長めに伸ばす。ドン の「ン」
pub fn kick(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((0.55 * SR) as usize);
    // 170Hz -> 52Hz
    let pitch: Vec<f32> = (0..n)
        .map(|i| 52.0 + 118.0 * (-(i as f32 / SR) / 0.020).exp())
        .collect();
    let mut body = sine_sweep(&pitch, 0.0);
    mul_in_place(&mut body, &env::ad(n, 0.0008, 0.17, 3.4));

    // サブは body より低く、長く残す。ここが胸に来る成分
    let mut sub = sine(45.0, n, 0.3);
    mul_in_place(&mut sub, &env::ad(n, 0.004, 0.30, 2.2));

    let mut click = noise(n, seed);
    mul_in_place(&mut click, &env::ad(n, 0.0002, 0.0035, 7.0));
    scale(&mut click, 0.42);
    let click = highpass(&click, 1200.0);

    let mut out: Vec<f32> = body
        .iter()
        .zip(&sub)
        .map(|(b, s)| b * 0.95 + s * 0.62)
        .collect();
    drive(&mut out, 2.3);
    add_scaled(&mut out, &click, 1.0);
    scale(&mut out, 0.78 * vel);
    out
}

/// ハードスタイル／音ゲーのボス曲のキック。
///
/// 普通のキックとの決定的な違いは「テールが音程を持っていて、それを歪ませる」
/// こと。サイン波を強く歪ませると倍音が一気に増えて、低音なのに前に出る。
pub fn hardkick(n: usize, vel: f32, seed: u64, cfg: &KickCfg, tail_hz: Option<f32>) -> Vec<f32> {
    let tail = tail_hz.unwrap_or(cfg.tail_hz);
    let l = cfg.length;
    let n = n.min((0.70 * SR) as usize);
    let w = cfg.weight;

    let mut sub = sine(46.0, n, 0.3);
    mul_in_place(&mut sub, &env::ad(n, 0.005, 0.26 * l, 2.0));
    let mut punch = sine(tail, n, 0.5);
    mul_in_place(&mut punch, &env::ad(n, 0.002, 0.17 * l, 2.2));

    // 胴鳴り。ノイズを 120〜350Hz だけ残して短く鳴らす
    let mut body = noise(n, seed + 7);
    mul_in_place(&mut body, &env::ad(n, 0.001, 0.075 * l, 3.0));
    let lo = highpass(&body, 120.0);
    let hi = highpass(&body, 350.0);
    let body: Vec<f32> = lo.iter().zip(&hi).map(|(a, b)| a - b).collect();

    // 650Hz -> 230Hz
    let pitch: Vec<f32> = (0..n)
        .map(|i| 230.0 + 420.0 * (-(i as f32 / SR) / 0.008).exp())
        .collect();
    let mut knock = sine_sweep(&pitch, 0.0);
    mul_in_place(&mut knock, &env::ad(n, 0.0004, 0.048, 4.2));
    for v in knock.iter_mut() {
        *v = (*v * 3.2).tanh() * 0.55;
    }

    let mut attack = noise(n, seed);
    mul_in_place(&mut attack, &env::ad(n, 0.0002, 0.009, 5.5));
    let attack = highpass(&attack, 900.0);

    // 低い層はまとめて歪ませる。歪みで倍音が増えて、低音なのに前に出る
    let mut out: Vec<f32> = sub
        .iter()
        .zip(&punch)
        .map(|(s, p)| (((s * 0.95 * w) + (p * 1.45 * w)) * 1.8).tanh() * 0.62)
        .collect();
    add_scaled(&mut out, &body, 1.30 * cfg.body);
    add_scaled(&mut out, &knock, 1.30);
    add_scaled(&mut out, &attack, 1.40 * cfg.click);
    scale(&mut out, 0.86 * vel);
    out
}

/// 時間差のノイズバーストを重ねたクラップ。
pub fn clap(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((0.30 * SR) as usize);
    let mut out = vec![0.0f32; n];
    for (k, off) in [0.0f32, 0.010, 0.019].into_iter().enumerate() {
        let i = (off * SR) as usize;
        if i >= n {
            break;
        }
        let m = n - i;
        let mut seg = noise(m, seed + k as u64);
        mul_in_place(&mut seg, &env::ad(m, 0.0005, 0.014, 5.0));
        let g = 1.0 - 0.2 * k as f32;
        for (o, s) in out[i..].iter_mut().zip(&seg) {
            *o += s * g;
        }
    }
    let mut tail = noise(n, seed + 9);
    mul_in_place(&mut tail, &env::ad(n, 0.002, 0.075, 3.0));
    scale(&mut tail, 0.5);
    add_scaled(&mut out, &tail, 1.0);
    let base = highpass(&out, 1100.0);

    // 高域の抜け。2k〜8k が薄いと、鳴っていても「パチッ」と聞こえない
    let mut snap = noise(n, seed + 4);
    let snap_hp = highpass(&snap, 4500.0);
    snap = snap_hp;
    mul_in_place(&mut snap, &env::ad(n, 0.0004, 0.010, 5.0));

    let mut out: Vec<f32> = base;
    add_scaled(&mut out, &snap, 0.55);
    scale(&mut out, 0.82 * vel);
    out
}

/// ハット。5kHz から残す。7kHz より上だけだと音数の多い曲で存在が消える。
pub fn hat(n: usize, vel: f32, decay: f32, seed: u64) -> Vec<f32> {
    let n = n.min(((decay * 4.0 + 0.02) * SR) as usize);
    let mut x = noise(n, seed);
    mul_in_place(&mut x, &env::ad(n, 0.0003, decay, 4.5));
    let mut out = highpass(&x, 5000.0);
    scale(&mut out, 1.15 * vel);
    out
}

pub fn crash(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((1.6 * SR) as usize);
    let mut x = noise(n, seed);
    mul_in_place(&mut x, &env::ad(n, 0.002, 0.85, 2.2));
    let mut out = highpass(&x, 4000.0);
    scale(&mut out, 0.34 * vel);
    out
}

pub fn snare(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((0.24 * SR) as usize);
    let mut tone = sine(190.0, n, 0.0);
    mul_in_place(&mut tone, &env::ad(n, 0.001, 0.045, 4.0));
    scale(&mut tone, 0.5);
    let mut x = noise(n, seed);
    mul_in_place(&mut x, &env::ad(n, 0.0006, 0.055, 4.0));
    add_scaled(&mut tone, &x, 1.0);
    let mut out = highpass(&tone, 900.0);
    scale(&mut out, 0.42 * vel);
    out
}

/// タム。フィルで下りていく音。キックより高く、短く、音程がある。
pub fn tom(n: usize, vel: f32, freq: f32, seed: u64) -> Vec<f32> {
    let n = n.min((0.45 * SR) as usize);
    let pitch: Vec<f32> = (0..n)
        .map(|i| freq * (1.0 + 0.55 * (-(i as f32 / SR) / 0.035).exp()))
        .collect();
    let mut body = sine_sweep(&pitch, 0.0);
    mul_in_place(&mut body, &env::ad(n, 0.001, 0.13, 3.0));
    let mut skin = highpass(&noise(n, seed), 900.0);
    mul_in_place(&mut skin, &env::ad(n, 0.0004, 0.012, 5.0));
    add_scaled(&mut body, &skin, 0.35);
    drive(&mut body, 1.5);
    scale(&mut body, 0.55 * vel);
    body
}

/// ライド。ハットより柔らかく長い刻み。芯に金属的な倍音を置く。
pub fn ride(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((1.1 * SR) as usize);
    let mut x = highpass(&noise(n, seed), 3000.0);
    mul_in_place(&mut x, &env::ad(n, 0.001, 0.42, 2.2));
    // 「コーン」と鳴る芯。これが無いとただのノイズになる
    let mut ping = vec![0.0f32; n];
    for f in [523.0f32, 787.0, 1174.0] {
        add_scaled(&mut ping, &sine(f, n, 0.0), 1.0);
    }
    mul_in_place(&mut ping, &env::ad(n, 0.001, 0.10, 3.5));
    scale(&mut ping, 0.10);
    let mut out: Vec<f32> = x.iter().map(|v| v * 0.42).collect();
    add_scaled(&mut out, &ping, 1.0);
    scale(&mut out, 0.40 * vel);
    out
}

/// シェイカー。16分を埋める細かい推進力。ハットより丸い。
pub fn shaker(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((0.10 * SR) as usize);
    let mut x = noise(n, seed);
    mul_in_place(&mut x, &env::ad(n, 0.004, 0.020, 3.0));
    let a = highpass(&x, 4000.0);
    let b = highpass(&x, 11000.0);
    let g = 0.55 * vel;
    a.iter().zip(&b).map(|(u, v)| (u - v) * g).collect()
}

/// リムショット。薄いところの刻み。短く硬い。
pub fn rim(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((0.09 * SR) as usize);
    let mut tone = sine(1750.0, n, 0.0);
    mul_in_place(&mut tone, &env::ad(n, 0.0002, 0.010, 6.0));
    let mut click = highpass(&noise(n, seed), 2500.0);
    mul_in_place(&mut click, &env::ad(n, 0.0002, 0.006, 7.0));
    let g = 0.55 * vel;
    tone.iter()
        .zip(&click)
        .map(|(t, c)| (t * 0.5 + c * 0.7) * g)
        .collect()
}

/// リバースシンバル。だんだん膨らんで、頭で切れる。ドロップ直前の吸い込み。
pub fn reverse(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.max((0.2 * SR) as usize);
    let x = highpass(&noise(n, seed), 2200.0);
    let mut out: Vec<f32> = x
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let r = if n <= 1 {
                0.0
            } else {
                i as f32 / (n - 1) as f32
            };
            v * r.powf(2.6)
        })
        .collect();
    let cut = ((0.006 * SR) as usize).min(n);
    if cut > 0 {
        let start = n - cut;
        for (i, v) in out[start..].iter_mut().enumerate() {
            let f = if cut == 1 {
                1.0
            } else {
                1.0 - i as f32 / (cut - 1) as f32
            };
            *v *= f;
        }
    }
    scale(&mut out, 0.42 * vel);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{peak, rms};

    fn all_finite(x: &[f32]) -> bool {
        x.iter().all(|v| v.is_finite())
    }

    #[test]
    fn every_drum_makes_sound_and_stays_finite() {
        let n = (0.5 * SR) as usize;
        let cfg = KickCfg::default();
        let cases: Vec<(&str, Vec<f32>)> = vec![
            ("kick", kick(n, 1.0, 0)),
            ("hardkick", hardkick(n, 1.0, 0, &cfg, None)),
            ("clap", clap(n, 1.0, 0)),
            ("hat", hat(n, 1.0, 0.05, 0)),
            ("crash", crash(n, 1.0, 0)),
            ("snare", snare(n, 1.0, 0)),
            ("tom", tom(n, 1.0, 180.0, 0)),
            ("ride", ride(n, 1.0, 0)),
            ("shaker", shaker(n, 1.0, 0)),
            ("rim", rim(n, 1.0, 0)),
            ("reverse", reverse(n, 1.0, 0)),
        ];
        for (name, y) in cases {
            assert!(!y.is_empty(), "{name}: 何も返らない");
            assert!(all_finite(&y), "{name}: 値が飛んだ");
            assert!(rms(&y) > 1e-4, "{name}: 無音 rms={}", rms(&y));
            assert!(peak(&y) < 8.0, "{name}: 大きすぎる peak={}", peak(&y));
        }
    }

    #[test]
    fn velocity_scales_level() {
        let n = (0.3 * SR) as usize;
        let soft = rms(&kick(n, 0.4, 0));
        let loud = rms(&kick(n, 1.0, 0));
        assert!(loud > soft * 2.0, "強弱が効いていない {soft} -> {loud}");
    }

    #[test]
    fn kick_is_low_and_hat_is_high() {
        // キックは低域、ハットは高域に重心があるはず。
        // ざっくり「ハイパスを通して何割残るか」で見る。
        let n = (0.3 * SR) as usize;
        let k = kick(n, 1.0, 0);
        let h = hat(n, 1.0, 0.05, 0);
        let k_hi = rms(&highpass(&k, 2000.0)) / rms(&k);
        let h_hi = rms(&highpass(&h, 2000.0)) / rms(&h);
        assert!(k_hi < 0.5, "キックに高域が多すぎる: {k_hi}");
        assert!(h_hi > 0.8, "ハットに高域が足りない: {h_hi}");
    }

    #[test]
    fn reverse_swells_then_cuts() {
        let n = (0.8 * SR) as usize;
        let y = reverse(n, 1.0, 0);
        let head = rms(&y[..n / 8]);
        let mid = rms(&y[n / 2..n * 5 / 8]);
        assert!(mid > head * 3.0, "膨らんでいない {head} -> {mid}");
        assert!(y[n - 1].abs() < 1e-4, "頭で切れていない");
    }

    #[test]
    fn hardkick_config_changes_weight() {
        let n = (0.5 * SR) as usize;
        let light = KickCfg { weight: 0.5, ..Default::default() };
        let heavy = KickCfg { weight: 1.6, ..Default::default() };
        let a = rms(&hardkick(n, 1.0, 0, &light, None));
        let b = rms(&hardkick(n, 1.0, 0, &heavy, None));
        assert!(b > a, "weight が効いていない {a} -> {b}");
    }
}
