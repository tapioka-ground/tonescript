//! ミックスとマスタリング。
//!
//! Python 版の `mp.py` のミックス節に当たる。考え方はそのまま持ってきた。
//!
//!   - トラックごとの上限は絶対値ではなく「実効値の何倍まで許すか」で持つ。
//!     打楽器は頭が鋭いのが正しいので、絶対値で揃えるとキックが死ぬ。
//!   - 全体はピークではなく音圧（LUFS）で合わせる。ピーク基準にすると
//!     「1本足したら全体が下がる」という起き方をする。
//!   - サイドチェインはキックの位置から作る。

use tonescript_dsp::filter::highpass;
use tonescript_dsp::osc::SR;
use tonescript_dsp::{peak, rms};

/// 左右2本。
pub struct Stereo {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl Stereo {
    pub fn silent(n: usize) -> Self {
        Self { l: vec![0.0; n], r: vec![0.0; n] }
    }
    pub fn len(&self) -> usize {
        self.l.len()
    }
    pub fn is_empty(&self) -> bool {
        self.l.is_empty()
    }
    /// 足す。
    pub fn add(&mut self, other: &Stereo, gain: f32) {
        for (d, s) in self.l.iter_mut().zip(&other.l) {
            *d += s * gain;
        }
        for (d, s) in self.r.iter_mut().zip(&other.r) {
            *d += s * gain;
        }
    }
    pub fn scale(&mut self, g: f32) {
        for v in self.l.iter_mut().chain(self.r.iter_mut()) {
            *v *= g;
        }
    }
    pub fn peak(&self) -> f32 {
        peak(&self.l).max(peak(&self.r))
    }
}

/// 線を「1サンプルごとの値」へ広げる。
///
/// 節は目盛り（16分）で書かれている。テンポが動くと目盛りの長さが
/// 変わるので、目盛り→秒の表を通してから引き伸ばす。
pub fn curve_to_samples(
    curve: &tonescript_song::model::Curve,
    step_times: &[f64],
    total: usize,
    sr: f32,
) -> Option<Vec<f32>> {
    if curve.is_empty() {
        return None;
    }
    // 各目盛りの始まるサンプル位置
    let at = |i: usize| -> usize { (step_times[i.min(step_times.len() - 1)] * sr as f64) as usize };
    let mut out = vec![0.0f32; total];
    let last_step = step_times.len().saturating_sub(1);
    let mut i = 0usize;
    for step in 0..last_step {
        let (s, e) = (at(step), at(step + 1).min(total));
        if s >= total {
            break;
        }
        let v0 = curve.at(step as f32).unwrap();
        let v1 = curve.at((step + 1) as f32).unwrap();
        let n = e.saturating_sub(s).max(1);
        for k in 0..n {
            let j = s + k;
            if j >= total {
                break;
            }
            // 目盛りの中も直線で繋ぐ。段差にすると音量が階段状に動いて
            // 「ジッ」と聞こえる
            out[j] = v0 + (v1 - v0) * (k as f32 / n as f32);
        }
        i = e;
    }
    // 曲の終わりから後ろは最後の値のまま
    let tail = curve.at(last_step as f32).unwrap();
    for v in out[i.min(total)..].iter_mut() {
        *v = tail;
    }
    Some(out)
}

/// 左右へ振る。-1 が左、0 が中央、+1 が右。
///
/// 等出力（中央でも端でも全体の大きさが変わらない）になるよう
/// sin/cos で配る。単純な線形だと中央が 3dB へこむ。
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let t = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5; // 0..1
    let a = t * std::f32::consts::FRAC_PI_2;
    (a.cos(), a.sin())
}

/// モノラルを左右へ広げる。
///
/// width 0 で完全中央、1 以上で広がる。広げるのは「左右で少しずらす」のでは
/// なく、逆相ぶんを混ぜる形。低音を広げると芯がぼやけるので、低いパートは
/// 曲ファイル側で width 0 にしてある。
pub fn widen(mono: &[f32], width: f32) -> Stereo {
    if width <= 0.0 {
        return Stereo { l: mono.to_vec(), r: mono.to_vec() };
    }
    let s = width * 0.5;
    let l = mono.iter().map(|v| v * (1.0 + s)).collect();
    let r = mono.iter().map(|v| v * (1.0 - s)).collect();
    // 広げても全体の大きさが変わらないように正規化する
    let mut out = Stereo { l, r };
    let g = 1.0 / (1.0 + s * s).sqrt();
    out.scale(g);
    out
}

/// キックの位置で凹むゲイン曲線。EDM の「ポンプ感」の正体。
pub fn sidechain_env(
    kick_at: &[usize],
    total: usize,
    depth: f32,
    attack: f32,
    hold: f32,
    release: f32,
) -> Vec<f32> {
    let mut env = vec![1.0f32; total];
    let a = ((attack * SR) as usize).max(1);
    let h = ((hold * SR) as usize).max(1);
    let r = ((release * SR) as usize).max(1);
    let shape_len = a + h + r;
    let mut shape = Vec::with_capacity(shape_len);
    for i in 0..a {
        shape.push(1.0 - depth * (i as f32 / a as f32));
    }
    for _ in 0..h {
        shape.push(1.0 - depth);
    }
    for i in 0..r {
        shape.push(1.0 - depth + depth * (i as f32 / r as f32));
    }
    for &k in kick_at {
        for (i, s) in shape.iter().enumerate() {
            let j = k + i;
            if j >= total {
                break;
            }
            // 重なったところは深いほうを採る。凹みが足し算で消えないように
            env[j] = env[j].min(*s);
        }
    }
    env
}

/// トラックの頭を丸める。
///
/// 絶対値で全トラックを揃えるのは間違い。打楽器は頭が鋭いのが正しく、
/// 一律に止めるとキックが削られて死ぬ。「実効値の何倍まで許すか」で持つ。
///
/// `limit` が None のトラックは何もしない。
pub fn tame_crest(x: &mut [f32], limit: Option<f32>, knee: f32) -> Option<f32> {
    let lim = limit?;
    let r = rms(x);
    if r <= 1e-9 {
        return None;
    }
    let ceiling = r * lim;
    let start = ceiling * knee;
    if peak(x) <= start {
        return None;
    }
    let before = peak(x);
    for v in x.iter_mut() {
        let a = v.abs();
        if a > start {
            // start から上だけを、天井へ向けて滑らかに寄せる
            let over = (a - start) / (ceiling - start).max(1e-9);
            let shaped = start + (ceiling - start) * over.tanh();
            *v = v.signum() * shaped;
        }
    }
    Some(tonescript_dsp::to_db(peak(x) / before))
}

/// 直流ずれを落とす。18Hz より下を切る。
pub fn remove_dc(x: &mut Vec<f32>) -> f32 {
    let before = x.iter().map(|v| *v as f64).sum::<f64>() / x.len().max(1) as f64;
    *x = highpass(x, 18.0);
    before as f32
}

// ---------------------------------------------------------------- 音圧

/// K 特性のフィルタを掛けたあとの実効値から求めた音圧（LUFS）。
///
/// EBU R128 の簡易版。段付きハイシェルフと 38Hz ハイパスを掛けてから
/// 実効値を取る。ゲート（無音区間を除く処理）は入れていない。
pub fn lufs(l: &[f32], r: &[f32]) -> f32 {
    let k = |x: &[f32]| -> Vec<f32> {
        // 1段目: 高域を持ち上げる棚（頭の当たりを人の耳に合わせる）
        let mut y = Vec::with_capacity(x.len());
        let (b0, b1, b2) = (1.53512485958697, -2.69169618940638, 1.19839281085285);
        let (a1, a2) = (-1.69065929318241, 0.73248077421585);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0, 0.0, 0.0);
        for &v in x {
            let x0 = v as f64;
            let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            y.push(y0 as f32);
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
        }
        // 2段目: 低域を落とす（38Hz 以下は音圧として数えない）
        let (b0, b1, b2) = (1.0, -2.0, 1.0);
        let (a1, a2) = (-1.99004745483398, 0.99007225036621);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0, 0.0, 0.0);
        let mut z = Vec::with_capacity(y.len());
        for &v in &y {
            let x0 = v as f64;
            let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            z.push(y0 as f32);
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
        }
        z
    };
    let (kl, kr) = (k(l), k(r));
    let ml = (kl.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / kl.len().max(1) as f64).max(1e-20);
    let mr = (kr.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / kr.len().max(1) as f64).max(1e-20);
    (-0.691 + 10.0 * (ml + mr).log10()) as f32
}

/// 目標の音圧へ合わせる。返り値は (掛けた倍率, 合わせる前の音圧)。
///
/// ピークではなく音圧で合わせる。ピーク基準にすると「サビに1本重ねたら
/// 全体の音量が下がる」「ドラムを上げたら他が下がる」が起きる。
pub fn normalize_lufs(s: &mut Stereo, target: f32) -> (f32, f32) {
    let before = lufs(&s.l, &s.r);
    let gain = tonescript_dsp::from_db(target - before);
    s.scale(gain);
    (gain, before)
}

/// 天井を超えたところだけを丸める。返り値は削った量（dB）。
pub fn limit(s: &mut Stereo, ceiling: f32) -> f32 {
    let before = s.peak();
    if before <= ceiling {
        return 0.0;
    }
    let knee = ceiling * 0.7;
    let span = ceiling * 0.3;
    for v in s.l.iter_mut().chain(s.r.iter_mut()) {
        let a = v.abs();
        if a > knee {
            let over = (a - knee) / span;
            *v = v.signum() * (knee + span * over.tanh());
        }
    }
    tonescript_dsp::to_db(s.peak() / before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, f: f32, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (std::f32::consts::TAU * f * i as f32 / SR).sin())
            .collect()
    }

    #[test]
    fn pan_is_equal_power() {
        let (l, r) = pan_gains(0.0);
        assert!((l - r).abs() < 1e-6, "中央で左右が違う");
        assert!((l * l + r * r - 1.0).abs() < 1e-5, "中央で出力が合わない");
        let (l, r) = pan_gains(-1.0);
        assert!(l > 0.99 && r < 0.01, "左に振り切れていない");
        let (l, r) = pan_gains(1.0);
        assert!(r > 0.99 && l < 0.01, "右に振り切れていない");
        // どこへ振っても全体の大きさは変わらない
        for p in [-1.0f32, -0.5, 0.0, 0.3, 1.0] {
            let (l, r) = pan_gains(p);
            assert!((l * l + r * r - 1.0).abs() < 1e-5, "p={p}");
        }
    }

    #[test]
    fn curve_becomes_a_sample_ramp() {
        use tonescript_song::model::Curve;
        // 0.1 秒ごとの目盛りが 4 つ
        let times: Vec<f64> = (0..5).map(|i| i as f64 * 0.1).collect();
        let total = (0.4 * SR) as usize;
        let c = Curve::new(vec![(0, 1.0), (4, 0.0)]);
        let g = curve_to_samples(&c, &times, total, SR).unwrap();
        assert_eq!(g.len(), total);
        assert!((g[0] - 1.0).abs() < 1e-3, "頭が 1 でない: {}", g[0]);
        let mid = total / 2;
        assert!((g[mid] - 0.5).abs() < 0.02, "真ん中が 0.5 でない: {}", g[mid]);
        // 段差が無いこと（1サンプルごとの変化が小さい）
        let jump = g.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(jump < 1e-3, "段差がある: {jump}");
        assert!(Curve::default().points.is_empty());
        assert!(curve_to_samples(&Curve::default(), &times, total, SR).is_none());
    }

    #[test]
    fn widen_keeps_level_and_center_stays_center() {
        let m = tone(4800, 440.0, 0.5);
        let c = widen(&m, 0.0);
        assert_eq!(c.l, c.r, "width 0 なら左右同じ");
        let w = widen(&m, 1.5);
        assert!(w.l != w.r, "広がっていない");
        // 全体の大きさは保たれる
        let a = (rms(&c.l).powi(2) + rms(&c.r).powi(2)).sqrt();
        let b = (rms(&w.l).powi(2) + rms(&w.r).powi(2)).sqrt();
        assert!((a - b).abs() < 0.02, "広げたら音量が変わった {a} -> {b}");
    }

    #[test]
    fn sidechain_dips_at_the_kick() {
        let n = 48_000;
        let env = sidechain_env(&[0, 24_000], n, 0.7, 0.003, 0.02, 0.2);
        assert!((env[0] - 1.0).abs() < 0.01, "頭は凹んでいない");
        let dip = (0.003 * SR) as usize + 10;
        assert!(env[dip] < 0.35, "凹んでいない: {}", env[dip]);
        // 戻りきったところは 1 に近い
        let back = (0.003 + 0.02 + 0.2) * SR;
        assert!(env[back as usize + 100] > 0.95, "戻っていない");
        // 2発目でも凹む
        assert!(env[24_000 + dip] < 0.35);
    }

    #[test]
    fn sidechain_overlap_takes_the_deeper_dip() {
        // 凹みが重なったとき、足し算で浅くならないこと
        let env = sidechain_env(&[0, 200], 48_000, 0.7, 0.003, 0.02, 0.2);
        assert!(env.iter().all(|v| *v >= 1.0 - 0.7 - 1e-6), "深すぎる");
        assert!(env.iter().all(|v| *v <= 1.0 + 1e-6));
    }

    #[test]
    fn crest_leaves_drums_alone_when_unlimited() {
        let mut x = tone(4800, 100.0, 0.3);
        x[100] = 5.0; // 鋭い頭
        let copy = x.clone();
        let cut = tame_crest(&mut x, None, 0.72);
        assert_eq!(cut, None);
        assert_eq!(x, copy, "None なら触らない");
    }

    #[test]
    fn crest_tames_a_spike_but_keeps_the_body() {
        let mut x = tone(48_000, 100.0, 0.3);
        let body_before = rms(&x);
        x[100] = 6.0;
        let cut = tame_crest(&mut x, Some(6.0), 0.72).expect("削るはず");
        assert!(cut < 0.0, "削っていない: {cut}");
        assert!(peak(&x) < 6.0, "頭が残っている");
        // 本体はほとんど変わらない
        assert!((rms(&x) - body_before).abs() / body_before < 0.05);
    }

    #[test]
    fn lufs_is_lower_for_quieter_signal() {
        let loud = tone(48_000, 1000.0, 0.5);
        let soft = tone(48_000, 1000.0, 0.05);
        let a = lufs(&loud, &loud);
        let b = lufs(&soft, &soft);
        assert!(a > b, "大きいほうが上のはず {a} {b}");
        // 1/10 なら約 20dB 下
        assert!((a - b - 20.0).abs() < 1.0, "差が {}dB", a - b);
    }

    #[test]
    fn normalize_hits_the_target() {
        let t = tone(48_000, 1000.0, 0.2);
        let mut s = Stereo { l: t.clone(), r: t };
        let (_g, before) = normalize_lufs(&mut s, -14.0);
        let after = lufs(&s.l, &s.r);
        assert!((after + 14.0).abs() < 0.1, "{before} -> {after}");
    }

    #[test]
    fn limiter_only_touches_the_top() {
        let t = tone(48_000, 200.0, 0.3);
        let mut s = Stereo { l: t.clone(), r: t.clone() };
        assert_eq!(limit(&mut s, 0.99), 0.0, "天井より下なら何もしない");
        assert_eq!(s.l, t);

        let loud: Vec<f32> = t.iter().map(|v| v * 5.0).collect();
        let mut s = Stereo { l: loud.clone(), r: loud };
        let cut = limit(&mut s, 0.99);
        assert!(cut < 0.0);
        assert!(s.peak() <= 1.0, "天井を超えている: {}", s.peak());
    }

    #[test]
    fn dc_is_removed() {
        let mut x: Vec<f32> = tone(48_000, 200.0, 0.3).iter().map(|v| v + 0.2).collect();
        let before = remove_dc(&mut x);
        assert!((before - 0.2).abs() < 0.01, "直流を測れていない: {before}");
        let after = x[4000..].iter().map(|v| *v as f64).sum::<f64>() / (x.len() - 4000) as f64;
        assert!(after.abs() < 0.01, "直流が残っている: {after}");
    }
}
