//! 音を出す側が見る「今の設定」。
//!
//! 音側のスレッドは曲ファイルも `Song` も見ない。見ると、画面側が曲を
//! 差し替えた瞬間に足元が変わる。代わりに、変わらない写しをここで作って
//! 丸ごと渡す。差し替えは**写しごと**行う（`Arc` を挿し替えるだけ）。
//!
//! フェーダーを動かすたびにこれを作り直すが、中身は数十バイトの表なので
//! 画面の速さ（毎秒60回）でも問題にならない。

use std::collections::HashMap;
use std::sync::Arc;

use tonescript_dsp::osc::SR;
use tonescript_song::model::{Curve, Lane, MixCfg};
use tonescript_song::Song;

/// パート1つぶんの設定。
#[derive(Clone, Debug)]
pub struct PartPlan {
    pub name: String,
    /// `GAINS`
    pub gain: f32,
    /// `MIX`
    pub mix: MixCfg,
    /// 鳴らすか（ミュート・ソロの結果）
    pub audible: bool,
    /// 音量・左右・残響・ダッキングの線
    pub gain_curve: Option<Curve>,
    pub pan_curve: Option<Curve>,
    pub reverb_curve: Option<Curve>,
    pub duck_curve: Option<Curve>,
}

impl PartPlan {
    fn new(name: &str, song: &Song) -> Self {
        let lanes = song.automation.get(name);
        let lane = |l: Lane| lanes.and_then(|m| m.get(&l)).cloned().filter(|c| !c.is_empty());
        Self {
            name: name.to_string(),
            gain: song.gains.get(name).copied().unwrap_or(1.0),
            mix: song.mix.get(name).copied().unwrap_or_default(),
            audible: true,
            gain_curve: lane(Lane::Gain),
            pan_curve: lane(Lane::Pan),
            reverb_curve: lane(Lane::Reverb),
            duck_curve: lane(Lane::Duck),
        }
    }
}

/// 音側が見る写し。作ったあとは誰も書き換えない。
#[derive(Clone, Debug)]
pub struct Plan {
    pub parts: Vec<PartPlan>,
    /// パート名 -> `parts` の何番目か
    pub index: HashMap<String, usize>,
    /// 目盛り -> 曲の頭からの秒数
    pub step_times: Arc<Vec<f64>>,
    /// `[深さ, アタック, 保持, 戻り]`
    pub sidechain: (f32, f32, f32, f32),
    /// `[秒数, 広がり]`
    pub reverb: (f32, f32),
    pub master_gain: f32,
    /// 曲の長さ（サンプル）
    pub total: u64,
}

impl Plan {
    /// 曲から写しを作る。`parts` は譜面にあるパート全部。
    pub fn from_song(song: &Song, parts: &[String]) -> Self {
        let times = tonescript_render::arrange::step_times(song);
        let total = ((times.last().copied().unwrap_or(0.0) + 4.0) * SR as f64) as u64;
        let mut index = HashMap::new();
        let mut list = Vec::with_capacity(parts.len());
        for (i, p) in parts.iter().enumerate() {
            index.insert(p.clone(), i);
            list.push(PartPlan::new(p, song));
        }
        Self {
            parts: list,
            index,
            step_times: Arc::new(times),
            sidechain: song.sidechain,
            reverb: song.reverb,
            master_gain: song.master_gain,
            total,
        }
    }

    /// 空の写し。曲を読む前でも音側を動かしておけるように。
    pub fn empty() -> Self {
        Self {
            parts: Vec::new(),
            index: HashMap::new(),
            step_times: Arc::new(vec![0.0]),
            sidechain: (0.70, 0.003, 0.020, 0.200),
            reverb: (1.9, 4.2),
            master_gain: 1.0,
            total: 0,
        }
    }

    pub fn part_of(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    /// 目盛りを秒へ。範囲の外は端の値で止める。
    pub fn time_of(&self, step: u32) -> f64 {
        let t = &self.step_times;
        t[(step as usize).min(t.len() - 1)]
    }

    /// 目盛りをサンプル位置へ。
    pub fn sample_of(&self, step: u32) -> u64 {
        (self.time_of(step) * SR as f64) as u64
    }
}

/// サンプル位置から目盛りを引く。
///
/// テンポが動く曲では「1サンプル＝何目盛り」が一定でないので、表を順に
/// 歩いて引く。再生は前へ進むだけなので、前回の位置から続ければ 1回あたり
/// 数歩で済む。戻った（頭出しした）ときだけ探し直す。
#[derive(Default, Debug)]
pub struct StepCursor {
    i: usize,
}

impl StepCursor {
    pub fn reset(&mut self) {
        self.i = 0;
    }

    /// そのサンプル位置は何目盛り目か。小数で返す。
    pub fn step_at(&mut self, sample: u64, times: &[f64]) -> f32 {
        if times.len() < 2 {
            return 0.0;
        }
        let t = sample as f64 / SR as f64;
        if t <= times[0] {
            self.i = 0;
            return 0.0;
        }
        let last = times.len() - 1;
        if t >= times[last] {
            self.i = last;
            return last as f32;
        }
        if self.i > last || times[self.i] > t {
            self.i = 0; // 戻された。探し直す
        }
        while self.i + 1 <= last && times[self.i + 1] <= t {
            self.i += 1;
        }
        let a = times[self.i];
        let b = times[(self.i + 1).min(last)];
        let f = if b > a { ((t - a) / (b - a)) as f32 } else { 0.0 };
        self.i as f32 + f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(src: &str) -> Song {
        tonescript_song::load_str(src).expect("読めるはず")
    }

    const BASE: &str = r#"
        let BPM = 120;
        let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
        let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
        let GAINS = #{ lead: 1.5 };
        let MIX = #{ lead: #{ width: 1.0, reverb: 0.3, duck: 0.5 } };
        let AUTOMATION = #{ lead: #{ pan: [[0, -1.0], [32, 1.0]] } };
    "#;

    #[test]
    fn a_plan_carries_the_song_settings() {
        let s = song(BASE);
        let p = Plan::from_song(&s, &["lead".to_string(), "bass".to_string()]);
        assert_eq!(p.part_of("lead"), Some(0));
        assert_eq!(p.part_of("bass"), Some(1));
        assert_eq!(p.part_of("nope"), None);
        assert_eq!(p.parts[0].gain, 1.5);
        assert_eq!(p.parts[0].mix.reverb, 0.3);
        assert!(p.parts[0].pan_curve.is_some(), "線が写っていない");
        assert!(p.parts[0].gain_curve.is_none(), "書いていない線が付いている");
        // 書いていないパートは既定値
        assert_eq!(p.parts[1].gain, 1.0);
        assert_eq!(p.parts[1].mix.reverb, 0.0);
    }

    #[test]
    fn steps_and_samples_line_up() {
        let s = song(BASE);
        let p = Plan::from_song(&s, &[]);
        // 120BPM の4分音符 = 0.5秒 = 4目盛り
        assert!((p.time_of(4) - 0.5).abs() < 1e-6, "{}", p.time_of(4));
        assert_eq!(p.sample_of(4), (0.5 * SR as f64) as u64);
        assert_eq!(p.sample_of(0), 0);
    }

    #[test]
    fn the_cursor_walks_forward_and_back() {
        let s = song(BASE);
        let p = Plan::from_song(&s, &[]);
        let t = &p.step_times;
        let mut c = StepCursor::default();
        // 前へ順に
        for step in 0..8u32 {
            let at = p.sample_of(step);
            let got = c.step_at(at, t);
            assert!((got - step as f32).abs() < 0.01, "{step} 目盛りが {got} になった");
        }
        // 頭出しで戻しても合うこと
        let got = c.step_at(p.sample_of(2), t);
        assert!((got - 2.0).abs() < 0.01, "戻したら {got}");
        // 節と節のあいだは小数で返る
        let mid = (p.sample_of(2) + p.sample_of(3)) / 2;
        let got = c.step_at(mid, t);
        assert!((got - 2.5).abs() < 0.05, "あいだが {got}");
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let s = song(BASE);
        let p = Plan::from_song(&s, &[]);
        let t = &p.step_times;
        let mut c = StepCursor::default();
        assert_eq!(c.step_at(0, t), 0.0);
        // 曲の終わりより後でも落ちない
        let far = c.step_at(u32::MAX as u64, t);
        assert_eq!(far, (t.len() - 1) as f32);
    }

    #[test]
    fn an_empty_plan_is_safe() {
        let p = Plan::empty();
        assert_eq!(p.total, 0);
        assert_eq!(p.sample_of(99), 0);
        let mut c = StepCursor::default();
        assert_eq!(c.step_at(1000, &p.step_times), 0.0);
    }
}
