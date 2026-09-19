//! 音色の作り方を、**数の並びとして**持つ。
//!
//! なぜこれが要るか
//! ----------------
//! 内蔵の46音色は Rust の関数として書いてある。つまり**新しい楽器を作るには
//! Rust を書いて作り直すしかない。** 曲はテキストで書けるのに、音はそうでは
//! なかった。
//!
//! ここは同じ部品（発振器・エンベロープ・フィルタ・倍音・FM・歪み）を、
//! 表に書いた数から組み立てる。曲ファイルに `PATCHES` を書けば、それが
//! そのまま楽器になる。**人が書いてもいいし、AI に書かせてもいい。**
//!
//! 組み立ての順
//! ------------
//! ```text
//!   発振器を重ねる ─┬→ 音量のかたち ─→ フィルタ ─→ 歪み ─→ ディレイ ─→ 音量
//!   倍音を足す     ─┤        ↑              ↑
//!   FM を足す      ─┤    ビブラート    フィルタのかたち
//!   頭の雑音を足す ─┘
//! ```
//!
//! 内蔵の音色と同じ部品を通るので、**混ぜて使っても浮かない。**

use crate::env;
use crate::filter::{self, LadderMode};
use crate::osc;
use crate::rng::Pcg64;
use crate::shape;
use crate::osc::SR;

/// 発振器1本。
#[derive(Clone, Debug, PartialEq)]
pub struct Osc {
    pub wave: Wave,
    /// 混ぜる量
    pub mix: f32,
    /// 音程のずれ（セント）。少しずらして重ねると太くなる
    pub detune: f32,
    /// 何オクターブ上下させるか
    pub octave: i32,
    /// `string` のとき: 伸びる秒数
    pub decay: f32,
    /// `string` のとき: 明るさ 0〜1（低いほど早く丸くなる）
    pub bright: f32,
    /// `string` のとき: 弾く位置 0〜1（0.5 で真ん中、端ほど硬い）
    pub pick: f32,
    /// `pulse` のとき: 上に居る割合 0〜1。0.5 で矩形波、細いほど鼻に掛かる
    pub width: f32,
}

impl Default for Osc {
    fn default() -> Self {
        Self {
            wave: Wave::Saw,
            mix: 1.0,
            detune: 0.0,
            octave: 0,
            decay: 2.0,
            bright: 0.5,
            pick: 0.25,
            width: 0.5,
        }
    }
}

/// 波の形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Saw,
    Square,
    Sine,
    /// 音程を持たない雑音。息や打撃に使う
    Noise,
    /// 撥いた弦。波形ではなく弦そのものを真似る（[`crate::string`]）。
    /// ギター・ベース・琴・ハープはこれでないと弦に聞こえない
    String,
    /// 幅を選べる矩形波。**ファミコンの音がこれ。**
    /// `width` で 12.5% / 25% / 50% を切り替える
    Pulse,
}

impl Wave {
    pub fn from_name(s: &str) -> Option<Wave> {
        Some(match s {
            "saw" => Wave::Saw,
            "square" => Wave::Square,
            "sine" => Wave::Sine,
            "noise" => Wave::Noise,
            "string" => Wave::String,
            "pulse" => Wave::Pulse,
            _ => return None,
        })
    }

    pub const NAMES: [&'static str; 6] =
        ["saw", "square", "sine", "noise", "string", "pulse"];
}

/// 音量のかたち。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Env {
    /// 立ち上がり（秒）
    pub a: f32,
    /// 落ちる時間（秒）
    pub d: f32,
    /// 伸ばしている間の高さ（0〜1）
    pub s: f32,
    /// 離してから消えるまで（秒）
    pub r: f32,
}

impl Default for Env {
    fn default() -> Self {
        Self { a: 0.005, d: 0.10, s: 0.7, r: 0.10 }
    }
}

/// フィルタの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    /// 4極のはしご型。太く、よく歪む
    Ladder,
    /// 低い所を落とす
    Highpass,
    /// その辺だけ残す
    Bandpass,
    /// 掛けない
    None,
}

impl FilterKind {
    pub fn from_name(s: &str) -> Option<FilterKind> {
        Some(match s {
            "ladder" | "lowpass" => FilterKind::Ladder,
            "highpass" => FilterKind::Highpass,
            "bandpass" => FilterKind::Bandpass,
            "none" => FilterKind::None,
            _ => return None,
        })
    }

    pub const NAMES: [&'static str; 5] = ["ladder", "lowpass", "highpass", "bandpass", "none"];
}

/// フィルタ。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    /// いちばん閉じたときの高さ（Hz）
    pub base: f32,
    /// かたちで開く量（Hz）。0 なら動かない
    pub sweep: f32,
    /// 響き。上げすぎると発振する
    pub res: f32,
    /// 音程についてくる量。1.0 で完全に追う（高い音ほど開く）
    pub track: f32,
    /// 強く弾いたときに開く量（Hz）
    pub vel: f32,
    /// 開き方のかたち
    pub env: (f32, f32, f32),
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            kind: FilterKind::None,
            base: 1200.0,
            sweep: 4000.0,
            res: 0.2,
            track: 0.0,
            vel: 0.0,
            env: (0.001, 0.08, 2.0),
        }
    }
}

/// 頭に混ぜる雑音。撥弦の爪や、息の立ち上がり。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Attack {
    pub amount: f32,
    /// これより下を落とす（Hz）
    pub hp: f32,
    pub a: f32,
    pub d: f32,
}

/// 揺れ。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vibrato {
    /// 1秒に何回
    pub rate: f32,
    /// どれだけ（1.0 = 倍の音程。ふつうは 0.01 くらい）
    pub depth: f32,
    /// 鳴り始めてから何秒後に掛かり始めるか
    pub delay: f32,
}

/// FM。片方の波で、もう片方の音程を揺らす。金属質な音になる。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Fm {
    /// 揺らす側の音程（元の音程の何倍か）
    pub ratio: f32,
    /// どれだけ揺らすか
    pub index: f32,
    /// 揺れが落ちる時間（秒）
    pub decay: f32,
}

/// 音量の揺れ。ビブラフォンの回転や、オルガンの回るスピーカー。
///
/// [`Vibrato`] は**音程**を揺らすが、こちらは**音量**を揺らす。
/// 別物なので両方持てる。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Tremolo {
    /// 1秒に何回
    pub rate: f32,
    /// どれだけ 0〜1。1 で音量が 0 まで落ちる
    pub depth: f32,
}

/// やまびこ。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Delay {
    pub time: f32,
    pub feedback: f32,
    pub mix: f32,
}

/// 音色1つの作り方。
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub osc: Vec<Osc>,
    /// 倍音を直に足す `[音程の倍率, 大きさ, 落ちる時間]`。
    /// 鐘やガムランのような、倍音が整数倍でない音に使う
    pub partials: Vec<(f32, f32, f32)>,
    pub env: Env,
    pub filter: Filter,
    pub attack: Attack,
    pub vibrato: Vibrato,
    pub tremolo: Tremolo,
    pub fm: Fm,
    pub delay: Delay,
    /// 胴鳴り `[中心の高さ, 鋭さ, 混ぜる量]`。
    /// ギターらしさの半分は弦ではなく胴にある
    pub body: Vec<(f32, f32, f32)>,
    /// 歪み。1.0 で掛けない
    pub drive: f32,
    /// 最後の音量
    pub gain: f32,
    /// 音価より長く鳴る秒数（撥弦や鐘の余韻）
    pub ring: f32,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            osc: vec![Osc::default()],
            partials: Vec::new(),
            env: Env::default(),
            filter: Filter::default(),
            attack: Attack::default(),
            vibrato: Vibrato::default(),
            tremolo: Tremolo::default(),
            fm: Fm::default(),
            delay: Delay::default(),
            body: Vec::new(),
            drive: 1.0,
            gain: 0.7,
            ring: 0.0,
        }
    }
}

/// 数のおかしい所を言う。**鳴らす前に気付けるように。**
///
/// 「鳴らしてみたら無音だった」を減らすのが目的なので、
/// 直したほうがいいものは全部ここで言う。
pub fn check(r: &Recipe) -> Result<(), String> {
    if r.osc.is_empty() && r.partials.is_empty() && r.attack.amount <= 0.0 {
        return Err("音の素がありません（osc か partials か attack のどれかが要ります）".into());
    }
    if r.osc.len() > 16 {
        return Err(format!("osc が {} 本あります。16 本までに", r.osc.len()));
    }
    if r.partials.len() > 64 {
        return Err(format!("partials が {} 個あります。64 個までに", r.partials.len()));
    }
    let total: f32 = r.osc.iter().map(|o| o.mix.abs()).sum();
    if !r.osc.is_empty() && total <= 0.0 {
        return Err("osc の mix が全部 0 です。音が出ません".into());
    }
    for (i, o) in r.osc.iter().enumerate() {
        if !(-4800.0..=4800.0).contains(&o.detune) {
            return Err(format!("osc[{i}] の detune が {} です。±4800 セントまでに", o.detune));
        }
        if !(-4..=4).contains(&o.octave) {
            return Err(format!("osc[{i}] の octave が {} です。±4 までに", o.octave));
        }
        if o.wave == Wave::Pulse && !(0.02..=0.98).contains(&o.width) {
            return Err(format!("osc[{i}] の width が {} です。0.02〜0.98 の間に", o.width));
        }
        if o.wave == Wave::String {
            if !(0.02..=20.0).contains(&o.decay) {
                return Err(format!("osc[{i}] の decay が {} 秒です。0.02〜20 の間に", o.decay));
            }
            if !(0.0..=1.0).contains(&o.bright) {
                return Err(format!("osc[{i}] の bright が {} です。0〜1 の間に", o.bright));
            }
            if !(0.0..=1.0).contains(&o.pick) {
                return Err(format!("osc[{i}] の pick が {} です。0〜1 の間に", o.pick));
            }
        }
    }
    let e = &r.env;
    for (name, v) in [("a", e.a), ("d", e.d), ("r", e.r)] {
        if !(0.0..=30.0).contains(&v) {
            return Err(format!("env.{name} が {v} 秒です。0〜30 の間に"));
        }
    }
    if !(0.0..=1.0).contains(&e.s) {
        return Err(format!("env.s が {} です。0〜1 の間に", e.s));
    }
    if r.filter.kind != FilterKind::None {
        let f = &r.filter;
        if !(20.0..=20_000.0).contains(&f.base) {
            return Err(format!("filter.base が {} Hz です。20〜20000 の間に", f.base));
        }
        if !(0.0..=20_000.0).contains(&f.sweep) {
            return Err(format!("filter.sweep が {} Hz です。0〜20000 の間に", f.sweep));
        }
        // 響きを上げすぎると、音ではなく発振になる
        if !(0.0..=0.95).contains(&f.res) {
            return Err(format!("filter.res が {} です。0〜0.95 の間に", f.res));
        }
    }
    if !(0.1..=20.0).contains(&r.drive) {
        return Err(format!("drive が {} です。0.1〜20 の間に", r.drive));
    }
    if !(0.0..=8.0).contains(&r.gain) {
        return Err(format!("gain が {} です。0〜8 の間に", r.gain));
    }
    if !(0.0..=8.0).contains(&r.ring) {
        return Err(format!("ring が {} 秒です。0〜8 の間に", r.ring));
    }
    if r.fm.ratio < 0.0 || r.fm.ratio > 64.0 {
        return Err(format!("fm.ratio が {} です。0〜64 の間に", r.fm.ratio));
    }
    if !(0.0..=1.0).contains(&r.tremolo.depth) {
        return Err(format!("tremolo.depth が {} です。0〜1 の間に", r.tremolo.depth));
    }
    if !(0.0..=1.0).contains(&r.delay.feedback) {
        return Err(format!("delay.feedback が {} です。0〜1 の間に", r.delay.feedback));
    }
    Ok(())
}

/// 作り方から音を作る。内蔵の音色と同じ部品を通る。
pub fn render(r: &Recipe, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    let mut rng = Pcg64::new(seed);

    // 揺れ。音程に掛ける倍率の列
    let wob = if r.vibrato.depth > 0.0 && r.vibrato.rate > 0.0 {
        Some(shape::vib(n, r.vibrato.rate, r.vibrato.depth, r.vibrato.delay, 0.0, seed))
    } else {
        None
    };
    // FM。音程をさらに揺らす
    let fm = if r.fm.index > 0.0 && r.fm.ratio > 0.0 {
        let d = r.fm.decay.max(1e-3);
        let mf = freq * r.fm.ratio;
        Some(
            (0..n)
                .map(|i| {
                    let t = i as f32 / SR;
                    let e = (-t / d).exp();
                    1.0 + r.fm.index * e * (std::f32::consts::TAU * mf * t).sin() * 0.1
                })
                .collect::<Vec<f32>>(),
        )
    } else {
        None
    };

    // 発振器を重ねる
    let mut out = vec![0.0f32; n];
    let total: f32 = r.osc.iter().map(|o| o.mix.abs()).sum::<f32>().max(1e-6);
    for o in &r.osc {
        let f0 = freq * 2f32.powi(o.octave) * 2f32.powf(o.detune / 1200.0);
        if f0 >= SR * 0.49 && o.wave != Wave::Noise {
            continue; // 高すぎる。折り返すだけなので鳴らさない
        }
        let phase = rng.next_f64() as f32;
        let g = o.mix / total;
        let wave = match o.wave {
            Wave::Noise => (0..n).map(|_| rng.next_f64() as f32 * 2.0 - 1.0).collect::<Vec<f32>>(),
            // 弦は自分で減衰を持つ。音程の揺れは効かない（弦は揺らせない）
            Wave::String => crate::string::pluck(
                f0,
                n,
                crate::string::Pluck { decay: o.decay, bright: o.bright, pick: o.pick },
                rng.next_u64(),
            ),
            _ if (wob.is_some() || fm.is_some()) && o.wave != Wave::String => {
                // 音程が動く。列で渡す
                let f: Vec<f32> = (0..n)
                    .map(|i| {
                        let mut v = f0;
                        if let Some(w) = &wob {
                            v *= w[i];
                        }
                        if let Some(m) = &fm {
                            v *= m[i];
                        }
                        v
                    })
                    .collect();
                match o.wave {
                    Wave::Saw => osc::saw_var(&f, phase),
                    Wave::Square => osc::square_var(&f, phase),
                    Wave::Pulse => osc::pulse_var(&f, phase, o.width),
                    Wave::Sine => shape::sine_sweep(&f, phase),
                    Wave::Noise | Wave::String => unreachable!(),
                }
            }
            Wave::Saw => osc::saw(f0, n, phase),
            Wave::Square => osc::square(f0, n, phase),
            Wave::Pulse => osc::pulse(f0, n, phase, o.width),
            Wave::Sine => shape::sine(f0, n, phase),
        };
        shape::add_scaled(&mut out, &wave, g);
    }

    // 倍音を直に足す（鐘やガムラン）
    if !r.partials.is_empty() {
        let p = shape::partials(freq, n, &r.partials, seed ^ 0x9e37, 0.0);
        shape::add_scaled(&mut out, &p, 1.0);
    }

    // 音量のかたち
    let e = &r.env;
    shape::mul_in_place(&mut out, &env::lead(n, e.a, e.d, e.s, e.r));

    // 音量の揺れ
    if r.tremolo.depth > 0.0 && r.tremolo.rate > 0.0 {
        let d = r.tremolo.depth.clamp(0.0, 1.0);
        for (i, v) in out.iter_mut().enumerate() {
            let t = i as f32 / SR;
            // 1 を中心に上下させる。深さ 1 で 0 まで落ちる
            *v *= 1.0 - d * 0.5 * (1.0 - (std::f32::consts::TAU * r.tremolo.rate * t).cos());
        }
    }

    // フィルタ
    if r.filter.kind != FilterKind::None {
        let f = &r.filter;
        let fenv = env::ad(n, f.env.0, f.env.1, f.env.2);
        // 音程についてくる量。高い音ほど開く
        let track = 1.0 + f.track * (freq / 440.0 - 1.0);
        let cut: Vec<f32> = fenv
            .iter()
            .map(|x| ((f.base + f.sweep * x) * track + f.vel * vel).clamp(20.0, SR * 0.45))
            .collect();
        out = match f.kind {
            FilterKind::Ladder => {
                filter::ladder(&out, &cut, f.res, LadderMode::Zdf, 1.0 + f.res)
            }
            FilterKind::Highpass => filter::highpass(&out, f.base),
            FilterKind::Bandpass => filter::bandpass(&out, f.base, 1.0 + f.res * 8.0),
            FilterKind::None => out,
        };
    }

    // 頭の雑音
    if r.attack.amount > 0.0 {
        let a = &r.attack;
        let mut pick: Vec<f32> = (0..n).map(|_| rng.next_f64() as f32 * 2.0 - 1.0).collect();
        shape::mul_in_place(&mut pick, &env::ad(n, a.a.max(1e-5), a.d.max(1e-4), 6.0));
        if a.hp > 20.0 {
            pick = filter::highpass(&pick, a.hp);
        }
        shape::add_scaled(&mut out, &pick, a.amount);
    }

    // 胴鳴り。弦だけだと痩せて聞こえる
    if !r.body.is_empty() {
        out = crate::string::body(&out, &r.body);
    }

    // 歪み
    if (r.drive - 1.0).abs() > 1e-3 {
        out = shape::driven(&out, r.drive);
    }
    // やまびこ
    if r.delay.mix > 0.0 && r.delay.time > 0.0 {
        out = shape::delay(&out, r.delay.time, r.delay.feedback, r.delay.mix, 6);
    }
    shape::scale(&mut out, r.gain * vel);
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

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |a, b| a.max(b.abs()))
    }

    /// 周期を数えて音程を測る。
    fn pitch_of(x: &[f32], sr: f32) -> f32 {
        let cross = x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        cross as f32 * sr / x.len() as f32
    }

    #[test]
    fn the_default_recipe_makes_a_sound() {
        let r = Recipe::default();
        assert!(check(&r).is_ok(), "{:?}", check(&r));
        let w = render(&r, 440.0, 24_000, 1.0, 1);
        assert_eq!(w.len(), 24_000);
        assert!(rms(&w) > 0.01, "音が出ていない: {}", rms(&w));
    }

    #[test]
    fn the_pitch_is_what_was_asked_for() {
        let r = Recipe { osc: vec![Osc { wave: Wave::Sine, ..Default::default() }], ..Default::default() };
        for hz in [110.0f32, 440.0, 880.0] {
            let w = render(&r, hz, 24_000, 1.0, 1);
            let got = pitch_of(&w, SR);
            assert!((got - hz).abs() < hz * 0.05, "{hz}Hz を頼んで {got}Hz");
        }
    }

    #[test]
    fn an_octave_up_is_twice_the_pitch() {
        let base = Osc { wave: Wave::Sine, ..Default::default() };
        let a = render(&Recipe { osc: vec![base.clone()], ..Default::default() }, 220.0, 24_000, 1.0, 1);
        let b = render(
            &Recipe { osc: vec![Osc { octave: 1, ..base }], ..Default::default() },
            220.0,
            24_000,
            1.0,
            1,
        );
        let (pa, pb) = (pitch_of(&a, SR), pitch_of(&b, SR));
        assert!((pb / pa - 2.0).abs() < 0.1, "{pa}Hz → {pb}Hz");
    }

    #[test]
    fn detune_moves_the_pitch_a_little() {
        let base = Osc { wave: Wave::Sine, ..Default::default() };
        let a = render(&Recipe { osc: vec![base.clone()], ..Default::default() }, 440.0, 48_000, 1.0, 1);
        // 100セント = 半音上
        let b = render(
            &Recipe { osc: vec![Osc { detune: 100.0, ..base }], ..Default::default() },
            440.0,
            48_000,
            1.0,
            1,
        );
        let (pa, pb) = (pitch_of(&a, SR), pitch_of(&b, SR));
        assert!((pb / pa - 1.0595).abs() < 0.02, "半音上が {pa}Hz → {pb}Hz");
    }

    #[test]
    fn the_envelope_shapes_the_sound() {
        // 立ち上がりが遅いものは、頭が小さいこと
        let slow = Recipe {
            env: Env { a: 0.5, d: 0.1, s: 1.0, r: 0.1 },
            ..Default::default()
        };
        let w = render(&slow, 440.0, 48_000, 1.0, 1);
        let head = rms(&w[..2400]);
        let mid = rms(&w[24_000..26_400]);
        assert!(head < mid * 0.3, "頭が {head}、真ん中が {mid}");
    }

    #[test]
    fn a_closed_filter_takes_the_top_off() {
        let open = Recipe {
            filter: Filter { kind: FilterKind::None, ..Default::default() },
            ..Default::default()
        };
        let shut = Recipe {
            filter: Filter {
                kind: FilterKind::Ladder,
                base: 200.0,
                sweep: 0.0,
                res: 0.1,
                ..Default::default()
            },
            ..Default::default()
        };
        let a = render(&open, 440.0, 24_000, 1.0, 1);
        let b = render(&shut, 440.0, 24_000, 1.0, 1);
        // 高い所が落ちれば、隣り合うサンプルの差が小さくなる
        let rough = |x: &[f32]| -> f32 {
            x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / x.len() as f32
        };
        assert!(rough(&b) < rough(&a) * 0.6, "閉じても {} → {}", rough(&a), rough(&b));
    }

    #[test]
    fn partials_make_an_inharmonic_sound() {
        // 鐘。整数倍でない倍音を足すと、音程が1つに決まらない
        let r = Recipe {
            osc: Vec::new(),
            partials: vec![(1.0, 0.5, 2.0), (2.76, 0.3, 1.5), (5.4, 0.2, 1.0)],
            env: Env { a: 0.001, d: 2.0, s: 0.0, r: 0.5 },
            ..Default::default()
        };
        assert!(check(&r).is_ok());
        let w = render(&r, 440.0, 48_000, 1.0, 1);
        assert!(rms(&w) > 0.005, "鳴っていない");
    }

    #[test]
    fn velocity_changes_how_loud_it_is() {
        let r = Recipe::default();
        let soft = render(&r, 440.0, 24_000, 0.3, 1);
        let loud = render(&r, 440.0, 24_000, 1.0, 1);
        assert!(rms(&loud) > rms(&soft) * 2.0, "強さが効いていない");
    }

    #[test]
    fn the_same_recipe_and_seed_make_the_same_sound() {
        // 毎回同じ音になること。でないと再生と書き出しがずれる
        let r = Recipe {
            osc: vec![Osc { wave: Wave::Noise, ..Default::default() }],
            ..Default::default()
        };
        let a = render(&r, 440.0, 4800, 1.0, 7);
        let b = render(&r, 440.0, 4800, 1.0, 7);
        assert_eq!(a, b, "同じ作り方と種で違う音になった");
        let c = render(&r, 440.0, 4800, 1.0, 8);
        assert_ne!(a, c, "種を変えても同じ音になった");
    }

    #[test]
    fn it_never_comes_out_silent_by_accident() {
        // よくある形を並べて、どれも音が出ること
        let cases: Vec<(&str, Recipe)> = vec![
            ("のこぎり1本", Recipe::default()),
            (
                "重ねたのこぎり",
                Recipe {
                    osc: vec![
                        Osc { detune: -12.0, ..Default::default() },
                        Osc { detune: 12.0, ..Default::default() },
                    ],
                    ..Default::default()
                },
            ),
            (
                "フィルタ付き",
                Recipe {
                    filter: Filter {
                        kind: FilterKind::Ladder,
                        base: 800.0,
                        sweep: 6000.0,
                        res: 0.4,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
            (
                "FM",
                Recipe {
                    osc: vec![Osc { wave: Wave::Sine, ..Default::default() }],
                    fm: Fm { ratio: 2.0, index: 3.0, decay: 0.5 },
                    ..Default::default()
                },
            ),
            (
                "撥弦",
                Recipe {
                    attack: Attack { amount: 0.3, hp: 2000.0, a: 0.0003, d: 0.01 },
                    env: Env { a: 0.001, d: 0.3, s: 0.2, r: 0.1 },
                    ..Default::default()
                },
            ),
            (
                "やまびこ",
                Recipe {
                    delay: Delay { time: 0.12, feedback: 0.4, mix: 0.3 },
                    ..Default::default()
                },
            ),
        ];
        for (name, r) in cases {
            assert!(check(&r).is_ok(), "{name}: {:?}", check(&r));
            let w = render(&r, 440.0, 24_000, 1.0, 1);
            assert!(rms(&w) > 0.005, "{name} が無音（実効 {}）", rms(&w));
            assert!(peak(&w) < 8.0, "{name} が大きすぎる（ピーク {}）", peak(&w));
            assert!(w.iter().all(|v| v.is_finite()), "{name} に数でない値が出た");
        }
    }

    #[test]
    fn bad_numbers_are_refused_with_a_reason() {
        let cases: Vec<(&str, Recipe)> = vec![
            ("素が無い", Recipe { osc: Vec::new(), ..Default::default() }),
            (
                "mix が全部 0",
                Recipe {
                    osc: vec![Osc { mix: 0.0, ..Default::default() }],
                    ..Default::default()
                },
            ),
            (
                "響きが高すぎ",
                Recipe {
                    filter: Filter { kind: FilterKind::Ladder, res: 5.0, ..Default::default() },
                    ..Default::default()
                },
            ),
            ("歪みが 0", Recipe { drive: 0.0, ..Default::default() }),
            ("余韻が長すぎ", Recipe { ring: 100.0, ..Default::default() }),
            (
                "かたちが長すぎ",
                Recipe { env: Env { a: 100.0, ..Default::default() }, ..Default::default() },
            ),
        ];
        for (name, r) in cases {
            let got = check(&r);
            assert!(got.is_err(), "{name} が通ってしまった");
            assert!(!got.unwrap_err().is_empty(), "{name} の理由が空");
        }
    }

    #[test]
    fn the_names_map_to_the_parts() {
        assert_eq!(Wave::from_name("saw"), Some(Wave::Saw));
        assert_eq!(Wave::from_name("noise"), Some(Wave::Noise));
        assert_eq!(Wave::from_name("nope"), None);
        assert_eq!(FilterKind::from_name("lowpass"), Some(FilterKind::Ladder));
        assert_eq!(FilterKind::from_name("bandpass"), Some(FilterKind::Bandpass));
        assert_eq!(FilterKind::from_name("nope"), None);
    }

    #[test]
    fn a_pitch_too_high_to_play_is_skipped_not_folded() {
        // 24kHz を超える音程は、鳴らすと折り返して変な音になる
        let r = Recipe {
            osc: vec![Osc { wave: Wave::Sine, octave: 4, ..Default::default() }],
            ..Default::default()
        };
        let w = render(&r, 4000.0, 4800, 1.0, 1);
        assert!(rms(&w) < 1e-6, "折り返した音が出ている: {}", rms(&w));
    }
}
