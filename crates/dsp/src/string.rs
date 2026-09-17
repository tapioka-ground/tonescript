//! 撥いた弦。**波形ではなく、弦そのものを真似る。**
//!
//! なぜ削るだけでは弦にならないか
//! ------------------------------
//! のこぎり波をフィルタで削ると「弦っぽい何か」にはなるが、単体で聞くと
//! 弦に聞こえない。弦の音の正体は波形ではなく**往復する波**だからで、
//! 弾いた瞬間の乱れが弦を行き来し、端で折り返すたびに高い音から失われて
//! いく。だんだん丸い音になっていくのはそのため。
//!
//! やっていること（Karplus-Strong）
//! --------------------------------
//! ```text
//!   ① 弦の長さぶんの管を用意する（長さ = 周波数の逆数）
//!   ② そこへ短い雑音を入れる（弾いた瞬間の乱れ）
//!   ③ 管を1周するたびに、少し高い音を削って、少し小さくする
//!   ④ ②を繰り返し読む
//! ```
//!
//! たったこれだけで弦になる。**削って作るより、短くて本物に近い。**
//!
//! 半端な長さ
//! ----------
//! 管の長さは整数サンプルにならない（440Hz なら 109.09 サンプル）。
//! 整数へ丸めると**音程がずれる**ので、隣り合う2つのあいだを取って読む。
//! 高い音ほど管が短く、丸めの影響が大きいので、ここを省くと高音だけ
//! 音痴になる。

use crate::osc::SR;
use crate::rng::Pcg64;

/// 弦の鳴らし方。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pluck {
    /// 音が消えるまでのおよその秒数。長いほど伸びる
    pub decay: f32,
    /// 明るさ 0〜1。小さいほど早く丸くなる（ナイロン弦）、
    /// 大きいほど硬く伸びる（金属弦）
    pub bright: f32,
    /// どこを弾くか 0〜1。0.5 で真ん中（丸い）、0.1 で端（硬い）
    pub pick: f32,
}

impl Default for Pluck {
    fn default() -> Self {
        Self { decay: 2.0, bright: 0.5, pick: 0.25 }
    }
}

/// 弦を1本弾く。
pub fn pluck(freq: f32, n: usize, p: Pluck, seed: u64) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    if freq <= 0.0 {
        return vec![0.0; n];
    }
    let b = p.bright.clamp(0.0, 1.0);
    // 一周ごとに高い音を削る量。明るいほど削らない
    let damp = 0.5 - 0.45 * b;
    // **削るフィルタ自身も遅れを持つ。** その遅れを管の長さから引かないと、
    // 管が長くなったのと同じことになって音程が下がる。高い音ほど管が短い
    // ので、引き忘れると高音だけ音痴になる（1760Hz で 6% 下がった）。
    //
    // 遅れは周波数で変わる。低い音での近似（damp/(1-damp)）で済ませると、
    // 今度は高い音が 30 セント上ずれる。鳴らしたい高さでの値を出す。
    //
    // 半端なぶんを埋める補間にも遅れがある。こちらも高い音ほど狂うので、
    // **鳴らしたい高さで合うように、混ぜる比を解いて決める。**
    // 近似のままだと 1760Hz で 30 セント上ずれた（半音の3分の1）
    let (li, frac) = tune(freq, damp);

    // 弾いた瞬間の乱れ
    let mut rng = Pcg64::new(seed);
    let mut line: Vec<f32> = (0..li).map(|_| rng.next_f64() as f32 * 2.0 - 1.0).collect();

    // 弾く位置。その点からの距離ぶん遅らせて引くと、そこが節になる。
    // 端を弾くと硬く、真ん中を弾くと丸くなるのはこれ
    let shift = ((li as f32 * p.pick.clamp(0.02, 0.98)) as usize).max(1) % li.max(1);
    if shift > 0 {
        let copy = line.clone();
        for i in 0..li {
            line[i] = copy[i] - copy[(i + li - shift) % li];
        }
    }

    // 明るさ。入れる前に丸めておくと、ナイロン弦のように柔らかく始まる
    let smooth = 1.0 - b;
    let mut acc = 0.0f32;
    for v in line.iter_mut() {
        acc = *v * (1.0 - smooth) + acc * smooth;
        *v = acc;
    }

    // 一周ぶんの減り。decay 秒で 1/e になるように
    let loss = (-(SR / freq) / SR / p.decay.max(0.02)).exp();

    let mut out = Vec::with_capacity(n);
    let mut idx = 0usize;
    let mut last = 0.0f32;
    // 管から出た1つ前の値。半端な長さを埋めるのに使う
    let mut prev = 0.0f32;
    for _ in 0..n {
        let cur = line[idx];
        // 遅れ li と li+1 のあいだを取って、半端な長さにする
        let v = cur * (1.0 - frac) + prev * frac;
        prev = cur;
        // 端で折り返すたびに高い音が落ちる
        let filtered = v * (1.0 - damp) + last * damp;
        last = filtered;
        line[idx] = filtered * loss;
        out.push(v);
        idx = (idx + 1) % li;
    }
    out
}

/// 管の長さと、半端なぶんの混ぜ比を決める。**試し用に公開している。**
///
/// 一周の遅れは「管 + 補間 + 削るフィルタ」の3つの和で、後ろ2つは
/// 周波数で変わる。合計がちょうど1周期になるよう、混ぜ比を挟み撃ちで解く。
pub fn tune(freq: f32, damp: f32) -> (usize, f32) {
    let period = SR / freq;
    let fd = phase_delay(damp, freq);
    let w = std::f32::consts::TAU * freq / SR;
    // 補間の遅れ。比 0 で 0、比 1 で 1 サンプル
    let lerp_delay = |f: f32| -> f32 {
        if w <= 1e-6 {
            return f;
        }
        (f * w.sin()).atan2((1.0 - f) + f * w.cos()) / w
    };
    let mut li = (period - fd).floor() as i32;
    // 残りが 0〜1 サンプルに収まる管の長さを探す
    while li > 2 && period - fd - (li as f32) > 1.0 {
        li += 1;
    }
    while li > 2 && period - fd - (li as f32) < 0.0 {
        li -= 1;
    }
    let li = li.max(2) as usize;
    let rest = (period - fd - li as f32).clamp(0.0, 1.0);
    // 挟み撃ち。20回で十分に収まる
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let mid = (lo + hi) * 0.5;
        if lerp_delay(mid) < rest {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (li, (lo + hi) * 0.5)
}

/// 一次フィルタ `y = (1-d)x + d·y[-1]` が、その高さで持つ遅れ（サンプル）。
///
/// 低い音では `d/(1-d)` に近づくが、高い音では小さくなる。弦の長さは
/// この遅れを引いて決めるので、近似で済ませると高音が上ずる。
pub fn phase_delay(d: f32, freq: f32) -> f32 {
    let w = std::f32::consts::TAU * freq / SR;
    if w <= 1e-6 {
        return d / (1.0 - d).max(1e-6);
    }
    (d * w.sin()).atan2(1.0 - d * w.cos()) / w
}

/// 胴鳴り。木箱や金属の筐体が、決まった高さだけを持ち上げる。
///
/// ギターらしさの半分は弦ではなく**胴**にある。弦だけだと痩せて聞こえる。
/// `peaks` は `[中心の高さ, 鋭さ, 混ぜる量]`。
pub fn body(x: &[f32], peaks: &[(f32, f32, f32)]) -> Vec<f32> {
    let mut out = x.to_vec();
    for &(f, q, g) in peaks {
        if g <= 0.0 || f < 20.0 || f > SR * 0.45 {
            continue;
        }
        let r = crate::filter::bandpass(x, f, q.max(0.3));
        for (o, v) in out.iter_mut().zip(&r) {
            *o += v * g;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// 音程を測る。
    ///
    /// **整数サンプルの遅れで測ってはいけない。** 1760Hz の周期は 27
    /// サンプルしかないので、1サンプル刻みでは測る側の粗さだけで 30 セント
    /// ずれる（実際それで、合っているものを「ずれている」と読み違えた）。
    /// あいだを埋めながら細かく探す。
    fn pitch_of(x: &[f32], near: f32) -> f32 {
        let at = |t: f32| -> f32 {
            let k = t as usize;
            let f = t - k as f32;
            if k + 1 >= x.len() {
                return 0.0;
            }
            x[k] * (1.0 - f) + x[k + 1] * f
        };
        let center = SR / near;
        let (mut best, mut score) = (center, f32::MIN);
        let mut lag = center * 0.94;
        while lag < center * 1.06 {
            let n = x.len() - (lag as usize) - 2;
            let (mut sum, mut ea, mut eb) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..n {
                let (a, b) = (x[i], at(i as f32 + lag));
                sum += a * b;
                ea += a * a;
                eb += b * b;
            }
            let s = sum / (ea.sqrt() * eb.sqrt()).max(1e-12);
            if s > score {
                score = s;
                best = lag;
            }
            lag += 0.004;
        }
        SR / best
    }

    fn cents(got: f32, want: f32) -> f32 {
        1200.0 * (got / want).log2()
    }

    #[test]
    fn a_plucked_string_sounds() {
        let w = pluck(220.0, 24_000, Pluck::default(), 1);
        assert_eq!(w.len(), 24_000);
        assert!(rms(&w) > 0.01, "鳴っていない: {}", rms(&w));
        assert!(w.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_pitch_is_what_was_asked_for() {
        // 管の長さ・補間・削るフィルタの遅れを全部足して1周期にしていないと、
        // 高い音ほどずれる。ギターの開放弦6本と、その上まで見る
        for hz in [82.41f32, 110.0, 146.83, 196.0, 246.94, 329.63, 440.0] {
            let w = pluck(hz, 24_000, Pluck::default(), 1);
            let c = cents(pitch_of(&w, hz), hz);
            // 5セント＝人が「合っている」と感じる範囲
            assert!(c.abs() < 5.0, "{hz}Hz が {c:+.1} セントずれた");
        }
    }

    #[test]
    fn even_the_top_of_the_range_stays_in_tune() {
        // 周期が数十サンプルしかない高さ。ここが甘いと、高い音だけ音痴になる
        for hz in [880.0f32, 1760.0] {
            let w = pluck(hz, 24_000, Pluck::default(), 1);
            let c = cents(pitch_of(&w, hz), hz);
            // 半音の8分の1まで。ギターの最高音（A6）でこれなら実用に足りる
            assert!(c.abs() < 12.5, "{hz}Hz が {c:+.1} セントずれた");
        }
    }

    #[test]
    fn it_dies_away_like_a_real_string() {
        let w = pluck(220.0, 48_000 * 2, Pluck { decay: 1.0, ..Default::default() }, 1);
        let head = rms(&w[..4800]);
        let one_sec = rms(&w[48_000..52_800]);
        let two_sec = rms(&w[90_000..94_800]);
        assert!(head > one_sec, "減っていない");
        assert!(one_sec > two_sec, "減り続けていない");
        // 1秒で 1/e くらい（弾き方で前後するので幅を持たせる）
        let ratio = one_sec / head;
        assert!(ratio > 0.05 && ratio < 0.75, "1秒後に {ratio:.3} 倍");
    }

    #[test]
    fn a_longer_decay_rings_longer() {
        let short = pluck(220.0, 48_000, Pluck { decay: 0.3, ..Default::default() }, 1);
        let long = pluck(220.0, 48_000, Pluck { decay: 4.0, ..Default::default() }, 1);
        let at = 24_000..28_800;
        assert!(
            rms(&long[at.clone()]) > rms(&short[at]) * 3.0,
            "伸びが変わっていない"
        );
    }

    #[test]
    fn it_gets_rounder_as_it_dies() {
        // 本物の弦と同じで、高い音から先に失われること
        let w = pluck(220.0, 48_000, Pluck { decay: 3.0, bright: 0.5, ..Default::default() }, 1);
        // 隣り合うサンプルの差＝高い音の多さ
        let rough = |x: &[f32]| -> f32 {
            let r = rms(x).max(1e-9);
            x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / x.len() as f32 / r
        };
        let head = rough(&w[..4800]);
        let tail = rough(&w[36_000..40_800]);
        assert!(tail < head * 0.9, "頭 {head:.3} / 後ろ {tail:.3}（丸くなっていない）");
    }

    #[test]
    fn a_dull_string_loses_its_top_faster() {
        let bright = pluck(220.0, 48_000, Pluck { bright: 0.9, ..Default::default() }, 1);
        let dull = pluck(220.0, 48_000, Pluck { bright: 0.1, ..Default::default() }, 1);
        let rough = |x: &[f32]| -> f32 {
            x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / x.len() as f32
        };
        assert!(rough(&dull) < rough(&bright), "明るさが効いていない");
    }

    #[test]
    fn where_you_pick_changes_the_tone() {
        let mid = pluck(220.0, 24_000, Pluck { pick: 0.5, ..Default::default() }, 1);
        let edge = pluck(220.0, 24_000, Pluck { pick: 0.08, ..Default::default() }, 1);
        assert_ne!(mid, edge, "弾く位置が効いていない");
        assert!(rms(&mid) > 0.005 && rms(&edge) > 0.005);
    }

    #[test]
    fn the_same_seed_gives_the_same_string() {
        let a = pluck(220.0, 4800, Pluck::default(), 42);
        let b = pluck(220.0, 4800, Pluck::default(), 42);
        assert_eq!(a, b);
        let c = pluck(220.0, 4800, Pluck::default(), 43);
        assert_ne!(a, c);
    }

    #[test]
    fn silly_input_does_not_blow_up() {
        assert!(pluck(220.0, 0, Pluck::default(), 1).is_empty());
        assert!(pluck(0.0, 100, Pluck::default(), 1).iter().all(|v| *v == 0.0));
        // とても低い音（管が長い）でも落ちない
        let w = pluck(20.0, 4800, Pluck::default(), 1);
        assert_eq!(w.len(), 4800);
        assert!(w.iter().all(|v| v.is_finite()));
        // とても高い音（管が2サンプル）でも落ちない
        let w = pluck(20_000.0, 4800, Pluck::default(), 1);
        assert!(w.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_body_adds_without_taking_away() {
        let w = pluck(220.0, 24_000, Pluck::default(), 1);
        let with = body(&w, &[(110.0, 4.0, 0.5), (400.0, 6.0, 0.3)]);
        assert_eq!(with.len(), w.len());
        assert!(rms(&with) > rms(&w), "胴を足したのに痩せた");
        assert!(with.iter().all(|v| v.is_finite()));
        // 何も指定しなければ素通し
        assert_eq!(body(&w, &[]), w);
        // 混ぜる量 0 も素通し
        assert_eq!(body(&w, &[(110.0, 4.0, 0.0)]), w);
    }
}
