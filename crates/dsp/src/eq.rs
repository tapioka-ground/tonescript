//! トラックごとの音の整え。低・中・高の3つ。
//!
//! なぜ要るか
//! ----------
//! ミックスは音量だけでは作れない。ベースとキックは同じ低い所で鳴るので、
//! 音量をどう動かしても互いを消し合う。**片方の低い所を削って、もう片方に
//! 場所を空ける**のがやり方で、そのための道具がこれ。
//!
//! 3つで足りるのか
//! ---------------
//! 足りる。本職の道具は6バンドも8バンドも積めるが、実際に使うのは
//! 「低いところを削る」「中を持ち上げる／へこませる」「上を足す」の3つが
//! ほとんど。数を増やすほど画面も設定も重くなるので、ここは3つに絞った。
//!
//! 作り
//! ----
//! ```text
//!   入力 → 低の棚 → 中の山 → 高の棚 → 出力
//! ```
//!
//! 低と高は**棚**（そこから先を全部上げ下げ）、中は**山**（その辺だけ）。
//! 1サンプルずつ通せる形にしてあるので、鳴らしながらでも書き出しでも
//! **同じものが通る**。

use crate::osc::SR;

/// どう整えるか。持ち上げ／削りは dB。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqCfg {
    /// 低いところ（`low_hz` から下）を何 dB
    pub low: f32,
    /// 中（`mid_hz` のあたり）を何 dB
    pub mid: f32,
    /// 高いところ（`high_hz` から上）を何 dB
    pub high: f32,
    /// 低の棚の境い目
    pub low_hz: f32,
    /// 中の山の中心
    pub mid_hz: f32,
    /// 中の山の幅。大きいほど細い
    pub mid_q: f32,
    /// 高の棚の境い目
    pub high_hz: f32,
}

impl Default for EqCfg {
    fn default() -> Self {
        Self {
            low: 0.0,
            mid: 0.0,
            high: 0.0,
            low_hz: 200.0,
            mid_hz: 1000.0,
            mid_q: 0.9,
            high_hz: 4000.0,
        }
    }
}

impl EqCfg {
    /// 何も動かさない設定か。**そうなら通さずに済ませる。**
    pub fn is_flat(&self) -> bool {
        self.low.abs() < 0.01 && self.mid.abs() < 0.01 && self.high.abs() < 0.01
    }

    /// 書ける値か確かめる。
    pub fn check(&self) -> Result<(), String> {
        for (name, v) in [("low", self.low), ("mid", self.mid), ("high", self.high)] {
            if !(-24.0..=24.0).contains(&v) {
                return Err(format!("eq.{name} が {v}dB です。-24〜24 の間に"));
            }
        }
        for (name, v) in
            [("low_hz", self.low_hz), ("mid_hz", self.mid_hz), ("high_hz", self.high_hz)]
        {
            if !(20.0..=18_000.0).contains(&v) {
                return Err(format!("eq.{name} が {v}Hz です。20〜18000 の間に"));
            }
        }
        if !(0.2..=12.0).contains(&self.mid_q) {
            return Err(format!("eq.mid_q が {} です。0.2〜12 の間に", self.mid_q));
        }
        Ok(())
    }
}

/// ビキャッド1段。**状態を持つ**ので、ブロックをまたいでも繋がる。
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    /// 何もしない段か。そうならそのまま返す
    bypass: bool,
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self { b, a, bypass: false, ..Default::default() }
    }

    fn pass() -> Self {
        Self { bypass: true, ..Default::default() }
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        if self.bypass {
            return x;
        }
        let x0 = x as f64;
        let y0 = self.b[0] * x0 + self.b[1] * self.x1 + self.b[2] * self.x2
            - self.a[0] * self.y1
            - self.a[1] * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        y0 as f32
    }

    pub fn clear(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// RBJ の低い棚。`gain_db` だけ、`f0` から下を上下させる。
fn low_shelf(f0: f32, gain_db: f32) -> Biquad {
    if gain_db.abs() < 0.01 {
        return Biquad::pass();
    }
    let a = 10f64.powf(gain_db as f64 / 40.0);
    let w = std::f64::consts::TAU * (f0.clamp(20.0, SR * 0.45) as f64) / SR as f64;
    let (cw, sw) = (w.cos(), w.sin());
    // 棚の傾き。1.0 で最大（うねらない範囲）
    let al = sw / 2.0 * ((a + 1.0 / a) * (1.0 / 0.9 - 1.0) + 2.0).sqrt();
    let ap = a + 1.0;
    let am = a - 1.0;
    let tsa = 2.0 * a.sqrt() * al;
    let a0 = ap + am * cw + tsa;
    Biquad::new(
        [
            a * (ap - am * cw + tsa) / a0,
            2.0 * a * (am - ap * cw) / a0,
            a * (ap - am * cw - tsa) / a0,
        ],
        [-2.0 * (am + ap * cw) / a0, (ap + am * cw - tsa) / a0],
    )
}

/// RBJ の高い棚。
fn high_shelf(f0: f32, gain_db: f32) -> Biquad {
    if gain_db.abs() < 0.01 {
        return Biquad::pass();
    }
    let a = 10f64.powf(gain_db as f64 / 40.0);
    let w = std::f64::consts::TAU * (f0.clamp(20.0, SR * 0.45) as f64) / SR as f64;
    let (cw, sw) = (w.cos(), w.sin());
    let al = sw / 2.0 * ((a + 1.0 / a) * (1.0 / 0.9 - 1.0) + 2.0).sqrt();
    let ap = a + 1.0;
    let am = a - 1.0;
    let tsa = 2.0 * a.sqrt() * al;
    let a0 = ap - am * cw + tsa;
    Biquad::new(
        [
            a * (ap + am * cw + tsa) / a0,
            -2.0 * a * (am + ap * cw) / a0,
            a * (ap + am * cw - tsa) / a0,
        ],
        [2.0 * (am - ap * cw) / a0, (ap - am * cw - tsa) / a0],
    )
}

/// RBJ の山。`f0` のあたりだけを上下させる。
fn peaking(f0: f32, q: f32, gain_db: f32) -> Biquad {
    if gain_db.abs() < 0.01 {
        return Biquad::pass();
    }
    let a = 10f64.powf(gain_db as f64 / 40.0);
    let w = std::f64::consts::TAU * (f0.clamp(20.0, SR * 0.45) as f64) / SR as f64;
    let al = w.sin() / (2.0 * q.clamp(0.2, 12.0) as f64);
    let a0 = 1.0 + al / a;
    Biquad::new(
        [(1.0 + al * a) / a0, -2.0 * w.cos() / a0, (1.0 - al * a) / a0],
        [-2.0 * w.cos() / a0, (1.0 - al / a) / a0],
    )
}

/// 3つをまとめたもの。**1つのパートにつき1つ持つ。**
#[derive(Clone, Copy, Debug, Default)]
pub struct Eq {
    low: Biquad,
    mid: Biquad,
    high: Biquad,
    cfg: Option<EqCfg>,
}

impl Eq {
    pub fn new(cfg: EqCfg) -> Self {
        Self {
            low: low_shelf(cfg.low_hz, cfg.low),
            mid: peaking(cfg.mid_hz, cfg.mid_q, cfg.mid),
            high: high_shelf(cfg.high_hz, cfg.high),
            cfg: Some(cfg),
        }
    }

    /// 設定を差し替える。**同じ設定なら何もしない**（状態を保つため）。
    ///
    /// 鳴らしている最中に毎ブロック作り直すと、そのたびに中の値が消えて
    /// プツプツ鳴る。
    pub fn set(&mut self, cfg: EqCfg) {
        if self.cfg == Some(cfg) {
            return;
        }
        let keep = *self;
        *self = Eq::new(cfg);
        // 中の値は引き継ぐ。切り替えの段差を小さくする
        self.low.x1 = keep.low.x1;
        self.low.x2 = keep.low.x2;
        self.low.y1 = keep.low.y1;
        self.low.y2 = keep.low.y2;
        self.mid.x1 = keep.mid.x1;
        self.mid.x2 = keep.mid.x2;
        self.mid.y1 = keep.mid.y1;
        self.mid.y2 = keep.mid.y2;
        self.high.x1 = keep.high.x1;
        self.high.x2 = keep.high.x2;
        self.high.y1 = keep.high.y1;
        self.high.y2 = keep.high.y2;
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        self.high.run(self.mid.run(self.low.run(x)))
    }

    pub fn clear(&mut self) {
        self.low.clear();
        self.mid.clear();
        self.high.clear();
    }

    /// まとめて通す。書き出し側から使う。
    pub fn process(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            *v = self.run(*v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * std::f32::consts::TAU * hz / SR).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| (v * v) as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
    }

    /// その高さの音を通したとき、何 dB 変わるか。
    ///
    /// 頭の立ち上がりを避けるため、後ろ半分で測る。
    fn gain_at(cfg: EqCfg, hz: f32) -> f32 {
        let n = 24_000;
        let x = tone(hz, n);
        let mut y = x.clone();
        Eq::new(cfg).process(&mut y);
        20.0 * (rms(&y[n / 2..]) / rms(&x[n / 2..]).max(1e-9)).log10()
    }

    #[test]
    fn flat_is_left_alone() {
        let cfg = EqCfg::default();
        assert!(cfg.is_flat());
        let x = tone(440.0, 4800);
        let mut y = x.clone();
        Eq::new(cfg).process(&mut y);
        assert_eq!(y, x, "何も動かしていないのに通した");
    }

    #[test]
    fn the_low_shelf_moves_the_bottom_only() {
        let cfg = EqCfg { low: 6.0, ..Default::default() };
        // 棚の下（80Hz）は上がる
        let low = gain_at(cfg, 80.0);
        assert!((low - 6.0).abs() < 1.5, "80Hz が {low:+.2}dB（+6 のはず）");
        // ずっと上（8kHz）は動かない
        let high = gain_at(cfg, 8000.0);
        assert!(high.abs() < 0.6, "8kHz が {high:+.2}dB（動かないはず）");
    }

    #[test]
    fn the_high_shelf_moves_the_top_only() {
        let cfg = EqCfg { high: 6.0, ..Default::default() };
        let high = gain_at(cfg, 10_000.0);
        assert!((high - 6.0).abs() < 1.5, "10kHz が {high:+.2}dB");
        let low = gain_at(cfg, 80.0);
        assert!(low.abs() < 0.6, "80Hz が {low:+.2}dB（動かないはず）");
    }

    #[test]
    fn the_mid_bump_is_where_it_was_put() {
        let cfg = EqCfg { mid: 8.0, mid_hz: 1000.0, mid_q: 1.2, ..Default::default() };
        let at = gain_at(cfg, 1000.0);
        assert!((at - 8.0).abs() < 1.0, "1kHz が {at:+.2}dB（+8 のはず）");
        // 離れれば効かない
        for hz in [100.0f32, 8000.0] {
            let g = gain_at(cfg, hz);
            assert!(g.abs() < 1.5, "{hz}Hz が {g:+.2}dB（離れているのに動いた）");
        }
    }

    #[test]
    fn cutting_works_as_well_as_boosting() {
        // 削れないと意味がない。ミックスは削って場所を空けるもの
        let cfg = EqCfg { low: -12.0, ..Default::default() };
        let g = gain_at(cfg, 60.0);
        assert!((g + 12.0).abs() < 2.0, "60Hz が {g:+.2}dB（-12 のはず）");
    }

    #[test]
    fn a_narrow_mid_stays_narrow() {
        let wide = EqCfg { mid: 9.0, mid_q: 0.5, ..Default::default() };
        let narrow = EqCfg { mid: 9.0, mid_q: 8.0, ..Default::default() };
        // 中心では同じだけ上がる
        assert!((gain_at(wide, 1000.0) - gain_at(narrow, 1000.0)).abs() < 1.5);
        // 少し離れると、細いほうは効いていない
        let (w, n) = (gain_at(wide, 400.0), gain_at(narrow, 400.0));
        assert!(n < w - 2.0, "幅が効いていない: 広 {w:+.2} / 細 {n:+.2}");
    }

    #[test]
    fn it_never_blows_up() {
        // どの組み合わせでも数が壊れないこと
        for low in [-24.0f32, 0.0, 24.0] {
            for mid in [-24.0f32, 24.0] {
                for high in [-24.0f32, 24.0] {
                    let cfg = EqCfg { low, mid, high, mid_q: 12.0, ..Default::default() };
                    let mut y = tone(440.0, 4800);
                    Eq::new(cfg).process(&mut y);
                    assert!(y.iter().all(|v| v.is_finite()), "{low}/{mid}/{high} で壊れた");
                    assert!(rms(&y) < 100.0, "{low}/{mid}/{high} で暴れた");
                }
            }
        }
    }

    #[test]
    fn blocks_join_up_without_a_click() {
        // ブロックをまたいでも繋がること。切れていると境目でプツッと鳴る
        let cfg = EqCfg { low: 8.0, high: -6.0, ..Default::default() };
        let x = tone(220.0, 4800);
        let mut whole = x.clone();
        Eq::new(cfg).process(&mut whole);

        let mut eq = Eq::new(cfg);
        let mut piecewise = x.clone();
        for chunk in piecewise.chunks_mut(256) {
            eq.process(chunk);
        }
        let diff = whole.iter().zip(&piecewise).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(diff < 1e-6, "ブロックに切ると結果が変わる: {diff}");
    }

    #[test]
    fn the_same_setting_does_not_reset_the_state() {
        let cfg = EqCfg { low: 6.0, ..Default::default() };
        let mut eq = Eq::new(cfg);
        for v in tone(220.0, 1000) {
            eq.run(v);
        }
        let before = eq.low.y1;
        eq.set(cfg);
        assert_eq!(eq.low.y1, before, "同じ設定で中の値が消えた");
    }

    #[test]
    fn a_default_eq_told_to_set_behaves_like_a_new_one() {
        // 鳴らす側は Eq::default() を持っておいて、あとから set する。
        // その道でも Eq::new と同じ結果になること
        let cfg = EqCfg { high: -24.0, ..Default::default() };
        let x = tone(8000.0, 24_000);

        let mut a = Eq::new(cfg);
        let mut ya = x.clone();
        a.process(&mut ya);

        let mut b = Eq::default();
        b.set(cfg);
        let mut yb = x.clone();
        b.process(&mut yb);

        let ra = rms(&ya[12_000..]);
        let rb = rms(&yb[12_000..]);
        assert!((ra - rb).abs() < 1e-5, "new {ra:.5} と set {rb:.5} が違う");
        assert!(rb < rms(&x[12_000..]) * 0.2, "set 経由だと削れていない");
    }

    #[test]
    fn bad_numbers_are_refused() {
        assert!(EqCfg::default().check().is_ok());
        let cases = [
            EqCfg { low: 40.0, ..Default::default() },
            EqCfg { mid: -40.0, ..Default::default() },
            EqCfg { mid_hz: 30_000.0, ..Default::default() },
            EqCfg { mid_q: 100.0, ..Default::default() },
        ];
        for c in cases {
            assert!(c.check().is_err(), "{c:?} が通った");
        }
    }
}
