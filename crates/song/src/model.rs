//! 曲の形。Rhai で書かれた曲ファイルは、最後にこの形へ落ちる。
//!
//! Python 版では曲ファイルがそのまま Python の名前空間で、
//! `S.MELODY` のように直接触っていた。Rhai では値を受け取ってから
//! ここへ詰め直す。境目がはっきりするぶん、何が必須で何が任意かが
//! 型として残る。

use std::collections::HashMap;

/// 位置と長さの刻み。4分音符ひとつを 4 に割る（＝16分音符が1目盛り）。
///
/// これは「1小節の長さ」ではない。拍子によって1小節の目盛り数は変わる。
/// 4/4 なら 16、3/4 なら 12、7/8 なら 14。`Meter` を見ること。
pub const TICKS_PER_BEAT: u32 = 4;

/// 4/4 の1小節ぶんの目盛り数。拍子を書かなかったときの既定。
pub const STEPS_PER_BAR: u32 = 16;

/// 拍子。分子ぶんの拍を、分母の音符で数える。
///
/// 4/4 なら「4分音符を4つ」、6/8 なら「8分音符を6つ」。
/// 目盛り（16分）でいくつになるかは `steps()` で出す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    pub num: u32,
    pub den: u32,
}

impl Default for Meter {
    fn default() -> Self {
        Self { num: 4, den: 4 }
    }
}

impl Meter {
    pub fn new(num: u32, den: u32) -> Self {
        Self { num: num.max(1), den: den.max(1) }
    }

    /// 1小節ぶんの目盛り数。
    ///
    ///   4/4 -> 4 * 16/4 = 16
    ///   3/4 -> 3 * 16/4 = 12
    ///   6/8 -> 6 * 16/8 = 12
    ///   7/8 -> 7 * 16/8 = 14
    ///   5/4 -> 5 * 16/4 = 20
    pub fn steps(&self) -> u32 {
        (self.num * TICKS_PER_BEAT * 4 / self.den).max(1)
    }

    /// 1拍ぶんの目盛り数。テンポはこの単位で数える。
    ///
    /// 8分の曲（6/8 など）は「8分音符を1拍」と数えるので 2 になる。
    pub fn steps_per_beat(&self) -> u32 {
        (TICKS_PER_BEAT * 4 / self.den).max(1)
    }

    /// 「拍の頭」に当たる目盛りか。小節線の中の目印を描くのに使う。
    pub fn is_beat(&self, step_in_bar: u32) -> bool {
        step_in_bar % self.steps_per_beat() == 0
    }
}

impl std::fmt::Display for Meter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// ひとつの音符。
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// 曲の頭からの位置（16分いくつ分）
    pub pos: u32,
    /// 長さ（16分いくつ分）
    pub len: u32,
    /// MIDI ノート番号
    pub pitch: i32,
    /// 強さ 0〜127
    pub vel: u8,
    /// 歌詞のモーラ。無いなら空
    pub mora: String,
}

/// セクション（イントロ、Aメロ…）。
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    pub bars: u32,
    /// ベース型の名前
    pub bass: String,
    /// ドラムキットの名前
    pub kit: String,
    /// リフ型の名前
    pub arp: String,
    /// ベロシティ倍率
    pub gain: f32,
    /// 拍子。書かなければ 4/4。
    pub meter: Meter,
}

/// 和音。表示名、構成音、ベースの音。
#[derive(Clone, Debug)]
pub struct Chord {
    pub name: String,
    /// 構成音（MIDI ノート番号）
    pub tones: Vec<i32>,
    /// ベースの音（MIDI ノート番号）
    pub bass: i32,
}

/// 音色の割り当て。
#[derive(Clone, Debug)]
pub struct Voice {
    pub ch: u8,
    /// 自作シンセの音色名。ドラムなど音色を持たないものは None
    pub patch: Option<String>,
    /// GM 音源へ書き出すときの予備
    pub program: Option<u8>,
    pub volume: u8,
    pub label: String,
    pub color: String,
}

/// パートごとのミックス設定。
#[derive(PartialEq, Clone, Copy, Debug)]
pub struct MixCfg {
    /// 左右の広がり。0 = 完全中央
    pub width: f32,
    /// 残響の送り量
    pub reverb: f32,
    /// サイドチェインの掛かり具合
    pub duck: f32,
}

impl Default for MixCfg {
    fn default() -> Self {
        Self { width: 0.0, reverb: 0.0, duck: 0.0 }
    }
}

/// 音声そのものを置くトラック（外の歌声ソフトで作った WAV）。
#[derive(PartialEq, Clone, Debug)]
pub struct AudioTrack {
    /// TONESCRIPT_ROOT からの相対パス
    pub path: String,
    pub gain: f32,
    pub label: String,
    pub color: String,
    /// 曲のどこから鳴らすか（目盛り）。0 なら曲の頭
    pub at: u32,
    /// 頭を何秒落とすか。息継ぎや前の物音を切る
    pub trim_in: f32,
    /// 尻を何秒落とすか
    pub trim_out: f32,
    /// 何秒かけて入るか
    pub fade_in: f32,
    /// 何秒かけて消えるか
    pub fade_out: f32,
}

impl Default for AudioTrack {
    fn default() -> Self {
        Self {
            path: String::new(),
            gain: 1.0,
            label: String::new(),
            color: "#ff4d6d".into(),
            at: 0,
            trim_in: 0.0,
            trim_out: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
        }
    }
}

/// オートメーションの節。(目盛りの位置, 値)
pub type Point = (u32, f32);

/// オートメーションで動かせるもの。
///
/// DAW でいう「音量やパンを線で描く」やつ。曲ファイルでは
/// セクション単位の数値しか持てなかったので、途中で連続に動かせなかった。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    /// 音量の倍率。既定 1.0
    Gain,
    /// 左右。-1 が左、0 が中央、+1 が右。既定 0
    Pan,
    /// 残響の送り量。既定はパートの MIX 設定
    Reverb,
    /// サイドチェインの掛かり具合。既定はパートの MIX 設定
    Duck,
}

impl Lane {
    pub fn from_name(s: &str) -> Option<Lane> {
        match s {
            "gain" | "音量" => Some(Lane::Gain),
            "pan" | "左右" => Some(Lane::Pan),
            "reverb" | "残響" => Some(Lane::Reverb),
            "duck" | "ダッキング" => Some(Lane::Duck),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Lane::Gain => "gain",
            Lane::Pan => "pan",
            Lane::Reverb => "reverb",
            Lane::Duck => "duck",
        }
    }
    /// 節が1つも無いときの値。
    pub fn default_value(&self) -> f32 {
        match self {
            Lane::Gain => 1.0,
            Lane::Pan => 0.0,
            Lane::Reverb | Lane::Duck => f32::NAN, // MIX の設定を使うという印
        }
    }
    /// 許す範囲。外れた値は書いた時点で止める。
    pub fn range(&self) -> (f32, f32) {
        match self {
            Lane::Gain => (0.0, 8.0),
            Lane::Pan => (-1.0, 1.0),
            Lane::Reverb | Lane::Duck => (0.0, 2.0),
        }
    }
}

/// 1本の線。節を位置の順に並べたもの。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub points: Vec<Point>,
}

impl Curve {
    pub fn new(mut points: Vec<Point>) -> Self {
        points.sort_by_key(|p| p.0);
        Self { points }
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// その位置での値。節と節のあいだは直線で繋ぐ。
    ///
    /// 最初の節より前は最初の値、最後の節より後は最後の値のまま。
    /// 端で 0 へ落とすと、書いていない所が黙るという事故になる。
    pub fn at(&self, step: f32) -> Option<f32> {
        let p = &self.points;
        if p.is_empty() {
            return None;
        }
        if step <= p[0].0 as f32 {
            return Some(p[0].1);
        }
        if step >= p[p.len() - 1].0 as f32 {
            return Some(p[p.len() - 1].1);
        }
        // 挟む2つを探す
        let i = p.partition_point(|q| (q.0 as f32) <= step);
        let (a, b) = (p[i - 1], p[i]);
        if b.0 == a.0 {
            return Some(b.1);
        }
        let t = (step - a.0 as f32) / (b.0 - a.0) as f32;
        Some(a.1 + (b.1 - a.1) * t)
    }

    /// 節を置く。同じ位置にあれば差し替える。
    pub fn set(&mut self, step: u32, value: f32) {
        match self.points.binary_search_by_key(&step, |p| p.0) {
            Ok(i) => self.points[i].1 = value,
            Err(i) => self.points.insert(i, (step, value)),
        }
    }

    /// 節を消す。
    pub fn remove(&mut self, step: u32) -> bool {
        match self.points.binary_search_by_key(&step, |p| p.0) {
            Ok(i) => {
                self.points.remove(i);
                true
            }
            Err(_) => false,
        }
    }
}

/// パート -> 動かすもの -> 線。
pub type Automation = HashMap<String, HashMap<Lane, Curve>>;

/// ドラムのひと叩き。(16分位置, 強さ)
pub type Hit = (u32, u8);

/// ドラムキット。パート名 -> (MIDIノート番号, 打点)
pub type Kit = HashMap<String, (u8, Vec<Hit>)>;

/// 曲まるごと。
#[derive(Clone, Debug, Default)]
pub struct Song {
    pub title: String,
    pub bpm: f32,
    pub key: String,
    /// 通し小節 -> BPM
    pub tempo_map: HashMap<u32, f32>,
    /// 'linear' か 'smooth'
    pub tempo_curve: String,

    pub sections: Vec<Section>,
    /// 通し小節 -> 和音
    pub chords: HashMap<u32, Chord>,
    /// 通し小節 -> その小節の旋律
    pub melody: HashMap<u32, Vec<(Option<String>, u32, String)>>,
    /// 通し小節 -> 鳴らすパート
    pub arrange: HashMap<u32, Vec<String>>,
    /// 通し小節 -> 半音いくつ上げるか
    pub transpose: HashMap<u32, i32>,
    /// 通し小節 -> ベロシティ倍率
    pub bar_accent: HashMap<u32, f32>,

    /// 型の名前 -> (16分位置, 長さ, ルートからの半音差)
    pub bass_patterns: HashMap<String, Vec<(u32, u32, i32)>>,
    /// 型の名前 -> (16分位置, 長さ, 和音の何番目, 何オクターブ上)
    pub arp_patterns: HashMap<String, Vec<(u32, u32, usize, i32)>>,
    /// (16分位置, 長さ, ベロシティ)
    pub chord_pattern: Vec<(u32, u32, u8)>,
    /// キット名 -> キット
    pub drum_kits: HashMap<String, Kit>,
    /// 名前 -> (MIDIノート番号, 打点)。キットに依らない一発もの
    pub extra_hits: HashMap<String, (u8, Vec<Hit>)>,

    pub voices: HashMap<String, Voice>,
    pub edit_parts: Vec<String>,
    pub audio_tracks: HashMap<String, AudioTrack>,

    /// パート -> 音量
    pub gains: HashMap<String, f32>,
    pub master_gain: f32,
    pub mix: HashMap<String, MixCfg>,
    /// セクション名 -> 主旋律の楽器
    pub lead_patch: HashMap<String, String>,
    /// セクション名 -> パート -> 楽器
    pub section_patch: HashMap<String, HashMap<String, String>>,
    /// 音階（主音からの半音）
    pub scale: Vec<i32>,
    /// 主音のピッチクラス
    pub scale_root: i32,

    /// サイドチェイン (深さ, アタック秒, 保持秒, 戻り秒)
    pub sidechain: (f32, f32, f32, f32),
    /// 残響 (秒, 広がり)
    pub reverb: (f32, f32),
    pub master_lufs: f32,
    pub premix_lufs: f32,
    /// 音量・左右などを時間で動かす線
    pub automation: Automation,
    pub kick: tonescript_dsp::drum::KickCfg,
}

impl Song {
    /// 全部で何小節か。
    pub fn bars(&self) -> u32 {
        self.sections.iter().map(|s| s.bars).sum()
    }

    /// イントロの小節数。
    pub fn intro_bars(&self) -> u32 {
        self.sections.first().map(|s| s.bars).unwrap_or(0)
    }

    /// 通し小節 -> セクション名。
    pub fn section_of(&self) -> HashMap<u32, String> {
        let mut out = HashMap::new();
        let mut bar = 1;
        for s in &self.sections {
            for _ in 0..s.bars {
                out.insert(bar, s.name.clone());
                bar += 1;
            }
        }
        out
    }

    /// [(名前, 開始小節, 終了小節), ...]
    pub fn section_ranges(&self) -> Vec<(String, u32, u32)> {
        let mut out = Vec::new();
        let mut bar = 1;
        for s in &self.sections {
            out.push((s.name.clone(), bar, bar + s.bars - 1));
            bar += s.bars;
        }
        out
    }

    /// 通し小節 -> そのセクションの設定を引く。
    fn by_bar<'a, T, F: Fn(&'a Section) -> T>(&'a self, f: F) -> HashMap<u32, T> {
        let mut out = HashMap::new();
        let mut bar = 1;
        for s in &self.sections {
            for _ in 0..s.bars {
                out.insert(bar, f(s));
                bar += 1;
            }
        }
        out
    }

    pub fn bar_bass(&self) -> HashMap<u32, String> {
        self.by_bar(|s| s.bass.clone())
    }
    pub fn bar_kit(&self) -> HashMap<u32, String> {
        self.by_bar(|s| s.kit.clone())
    }
    pub fn bar_arp(&self) -> HashMap<u32, String> {
        self.by_bar(|s| s.arp.clone())
    }
    pub fn bar_gain(&self) -> HashMap<u32, f32> {
        self.by_bar(|s| s.gain)
    }

    pub fn bar_meter(&self) -> HashMap<u32, Meter> {
        self.by_bar(|s| s.meter)
    }

    /// その小節の拍子。
    pub fn meter_at(&self, bar: u32) -> Meter {
        let mut at = 1;
        for s in &self.sections {
            if bar < at + s.bars {
                return s.meter;
            }
            at += s.bars;
        }
        self.sections.last().map(|s| s.meter).unwrap_or_default()
    }

    /// 各小節が「曲の頭から何目盛り目に始まるか」。長さは小節数+1。
    ///
    /// 拍子が変わると小節の長さが変わるので、`(bar-1) * 16` では出せない。
    /// ここを1か所に集めておくと、拍子を足しても他が壊れない。
    pub fn bar_starts(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.bars() as usize + 1);
        let mut at = 0;
        for s in &self.sections {
            let step = s.meter.steps();
            for _ in 0..s.bars {
                out.push(at);
                at += step;
            }
        }
        out.push(at);
        out
    }

    /// その小節の始まる目盛り。bar は 1 から数える。
    pub fn bar_start(&self, bar: u32) -> u32 {
        let starts = self.bar_starts();
        let i = (bar.max(1) - 1) as usize;
        starts.get(i).copied().unwrap_or_else(|| *starts.last().unwrap_or(&0))
    }

    /// その小節の目盛り数。
    pub fn bar_steps(&self, bar: u32) -> u32 {
        self.meter_at(bar).steps()
    }

    /// 曲全体の目盛り数。
    pub fn total_steps(&self) -> u32 {
        *self.bar_starts().last().unwrap_or(&0)
    }

    /// 目盛りの位置から小節番号を出す。拍子が混ざっていても効く。
    pub fn bar_of_step(&self, step: u32) -> u32 {
        let starts = self.bar_starts();
        // starts は昇順なので、step 以下で最大のところを探す
        match starts.binary_search(&step) {
            Ok(i) => (i as u32 + 1).min(self.bars().max(1)),
            Err(0) => 1,
            Err(i) => (i as u32).min(self.bars().max(1)),
        }
    }

    /// 拍子が1つでも 4/4 でなければ true。表示の出し分けに使う。
    pub fn has_odd_meter(&self) -> bool {
        self.sections.iter().any(|s| s.meter != Meter::default())
    }

    /// その小節で鳴らすパートに入っているか。
    pub fn plays(&self, bar: u32, part: &str) -> bool {
        self.arrange.get(&bar).map(|v| v.iter().any(|p| p == part)).unwrap_or(false)
    }
}

/// 音名を MIDI ノート番号へ。`A4` = 69。
///
/// Python 版の `midiwriter.n()` と同じ決まり。`C#4` `Db4` どちらも読む。
pub fn note_number(name: &str) -> Option<i32> {
    let b = name.as_bytes();
    if b.is_empty() {
        return None;
    }
    let step = match b[0].to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let mut i = 1;
    let mut acc = 0;
    while i < b.len() && (b[i] == b'#' || b[i] == b'b') {
        acc += if b[i] == b'#' { 1 } else { -1 };
        i += 1;
    }
    let oct: i32 = name[i..].parse().ok()?;
    // MIDI の決まり: C4 = 60（いわゆる「真ん中のド」）
    Some((oct + 1) * 12 + step + acc)
}

/// MIDI ノート番号を周波数へ。
#[inline]
pub fn freq_of(pitch: i32) -> f32 {
    440.0 * 2.0f32.powf((pitch - 69) as f32 / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_interpolates_and_holds_the_ends() {
        let c = Curve::new(vec![(0, 1.0), (16, 0.0), (32, 0.5)]);
        assert_eq!(c.at(0.0), Some(1.0));
        assert_eq!(c.at(8.0), Some(0.5), "真ん中は直線で繋ぐ");
        assert_eq!(c.at(16.0), Some(0.0));
        assert_eq!(c.at(24.0), Some(0.25));
        assert_eq!(c.at(32.0), Some(0.5));
        // 端の外は端の値のまま。0 へ落とすと書いていない所が黙る
        assert_eq!(c.at(-5.0), Some(1.0));
        assert_eq!(c.at(999.0), Some(0.5));
        assert_eq!(Curve::default().at(0.0), None);
    }

    #[test]
    fn curve_points_stay_sorted() {
        let c = Curve::new(vec![(32, 0.5), (0, 1.0), (16, 0.0)]);
        assert_eq!(c.points, vec![(0, 1.0), (16, 0.0), (32, 0.5)]);
    }

    #[test]
    fn curve_set_and_remove() {
        let mut c = Curve::default();
        c.set(16, 0.5);
        c.set(0, 1.0);
        c.set(16, 0.25); // 同じ位置は差し替え
        assert_eq!(c.points, vec![(0, 1.0), (16, 0.25)]);
        assert!(c.remove(16));
        assert!(!c.remove(16));
        assert_eq!(c.points, vec![(0, 1.0)]);
    }

    #[test]
    fn lane_names_round_trip() {
        for n in ["gain", "pan", "reverb", "duck"] {
            assert_eq!(Lane::from_name(n).unwrap().name(), n);
        }
        assert_eq!(Lane::from_name("音量"), Some(Lane::Gain));
        assert_eq!(Lane::from_name("そんなレーンはない"), None);
        assert_eq!(Lane::Gain.default_value(), 1.0);
        assert_eq!(Lane::Pan.default_value(), 0.0);
        assert!(Lane::Reverb.default_value().is_nan(), "MIX を使うという印");
    }

    #[test]
    fn note_names_parse() {
        assert_eq!(note_number("A4"), Some(69));
        assert_eq!(note_number("C4"), Some(60));
        assert_eq!(note_number("C0"), Some(12));
        assert_eq!(note_number("G#3"), Some(56));
        assert_eq!(note_number("Ab3"), Some(56)); // 異名同音
        assert_eq!(note_number("A#2"), Some(46));
        assert_eq!(note_number("E2"), Some(40));
        assert_eq!(note_number("なにこれ"), None);
        assert_eq!(note_number(""), None);
    }

    #[test]
    fn frequencies_are_right() {
        assert!((freq_of(69) - 440.0).abs() < 1e-3);
        assert!((freq_of(57) - 220.0).abs() < 1e-3);
        assert!((freq_of(81) - 880.0).abs() < 1e-3);
    }

    fn sec(name: &str, bars: u32, meter: Meter) -> Section {
        Section { name: name.into(), bars, bass: "plain".into(), kit: "light".into(),
                  arp: "main".into(), gain: 1.0, meter }
    }

    #[test]
    fn meter_steps() {
        assert_eq!(Meter::new(4, 4).steps(), 16);
        assert_eq!(Meter::new(3, 4).steps(), 12);
        assert_eq!(Meter::new(6, 8).steps(), 12);
        assert_eq!(Meter::new(7, 8).steps(), 14);
        assert_eq!(Meter::new(5, 4).steps(), 20);
        assert_eq!(Meter::new(2, 2).steps(), 16);
        // 1拍ぶんの目盛り
        assert_eq!(Meter::new(4, 4).steps_per_beat(), 4);
        assert_eq!(Meter::new(6, 8).steps_per_beat(), 2);
        assert_eq!(Meter::new(3, 2).steps_per_beat(), 8);
        assert_eq!(Meter::new(4, 4).to_string(), "4/4");
    }

    #[test]
    fn bar_starts_follow_the_meter() {
        let s = Song {
            sections: vec![
                sec("A", 2, Meter::new(4, 4)),   // 16 + 16
                sec("B", 2, Meter::new(3, 4)),   // 12 + 12
                sec("C", 1, Meter::new(7, 8)),   // 14
            ],
            ..Default::default()
        };
        assert_eq!(s.bar_starts(), vec![0, 16, 32, 44, 56, 70]);
        assert_eq!(s.total_steps(), 70);
        assert_eq!(s.bar_start(1), 0);
        assert_eq!(s.bar_start(3), 32);
        assert_eq!(s.bar_start(5), 56);
        assert_eq!(s.bar_steps(1), 16);
        assert_eq!(s.bar_steps(3), 12);
        assert_eq!(s.bar_steps(5), 14);
        assert!(s.has_odd_meter());
        // 目盛り -> 小節
        assert_eq!(s.bar_of_step(0), 1);
        assert_eq!(s.bar_of_step(15), 1);
        assert_eq!(s.bar_of_step(16), 2);
        assert_eq!(s.bar_of_step(43), 3);
        assert_eq!(s.bar_of_step(44), 4);
        assert_eq!(s.bar_of_step(69), 5);
    }

    #[test]
    fn four_four_is_unchanged() {
        let s = Song { sections: vec![sec("A", 4, Meter::default())], ..Default::default() };
        assert_eq!(s.bar_starts(), vec![0, 16, 32, 48, 64]);
        assert!(!s.has_odd_meter());
    }

    #[test]
    fn section_layout() {
        let s = Song {
            sections: vec![
                Section { name: "イントロ".into(), bars: 8, bass: "plain".into(),
                          kit: "light".into(), arp: "main".into(), gain: 0.85,
                          meter: Meter::default() },
                Section { name: "サビ".into(), bars: 8, bass: "octa".into(),
                          kit: "full".into(), arp: "high".into(), gain: 1.0,
                          meter: Meter::default() },
            ],
            ..Default::default()
        };
        assert_eq!(s.bars(), 16);
        assert_eq!(s.intro_bars(), 8);
        assert_eq!(s.section_of()[&1], "イントロ");
        assert_eq!(s.section_of()[&9], "サビ");
        assert_eq!(s.section_ranges(), vec![
            ("イントロ".to_string(), 1, 8),
            ("サビ".to_string(), 9, 16),
        ]);
        assert_eq!(s.meter_at(1), Meter::default());
        assert_eq!(s.bar_kit()[&3], "light");
        assert_eq!(s.bar_kit()[&12], "full");
        assert!((s.bar_gain()[&1] - 0.85).abs() < 1e-6);
    }
}
