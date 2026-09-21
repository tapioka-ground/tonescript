//! トラックごとの押さえ込み（コンプレッサー）。
//!
//! 何をするものか
//! --------------
//! **大きいところだけを小さくする。** 結果として大小の差が縮むので、
//! 全体を持ち上げても割れなくなり、埋もれずに前へ出てくる。
//!
//! 歌に一番効く。人は一定の音量で歌えないので、生のままだと大きい所が
//! 割れて小さい所が聞こえない。押さえ込んでから持ち上げると、どちらも
//! 聞こえるようになる。
//!
//! 仕組み
//! ------
//! ```text
//!   入力 → 大きさを測る → 越えたぶんを比で削る → 削り方をなます → 掛ける
//! ```
//!
//! **なますのが肝。** 瞬時に削ると波形そのものが歪む。「どれだけ速く
//! 効き始めるか（アタック）」と「どれだけ速く戻るか（リリース）」で、
//! 同じ設定でも全く違う音になる。
//!
//! - アタックが遅い → 打撃の頭が通り抜けて、パンチが残る
//! - アタックが速い → 頭から潰れて、平らになる
//! - リリースが速い → 隙間で持ち上がり、呼吸するように聞こえる
//! - リリースが遅い → 一度下がったら戻らず、落ち着く
//!
//! 削る量は**dB の世界でなます**。倍率のままなますと、小さい音のときに
//! 戻りが極端に遅くなる。

use crate::osc::SR;

/// どう押さえるか。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompCfg {
    /// ここを越えたぶんを削る（dB）。-60〜0
    pub threshold: f32,
    /// 比。4 なら「4dB 越えたら 1dB だけ出す」。1 で何もしない
    pub ratio: f32,
    /// 効き始めるまで（ミリ秒）
    pub attack: f32,
    /// 戻るまで（ミリ秒）
    pub release: f32,
    /// 曲がり角の丸さ（dB）。0 で角ばる
    pub knee: f32,
    /// 押さえたぶんを持ち上げる（dB）
    pub makeup: f32,
}

impl Default for CompCfg {
    fn default() -> Self {
        Self {
            threshold: -18.0,
            ratio: 1.0, // 既定では何もしない
            attack: 10.0,
            release: 120.0,
            knee: 6.0,
            makeup: 0.0,
        }
    }
}

impl CompCfg {
    /// 何もしない設定か。**そうなら通さずに済ませる。**
    pub fn is_off(&self) -> bool {
        self.ratio <= 1.001 && self.makeup.abs() < 0.01
    }

    pub fn check(&self) -> Result<(), String> {
        if !(-60.0..=0.0).contains(&self.threshold) {
            return Err(format!("comp.threshold が {}dB です。-60〜0 の間に", self.threshold));
        }
        if !(1.0..=20.0).contains(&self.ratio) {
            return Err(format!("comp.ratio が {} です。1〜20 の間に", self.ratio));
        }
        if !(0.1..=200.0).contains(&self.attack) {
            return Err(format!("comp.attack が {}ms です。0.1〜200 の間に", self.attack));
        }
        if !(1.0..=2000.0).contains(&self.release) {
            return Err(format!("comp.release が {}ms です。1〜2000 の間に", self.release));
        }
        if !(0.0..=24.0).contains(&self.knee) {
            return Err(format!("comp.knee が {}dB です。0〜24 の間に", self.knee));
        }
        if !(-12.0..=24.0).contains(&self.makeup) {
            return Err(format!("comp.makeup が {}dB です。-12〜24 の間に", self.makeup));
        }
        Ok(())
    }
}

/// 1トラックぶん。**状態を持つので使い回す。**
#[derive(Clone, Copy, Debug)]
pub struct Comp {
    cfg: CompCfg,
    /// 今どれだけ削っているか（dB、正の数）
    reduction: f32,
    a_coef: f32,
    r_coef: f32,
}

impl Default for Comp {
    fn default() -> Self {
        Self::new(CompCfg::default())
    }
}

/// ミリ秒から「1サンプルでどれだけ近づくか」へ。
fn coef(ms: f32) -> f32 {
    let t = (ms.max(0.01) / 1000.0) * SR;
    // 時定数ぶんで 1/e まで近づく
    (-1.0 / t).exp()
}

impl Comp {
    pub fn new(cfg: CompCfg) -> Self {
        Self { cfg, reduction: 0.0, a_coef: coef(cfg.attack), r_coef: coef(cfg.release) }
    }

    /// 設定を差し替える。**同じなら何もしない**（削り具合を保つため）。
    pub fn set(&mut self, cfg: CompCfg) {
        if self.cfg == cfg {
            return;
        }
        self.cfg = cfg;
        self.a_coef = coef(cfg.attack);
        self.r_coef = coef(cfg.release);
    }

    /// 今どれだけ削っているか（dB）。画面の針に出す。
    pub fn reduction_db(&self) -> f32 {
        self.reduction
    }

    pub fn clear(&mut self) {
        self.reduction = 0.0;
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let c = &self.cfg;
        // 大きさを dB で見る。0 のときに -inf にならないよう下を止める
        let level = 20.0 * x.abs().max(1e-9).log10();
        let over = level - c.threshold;

        // 越えたぶんをどれだけ削るか。角を丸めるぶんは2次で繋ぐ
        let want = if c.knee > 0.0 && over > -c.knee * 0.5 && over < c.knee * 0.5 {
            let t = over + c.knee * 0.5;
            (1.0 - 1.0 / c.ratio) * t * t / (2.0 * c.knee)
        } else if over > 0.0 {
            over * (1.0 - 1.0 / c.ratio)
        } else {
            0.0
        };

        // なます。**増やすときはアタック、減らすときはリリース**
        let k = if want > self.reduction { self.a_coef } else { self.r_coef };
        self.reduction = want + (self.reduction - want) * k;

        x * 10f32.powf((c.makeup - self.reduction) / 20.0)
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

    fn tone(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * std::f32::consts::TAU * hz / SR).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| (v * v) as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
    }

    fn db(x: f32) -> f32 {
        20.0 * x.max(1e-12).log10()
    }

    /// 落ち着いてからの出力（後ろ半分）。
    fn settled(cfg: CompCfg, amp: f32) -> f32 {
        let n = 48_000;
        let mut x = tone(200.0, amp, n);
        Comp::new(cfg).process(&mut x);
        rms(&x[n / 2..])
    }

    #[test]
    fn off_by_default() {
        assert!(CompCfg::default().is_off(), "既定で効いてしまっている");
        let mut x = tone(200.0, 0.8, 4800);
        let before = x.clone();
        Comp::new(CompCfg::default()).process(&mut x);
        // 比 1 なら素通し
        let diff = x.iter().zip(&before).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(diff < 1e-6, "比 1 なのに変わった: {diff}");
    }

    #[test]
    fn quiet_signals_pass_untouched() {
        let cfg = CompCfg { threshold: -12.0, ratio: 4.0, knee: 0.0, ..Default::default() };
        // -30dB くらい。閾値のずっと下
        let a = settled(cfg, 0.03);
        let want = rms(&tone(200.0, 0.03, 48_000)[24_000..]);
        assert!((db(a) - db(want)).abs() < 0.2, "小さい音まで触った: {:+.2}dB", db(a) - db(want));
    }

    #[test]
    fn the_ratio_is_what_it_says() {
        // 閾値を 12dB 越えた信号を 4:1 で押さえたら、出るのは 3dB ぶん
        let cfg =
            CompCfg { threshold: -24.0, ratio: 4.0, knee: 0.0, attack: 1.0, ..Default::default() };
        let amp = 10f32.powf(-12.0 / 20.0); // -12dBFS。閾値の 12dB 上
        let out = db(settled(cfg, amp));
        // 正弦波の実効値は振幅の 1/√2 = -3dB
        let want = -24.0 - 3.0 + 12.0 / 4.0;
        assert!((out - want).abs() < 1.0, "出力 {out:+.2}dB（{want:+.2} のはず）");
    }

    #[test]
    fn a_bigger_ratio_squashes_more() {
        let mk = |ratio: f32| CompCfg {
            threshold: -24.0,
            ratio,
            knee: 0.0,
            attack: 1.0,
            ..Default::default()
        };
        let amp = 10f32.powf(-6.0 / 20.0);
        let gentle = db(settled(mk(2.0), amp));
        let hard = db(settled(mk(10.0), amp));
        assert!(hard < gentle - 3.0, "比を上げても潰れない（{gentle:+.2} / {hard:+.2}）");
    }

    #[test]
    fn makeup_puts_the_level_back() {
        let base = CompCfg { threshold: -24.0, ratio: 4.0, knee: 0.0, ..Default::default() };
        let lifted = CompCfg { makeup: 6.0, ..base };
        let amp = 10f32.powf(-6.0 / 20.0);
        let d = db(settled(lifted, amp)) - db(settled(base, amp));
        assert!((d - 6.0).abs() < 0.2, "持ち上げが {d:+.2}dB（+6 のはず）");
    }

    #[test]
    fn a_slow_attack_lets_the_hit_through() {
        // 打撃の頭が通り抜けること。これがパンチの正体
        let n = 24_000;
        let x = tone(200.0, 0.9, n);
        let mut fast = x.clone();
        let mut slow = x.clone();
        let base =
            CompCfg { threshold: -30.0, ratio: 8.0, knee: 0.0, release: 200.0, ..Default::default() };
        Comp::new(CompCfg { attack: 0.5, ..base }).process(&mut fast);
        Comp::new(CompCfg { attack: 80.0, ..base }).process(&mut slow);
        // 頭の 2ms
        let head = (0.002 * SR) as usize;
        assert!(
            rms(&slow[..head]) > rms(&fast[..head]) * 1.5,
            "遅いアタックで頭が残っていない（速 {:.4} / 遅 {:.4}）",
            rms(&fast[..head]),
            rms(&slow[..head])
        );
        // 落ち着いたあとも、遅いほうが大きい。
        //
        // 大きさは山の高さで見ているので、正弦波では1周期に2度 0 を通る。
        // アタックが遅いと山に追いつけず、平均の削り量が減る。
        // 本物のコンプレッサーも同じ振る舞いをする
        let a = db(rms(&fast[n / 2..]));
        let b = db(rms(&slow[n / 2..]));
        assert!(b > a, "遅いアタックのほうが小さくなった（速 {a:+.2} / 遅 {b:+.2}）");
        // どちらも素通しよりは小さいこと
        let raw = db(rms(&x[n / 2..]));
        assert!(a < raw && b < raw, "押さえていない（素 {raw:+.2}）");
    }

    #[test]
    fn it_lets_go_after_the_loud_part() {
        // 大きい音のあとに小さい音。戻っていないと小さい音が聞こえない
        let mut cfg =
            CompCfg { threshold: -24.0, ratio: 8.0, knee: 0.0, attack: 1.0, ..Default::default() };
        cfg.release = 30.0;
        let mut x = tone(200.0, 0.9, 12_000);
        x.extend(tone(200.0, 0.05, 36_000));
        let mut y = x.clone();
        Comp::new(cfg).process(&mut y);
        // 小さい音の後半は、ほぼ素通しに戻っていること
        let tail = 36_000;
        let got = db(rms(&y[tail..]));
        let want = db(rms(&x[tail..]));
        assert!((got - want).abs() < 1.0, "戻っていない（{got:+.2} / 素 {want:+.2}）");
    }

    #[test]
    fn the_knee_softens_the_corner() {
        // 閾値のすぐ上で、角ばった設定より丸い設定のほうが多く削れている
        let hard = CompCfg { threshold: -20.0, ratio: 8.0, knee: 0.0, ..Default::default() };
        let soft = CompCfg { knee: 18.0, ..hard };
        // 閾値のすぐ下
        let amp = 10f32.powf(-24.0 / 20.0);
        let a = db(settled(hard, amp));
        let b = db(settled(soft, amp));
        assert!(b < a - 0.3, "丸めても変わらない（角 {a:+.2} / 丸 {b:+.2}）");
    }

    #[test]
    fn the_meter_shows_how_much_it_is_squashing() {
        let cfg =
            CompCfg { threshold: -30.0, ratio: 4.0, knee: 0.0, attack: 1.0, ..Default::default() };
        let mut c = Comp::new(cfg);
        assert_eq!(c.reduction_db(), 0.0, "鳴らす前から削っている");
        let mut x = tone(200.0, 0.9, 24_000);
        c.process(&mut x);
        assert!(c.reduction_db() > 3.0, "削っているのに針が振れない: {}", c.reduction_db());
    }

    #[test]
    fn blocks_join_up() {
        let cfg = CompCfg { threshold: -24.0, ratio: 4.0, ..Default::default() };
        let x = tone(200.0, 0.8, 9600);
        let mut whole = x.clone();
        Comp::new(cfg).process(&mut whole);
        let mut c = Comp::new(cfg);
        let mut piece = x.clone();
        for chunk in piece.chunks_mut(256) {
            c.process(chunk);
        }
        let diff = whole.iter().zip(&piece).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(diff < 1e-6, "ブロックに切ると変わる: {diff}");
    }

    #[test]
    fn it_never_blows_up() {
        for ratio in [1.0f32, 4.0, 20.0] {
            for th in [-60.0f32, -24.0, 0.0] {
                let cfg = CompCfg {
                    threshold: th,
                    ratio,
                    attack: 0.1,
                    release: 1.0,
                    makeup: 24.0,
                    knee: 24.0,
                };
                let mut x = tone(200.0, 1.0, 4800);
                x[0] = 0.0; // 無音から始めても壊れないこと
                Comp::new(cfg).process(&mut x);
                assert!(x.iter().all(|v| v.is_finite()), "{ratio}/{th} で壊れた");
            }
        }
    }

    #[test]
    fn bad_numbers_are_refused() {
        assert!(CompCfg::default().check().is_ok());
        let cases = [
            CompCfg { ratio: 50.0, ..Default::default() },
            CompCfg { threshold: 10.0, ..Default::default() },
            CompCfg { attack: 0.0, ..Default::default() },
            CompCfg { release: 9000.0, ..Default::default() },
            CompCfg { makeup: 50.0, ..Default::default() },
        ];
        for c in cases {
            assert!(c.check().is_err(), "{c:?} が通った");
        }
    }
}
