//! フィルタ。
//!
//! ここが Python からの移植でいちばん効く所。
//!
//! ラダーは1サンプルずつ前の出力を使うので numpy では書けず、Python 版は
//! numba に頼っていた。Rust では素のループがそのまま機械語になるので、
//! 依存が1つ消えて、JIT の暖機も消える。
//!
//! `highpass` はもっと極端で、Python 版は
//! 「1極ハイパスを1サンプルずつ回すと7万回ループするから、
//! インパルス応答を打ち切って FFT 畳み込みにする」という作りだった。
//! Python では正しい判断だが、ここでは 3 行のループで済む。
//! 実測で `_raw_fft` が合成全体の 26%（21.3 秒中 5.7 秒）を占めていて、
//! その大半がこの関数から来ていた。FFT ごと消える。

use crate::osc::SR;

/// ラダーの方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LadderMode {
    /// 今までの作り。帰還に1サンプルの遅れが入る。
    #[default]
    Classic,
    /// ゼロ遅延帰還。指定した周波数と実際に切れる周波数が合う。
    Zdf,
}

/// 4極ラダー風ローパス（今までの作り）。cutoff は毎サンプルの Hz。
///
/// 係数を 1.0 まで許すと発振する。g=1 では1段が1サンプルで入力に追いつき、
/// そこへレゾナンスの帰還が掛かると際限なく増えて桁あふれする。
/// 係数に上限を置き、各段も軽く飽和させて止める。
pub fn ladder_classic(x: &[f32], cutoff: &[f32], res: f32) -> Vec<f32> {
    let n = x.len();
    let mut y = vec![0.0f32; n];
    if n == 0 {
        return y;
    }
    let fb = res as f64 * 4.0;
    const LIM: f64 = 6.0;
    let (mut s1, mut s2, mut s3, mut s4) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        // cutoff が定数で渡されたときの保険。Python の np.resize と同じ。
        let fc = cutoff[i % cutoff.len()].clamp(20.0, SR * 0.45) as f64;
        let g = (2.0 * (std::f64::consts::PI * fc / SR as f64).sin()).clamp(0.0, 0.92);
        let mut u = x[i] as f64 - fb * s4;
        u = u.clamp(-LIM, LIM);
        s1 += g * (u - s1);
        s2 += g * (s1 - s2);
        s3 += g * (s2 - s3);
        s4 += g * (s3 - s4);
        s4 = s4.clamp(-LIM, LIM);
        y[i] = s4 as f32;
    }
    y
}

/// ZDF（ゼロ遅延帰還）ラダー。
///
/// 4段を通した後の値を先に閉じた形で求めてから各段を進めるので、
/// 帰還に遅れが入らない。
///
/// ```text
/// y4 = (g^4 * x + S) / (1 + k * g^4)
/// ```
pub fn ladder_zdf(x: &[f32], cutoff: &[f32], res: f32, drive: f32) -> Vec<f32> {
    let n = x.len();
    let mut y = vec![0.0f32; n];
    if n == 0 {
        return y;
    }
    let k = res as f64 * 4.0;
    let drv = drive as f64;
    let (mut s1, mut s2, mut s3, mut s4) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let fc = cutoff[i % cutoff.len()].clamp(20.0, SR * 0.49) as f64;
        let big_g = (std::f64::consts::PI * fc / SR as f64).tan();
        let g = big_g / (1.0 + big_g);
        let g2 = g * g;
        let g4 = g2 * g2;
        let one_g = 1.0 - g;
        // 4段の記憶が出口にどれだけ効くか
        let s = one_g * (g2 * g * s1 + g2 * s2 + g * s3 + s4);
        let out = (g4 * x[i] as f64 + s) / (1.0 + k * g4);
        // 帰還の道の飽和。Python 版は exp 2 回で tanh を書いていたが、
        // Rust には tanh がそのままあって、速さも変わらない。
        let fb = if drv > 0.0 {
            (out * drv).tanh() / drv
        } else {
            out
        };
        let u = x[i] as f64 - k * fb;
        let y1 = g * u + one_g * s1;
        let y2 = g * y1 + one_g * s2;
        let y3 = g * y2 + one_g * s3;
        let y4 = g * y3 + one_g * s4;
        // TPT の記憶の更新は s = 2y - s。s = y にすると
        // 1段あたりの切れる周波数がちょうど半分になる。
        s1 = y1 + y1 - s1;
        s2 = y2 + y2 - s2;
        s3 = y3 + y3 - s3;
        s4 = y4 + y4 - s4;
        y[i] = y4 as f32;
    }
    y
}

pub fn ladder(x: &[f32], cutoff: &[f32], res: f32, mode: LadderMode, drive: f32) -> Vec<f32> {
    match mode {
        LadderMode::Zdf => ladder_zdf(x, cutoff, res, drive),
        LadderMode::Classic => ladder_classic(x, cutoff, res),
    }
}

/// 1極ハイパス。ノイズ系の色付け用。
///
/// ```text
/// y[i] = a * (y[i-1] + x[i] - x[i-1])
/// ```
///
/// Python 版はこの式を FFT 畳み込みに化かしていた。ここでは素直に回す。
pub fn highpass(x: &[f32], cutoff: f32) -> Vec<f32> {
    let n = x.len();
    let mut y = vec![0.0f32; n];
    if n == 0 {
        return y;
    }
    let a = (-2.0 * std::f64::consts::PI * cutoff as f64 / SR as f64).exp();
    if a <= 1e-9 {
        return x.to_vec();
    }
    let mut prev_x = 0.0f64;
    let mut prev_y = 0.0f64;
    for i in 0..n {
        let xi = x[i] as f64;
        let yi = a * (prev_y + xi - prev_x);
        y[i] = yi as f32;
        prev_x = xi;
        prev_y = yi;
    }
    y
}

/// RBJ のバンドパス係数。中心 f0、鋭さ q。
fn rbj_bp(f0: f32, q: f32) -> ([f64; 3], [f64; 2]) {
    let w = 2.0 * std::f64::consts::PI * (f0.clamp(20.0, SR * 0.45) as f64) / SR as f64;
    let al = w.sin() / (2.0 * (q.max(0.3) as f64));
    let a0 = 1.0 + al;
    ([al / a0, 0.0, -al / a0], [-2.0 * w.cos() / a0, (1.0 - al) / a0])
}

fn biquad(x: &[f32], b: [f64; 3], a: [f64; 2]) -> Vec<f32> {
    let mut y = vec![0.0f32; x.len()];
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for i in 0..x.len() {
        let x0 = x[i] as f64;
        let y0 = b[0] * x0 + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
        y[i] = y0 as f32;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
    }
    y
}

/// 一定の中心周波数のバンドパス。共鳴体（管や胴）を作るのに使う。
pub fn bandpass(x: &[f32], f0: f32, q: f32) -> Vec<f32> {
    let (b, a) = rbj_bp(f0, q);
    biquad(x, b, a)
}

/// 複数の共鳴の山を足す。table は [(周波数, 鋭さ, 量), ...]。
///
/// 人の声や二枚リードの「しゃべっている感じ」は、倍音の並びではなく
/// 決まった周波数に山があることから来る。音程を変えても山は動かない。
pub fn formants(x: &[f32], table: &[(f32, f32, f32)]) -> Vec<f32> {
    let mut out = vec![0.0f32; x.len()];
    for &(f0, q, g) in table {
        let band = bandpass(x, f0, q);
        for (o, b) in out.iter_mut().zip(&band) {
            *o += b * g;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc;

    /// 信号の実効値。
    fn rms(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| (v * v) as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
    }

    #[test]
    fn lowpass_actually_removes_highs() {
        let hi = osc::saw(8000.0, 24_000, 0.0);
        let cut = vec![300.0f32];
        for (name, y) in [
            ("classic", ladder_classic(&hi, &cut, 0.2)),
            ("zdf", ladder_zdf(&hi, &cut, 0.2, 0.0)),
        ] {
            let before = rms(&hi);
            let after = rms(&y[2000..]); // 立ち上がりを避ける
            assert!(
                after < before * 0.35,
                "{name}: 高い音が削れていない {before} -> {after}"
            );
        }
    }

    #[test]
    fn lowpass_keeps_lows() {
        let lo = osc::saw(80.0, 24_000, 0.0);
        let cut = vec![8000.0f32];
        let y = ladder_zdf(&lo, &cut, 0.1, 0.0);
        let r = rms(&y[2000..]) / rms(&lo[2000..]);
        assert!(r > 0.6, "低い音まで削れている: 倍率 {r}");
    }

    #[test]
    fn ladder_does_not_blow_up_when_wide_open() {
        // Python 版のコメントにある事故。10kHz まで開けて共鳴を上げる。
        let x = osc::saw(220.0, 48_000, 0.0);
        let cut = vec![10_000.0f32];
        for (name, y) in [
            ("classic", ladder_classic(&x, &cut, 0.9)),
            ("zdf", ladder_zdf(&x, &cut, 0.9, 0.0)),
        ] {
            assert!(
                y.iter().all(|v| v.is_finite()),
                "{name}: 値が飛んだ"
            );
            let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak < 50.0, "{name}: 発振している peak={peak}");
        }
    }

    #[test]
    fn highpass_removes_dc() {
        // 直流だけの信号は、ハイパスを通せばほぼ消えるはず。
        let dc = vec![1.0f32; 24_000];
        let y = highpass(&dc, 200.0);
        assert!(
            rms(&y[4000..]) < 0.02,
            "直流が残っている: {}",
            rms(&y[4000..])
        );
    }

    #[test]
    fn highpass_keeps_highs() {
        let hi = osc::saw(6000.0, 24_000, 0.0);
        let y = highpass(&hi, 200.0);
        let r = rms(&y[1000..]) / rms(&hi[1000..]);
        assert!(r > 0.8, "高い音まで削れている: 倍率 {r}");
    }

    #[test]
    fn bandpass_peaks_at_center() {
        // 中心と外れた所で、通る量を比べる。
        let at = osc::saw(1000.0, 24_000, 0.0);
        let off = osc::saw(120.0, 24_000, 0.0);
        let a = rms(&bandpass(&at, 1000.0, 6.0)[2000..]) / rms(&at[2000..]);
        let b = rms(&bandpass(&off, 1000.0, 6.0)[2000..]) / rms(&off[2000..]);
        assert!(a > b * 2.0, "中心が立っていない: 中心 {a} / 外 {b}");
    }

    #[test]
    fn empty_input_is_fine() {
        assert!(ladder_classic(&[], &[440.0], 0.5).is_empty());
        assert!(ladder_zdf(&[], &[440.0], 0.5, 0.0).is_empty());
        assert!(highpass(&[], 200.0).is_empty());
        assert!(bandpass(&[], 800.0, 4.0).is_empty());
    }
}
