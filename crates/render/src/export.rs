//! 書き出しの形を決める。
//!
//! 中で作る音は 48kHz の小数のまま。ここは**最後に形を整える所**だけを
//! 持つ。周波数を変える、深さを変える、範囲を切る。
//!
//! 96kHz のこと
//! ------------
//! 48kHz で作ったものを 96kHz で書いても、**中身は増えない。** 元より上の
//! 音が生えてくることはないので、ファイルが倍になるだけ。それでも選べる
//! ようにしてあるのは、相手の決まりで「96kHz で出せ」と言われることが
//! あるから。音が良くなると思って選ぶものではない。
//!
//! 44.1kHz は別で、これは**必要な変換**。48kHz のまま CD 用の箱へ入れると
//! 8.8% 速く再生される。
//!
//! 丸めの粉（ディザ）
//! ------------------
//! 小数を 16bit の整数へ落とすと、小さい音のところで段が付く。段は
//! 「ジリジリ」という規則的な歪みとして聞こえる。**わざと微かな雑音を
//! 足して**段をばらけさせると、歪みではなく小さな雑音になる。人の耳には
//! そのほうが自然に聞こえる。
//!
//! 24bit や 32bit では要らない（段が細かすぎて聞こえない）。

use crate::mix::Stereo;

/// 書き出しの形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub rate: Rate,
    pub depth: Depth,
    /// 16bit へ落とすときに丸めの粉を足すか
    pub dither: bool,
}

impl Default for Format {
    fn default() -> Self {
        Self { rate: Rate::R48000, depth: Depth::I16, dither: true }
    }
}

/// サンプリング周波数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    /// CD と同じ。48kHz からの変換が要る
    R44100,
    /// 中で作っているのと同じ。変換なし
    R48000,
    /// 倍。中身は増えない
    R96000,
}

impl Rate {
    pub fn hz(self) -> u32 {
        match self {
            Rate::R44100 => 44_100,
            Rate::R48000 => 48_000,
            Rate::R96000 => 96_000,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Rate::R44100 => "44.1kHz",
            Rate::R48000 => "48kHz",
            Rate::R96000 => "96kHz",
        }
    }

    pub const ALL: [Rate; 3] = [Rate::R44100, Rate::R48000, Rate::R96000];
}

/// 1サンプルを何ビットで書くか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// 配信も CD もこれ
    I16,
    /// 作業の受け渡し用
    I24,
    /// 小数のまま。天井を超えたぶんも残る
    F32,
}

impl Depth {
    pub fn bits(self) -> u16 {
        match self {
            Depth::I16 => 16,
            Depth::I24 => 24,
            Depth::F32 => 32,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Depth::I16 => "16bit",
            Depth::I24 => "24bit",
            Depth::F32 => "32bit 小数",
        }
    }

    pub const ALL: [Depth; 3] = [Depth::I16, Depth::I24, Depth::F32];
}

/// 周波数を変える。
///
/// 間は直線で結ぶ。凝った変換ではないが、**48kHz から 44.1kHz は比が
/// 近い**（1.088 倍）ので、ここで粗が出るのは元々ほとんど音の無い
/// 20kHz 付近だけになる。
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || x.is_empty() {
        return x.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let n = (x.len() as f64 * ratio).round() as usize;
    let mut out = Vec::with_capacity(n);
    let last = x.len() - 1;
    for i in 0..n {
        let t = i as f64 / ratio;
        let k = t as usize;
        let f = (t - k as f64) as f32;
        let a = x[k.min(last)];
        let b = x[(k + 1).min(last)];
        out.push(a + (b - a) * f);
    }
    out
}

/// 範囲を切り出す。`to` が `from` 以下なら、まるごと返す。
pub fn cut(s: &Stereo, from: usize, to: usize) -> Stereo {
    let n = s.len();
    if to <= from || from >= n {
        return s.clone();
    }
    let to = to.min(n);
    Stereo { l: s.l[from..to].to_vec(), r: s.r[from..to].to_vec() }
}

/// 書き出す形へ整える。周波数を合わせるだけ（深さは書くときに決まる）。
pub fn shape(s: &Stereo, from_hz: u32, fmt: Format) -> Stereo {
    let hz = fmt.rate.hz();
    if hz == from_hz {
        return s.clone();
    }
    Stereo { l: resample(&s.l, from_hz, hz), r: resample(&s.r, from_hz, hz) }
}

/// 丸めの粉を作る。三角形に散らばる、とても小さい雑音。
///
/// 種を渡すので、同じ曲を2回書き出せば同じものが出る。
/// **書き出すたびに中身が変わる**と、同じかどうかを確かめられない。
pub struct Dither {
    state: u32,
    scale: f32,
}

impl Dither {
    pub fn new(depth: Depth) -> Option<Dither> {
        // 16bit のときだけ。24bit 以上は段が細かすぎて聞こえない
        (depth == Depth::I16).then(|| Dither { state: 0x2545_f491, scale: 1.0 / 32768.0 })
    }

    fn next(&mut self) -> f32 {
        // xorshift。外の道具を入れずに済ませる
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        (self.state as f32 / u32::MAX as f32) - 0.5
    }

    /// 1サンプルぶん。三角形にするため2つ足す
    pub fn run(&mut self) -> f32 {
        (self.next() + self.next()) * self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, hz: f32, sr: f32) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * std::f32::consts::TAU * hz / sr).sin()).collect()
    }

    #[test]
    fn the_same_rate_is_untouched() {
        let x = tone(100, 440.0, 48_000.0);
        assert_eq!(resample(&x, 48_000, 48_000), x);
    }

    #[test]
    fn the_length_in_seconds_survives() {
        // 1秒ぶん入れたら、どの周波数でも1秒ぶん
        let x = vec![0.0f32; 48_000];
        for to in [44_100u32, 96_000] {
            let y = resample(&x, 48_000, to);
            assert!(
                (y.len() as i32 - to as i32).abs() < 2,
                "{to}Hz で {} サンプルになった",
                y.len()
            );
        }
    }

    #[test]
    fn the_pitch_survives() {
        // 440Hz が 440Hz のまま出ること。山の数で見る
        let sr = 48_000.0;
        let x = tone(48_000, 440.0, sr);
        let cross = |s: &[f32]| s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        for to in [44_100u32, 96_000] {
            let y = resample(&x, 48_000, to);
            let (a, b) = (cross(&x), cross(&y));
            assert!((a as i32 - b as i32).abs() <= 1, "{to}Hz で山が {a} から {b} に変わった");
        }
    }

    #[test]
    fn cutting_takes_only_that_part() {
        let s = Stereo { l: (0..100).map(|i| i as f32).collect(), r: vec![0.0; 100] };
        let c = cut(&s, 10, 20);
        assert_eq!(c.len(), 10);
        assert_eq!(c.l[0], 10.0);
        // 逆向きや外れた指定は、まるごと返す（黙って空にしない）
        assert_eq!(cut(&s, 20, 10).len(), 100);
        assert_eq!(cut(&s, 500, 600).len(), 100);
        // 終わりが長すぎても、あるぶんだけ
        assert_eq!(cut(&s, 90, 999).len(), 10);
    }

    #[test]
    fn shaping_only_changes_the_rate_when_it_has_to() {
        let s = Stereo { l: tone(1000, 440.0, 48_000.0), r: tone(1000, 440.0, 48_000.0) };
        let same = shape(&s, 48_000, Format { rate: Rate::R48000, ..Default::default() });
        assert_eq!(same.l, s.l, "同じ周波数なのに触った");
        let down = shape(&s, 48_000, Format { rate: Rate::R44100, ..Default::default() });
        assert!(down.len() < s.len(), "44.1kHz で短くなっていない");
        let up = shape(&s, 48_000, Format { rate: Rate::R96000, ..Default::default() });
        assert!(up.len() > s.len());
    }

    #[test]
    fn dither_is_only_for_sixteen_bits() {
        assert!(Dither::new(Depth::I16).is_some());
        assert!(Dither::new(Depth::I24).is_none(), "24bit に粉を足している");
        assert!(Dither::new(Depth::F32).is_none());
    }

    #[test]
    fn dither_is_tiny_and_centred() {
        let mut d = Dither::new(Depth::I16).unwrap();
        let v: Vec<f32> = (0..10_000).map(|_| d.run()).collect();
        let peak = v.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        // 16bit の1段（1/32768 = 0.0000305）ぶんに収まっていること
        assert!(peak < 1.0 / 32768.0, "粉が大きすぎる: {peak}");
        let mean: f32 = v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean.abs() < 1e-6, "粉が偏っている: {mean}");
    }

    #[test]
    fn the_same_song_dithers_the_same_way() {
        // 2回書き出して中身が違うと、同じかどうか確かめられない
        let mut a = Dither::new(Depth::I16).unwrap();
        let mut b = Dither::new(Depth::I16).unwrap();
        for _ in 0..100 {
            assert_eq!(a.run(), b.run());
        }
    }

    #[test]
    fn the_labels_say_what_you_get() {
        assert_eq!(Rate::R44100.hz(), 44_100);
        assert_eq!(Rate::R96000.hz(), 96_000);
        assert_eq!(Depth::I24.bits(), 24);
        assert_eq!(Depth::F32.bits(), 32);
        assert_eq!(Rate::ALL.len(), 3);
        assert_eq!(Depth::ALL.len(), 3);
    }
}
