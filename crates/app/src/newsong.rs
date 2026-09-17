//! 新しい曲を作る。
//!
//! なぜ雛形を写さないのか
//! ----------------------
//! はじめは `songs/example.rhai` を写して数値だけ差し替える作りにしていた。
//! しかしあれはデモ曲で、16小節ぶんの旋律が手で書いてある。小節数を
//! 8 に減らすと 10小節目の旋律が曲の外へはみ出して読めなくなるし、
//! 拍子を 7/8 にすると「1小節の合計が 16」の前提が崩れる。
//!
//! そもそも DAW の「新規作成」は空の曲で、旋律は自分で描くもの。
//! ここでは指定から丸ごと組み立てる。何を書いても必ず通る形になる。
//!
//! 組み立てたものは、書き出す前に必ず読み直して確かめる。
//! 壊れたファイルを置かない。

use tonescript_song::model::Meter;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// 新しい曲の指定。
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// ファイル名（拡張子なし）
    pub name: String,
    /// 曲名
    pub title: String,
    pub bpm: u32,
    pub key: String,
    /// セクション1つあたりの小節数
    pub bars: u32,
    pub meter: Meter,
    /// 伴奏の型とドラムを入れておくか。
    /// 入れておくと、音符を1つも置かなくても鳴る。
    pub with_backing: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            name: "mysong".into(),
            title: "新しい曲".into(),
            bpm: 128,
            key: "A minor".into(),
            bars: 8,
            meter: Meter::default(),
            with_backing: true,
        }
    }
}

/// ファイル名に使えない文字を探す。
///
/// Windows のファイル名の決まりに加えて、コマンドの引数で困るものも外す。
pub fn bad_chars(name: &str) -> Option<char> {
    name.chars().find(|c| {
        c.is_control()
            || matches!(
                c,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '%' | '!' | '^' | '&'
                    | '(' | ')' | '[' | ']' | '{' | '}' | ';' | ',' | '=' | '+' | '\''
            )
    })
}

/// 指定に無理がないか確かめる。だめなら理由を返す。
pub fn check(spec: &Spec, songs_dir: &Path) -> Result<(), String> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err("ファイル名を入れてください".into());
    }
    if let Some(c) = bad_chars(name) {
        return Err(format!("ファイル名に使えない文字が入っています: {c}"));
    }
    if name.starts_with('.') || name.starts_with('_') {
        return Err("ファイル名の先頭に . や _ は使えません".into());
    }
    if name.chars().count() > 60 {
        return Err("ファイル名が長すぎます".into());
    }
    if path_of(songs_dir, name).exists() {
        return Err(format!("{name} はもうあります"));
    }
    if !(20..=400).contains(&spec.bpm) {
        return Err(format!("BPM が {} です。20〜400 の間に", spec.bpm));
    }
    if !(1..=512).contains(&spec.bars) {
        return Err(format!("小節数が {} です。1〜512 の間に", spec.bars));
    }
    if !(1..=32).contains(&spec.meter.num) || ![1, 2, 4, 8, 16].contains(&spec.meter.den) {
        return Err(format!("拍子 {} は扱えません", spec.meter));
    }
    Ok(())
}

pub fn path_of(songs_dir: &Path, name: &str) -> PathBuf {
    songs_dir.join(format!("{name}.rhai"))
}

/// Rhai の文字列に入れて安全な形にする。
fn q(s: &str) -> String {
    s.replace('\\', "").replace('"', "").replace(['\n', '\r'], " ")
}

/// 指定から曲ファイルの中身を組み立てる。
pub fn build(spec: &Spec) -> String {
    let m = spec.meter;
    let steps = m.steps();
    let per_beat = m.steps_per_beat();
    let beats = m.num;
    let bars = spec.bars;
    let meter_suffix = if m == Meter::default() {
        String::new()
    } else {
        format!(", [{}, {}]", m.num, m.den)
    };

    let mut s = String::new();
    let _ = writeln!(s, "// {}", q(&spec.title));
    let _ = writeln!(s, "//");
    let _ = writeln!(s, "// 音符は画面で描く。ここは曲の骨組みだけ。");
    let _ = writeln!(s, "// 位置と長さの単位は16分音符。1小節 = {steps}（拍子 {m}）。");
    let _ = writeln!(s);
    let _ = writeln!(s, "let TITLE = \"{}\";", q(&spec.title));
    let _ = writeln!(s, "let BPM = {};", spec.bpm);
    let _ = writeln!(s, "let KEY = \"{}\";", q(&spec.key));
    let _ = writeln!(s, "let TEMPO_MAP = #{{ \"1\": {} }};", spec.bpm);
    let _ = writeln!(s, "let TEMPO_CURVE = \"smooth\";");
    let _ = writeln!(s);

    let _ = writeln!(s, "// [名前, 小節数, ベース型, ドラムキット, リフ型, 音量{}]",
                     if meter_suffix.is_empty() { "" } else { ", 拍子" });
    let _ = writeln!(s, "let SECTIONS = [");
    let _ = writeln!(s, "    [\"イントロ\", {bars}, \"plain\", \"light\", \"main\", 0.85{meter_suffix}],");
    let _ = writeln!(s, "    [\"サビ\",     {bars}, \"octa\",  \"full\",  \"high\", 1.00{meter_suffix}],");
    let _ = writeln!(s, "];");
    let _ = writeln!(s);

    // --- 和音。i-VI-III-VII を回す
    let _ = writeln!(s, "// [表示名, [構成音...], ベースの音]");
    let _ = writeln!(s, "let Am = [\"Am\", [\"A3\", \"C4\", \"E4\"], \"A2\"];");
    let _ = writeln!(s, "let F  = [\"F\",  [\"F3\", \"A3\", \"C4\"], \"F2\"];");
    let _ = writeln!(s, "let C  = [\"C\",  [\"C4\", \"E4\", \"G4\"], \"C3\"];");
    let _ = writeln!(s, "let G  = [\"G\",  [\"G3\", \"B3\", \"D4\"], \"G2\"];");
    let _ = writeln!(s, "// i-VI-III-VII。解決しないので何周でも回せる、定番の並び。");
    let _ = writeln!(s, "let prog = [Am, F, C, G];");
    let _ = writeln!(s, "let CHORDS = #{{}};");
    let _ = writeln!(s, "for i in steps(0, {}, 1) {{", bars * 2);
    let _ = writeln!(s, "    CHORDS[\"\" + (i + 1)] = prog[i % prog.len()];");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s);

    // --- 旋律は空。画面で描く
    let _ = writeln!(s, "// 旋律は画面で描く。ここに書けば譜面にも出る。");
    let _ = writeln!(s, "//   \"1\": bar([[{}, \"A4\"], [{}, \"C5\"]]),   // 合計を {steps} に",
                     steps / 2, steps - steps / 2);
    let _ = writeln!(s, "let MELODY = #{{}};");
    let _ = writeln!(s);

    // --- 編成
    let parts = if spec.with_backing {
        "[\"lead\", \"chords\", \"bass\", \"sub\", \"arp\", \"kick\", \"clap\", \"closedhat\", \"shaker\"]"
    } else {
        "[\"lead\"]"
    };
    let _ = writeln!(s, "// どの小節で何を鳴らすか。ここに無いパートはその小節で黙る。");
    let _ = writeln!(s, "let parts = {parts};");
    let _ = writeln!(s, "let ARRANGE = #{{}};");
    let _ = writeln!(s, "for i in steps(1, {}, 1) {{", bars * 2 + 1);
    let _ = writeln!(s, "    ARRANGE[\"\" + i] = parts;");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s);

    // --- 伴奏の型。拍子から作るので、何拍子でも合う
    let _ = writeln!(s, "// 伴奏の型。1拍 = {per_beat}目盛り、1小節 = {beats}拍。");
    let _ = writeln!(s, "let CHORD_PATTERN = [];");
    let _ = writeln!(s, "for i in steps(0, {beats}, 2) {{");
    let _ = writeln!(s, "    CHORD_PATTERN.push([i * {per_beat} + 1, 1, 80]);");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s);
    let _ = writeln!(s, "// リフ: [位置, 長さ, 和音の何番目の音, 何オクターブ上]");
    let _ = writeln!(s, "let main_arp = [];");
    let _ = writeln!(s, "let high_arp = [];");
    let _ = writeln!(s, "for i in steps(0, {steps}, 2) {{");
    let _ = writeln!(s, "    main_arp.push([i, 2, (i / 2) % 3, 0]);");
    let _ = writeln!(s, "    high_arp.push([i, 2, (i / 2) % 3, 1]);");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "let ARP_PATTERNS = #{{ main: main_arp, high: high_arp }};");
    let _ = writeln!(s);
    let _ = writeln!(s, "// ベース: [位置, 長さ, ルートからの半音差]");
    let _ = writeln!(s, "let plain = [];");
    let _ = writeln!(s, "for i in steps(0, {beats}, 1) {{");
    let _ = writeln!(s, "    plain.push([i * {per_beat}, {per_beat}, 0]);");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "// 根音と1オクターブ上を同時に叩いて刻む");
    let _ = writeln!(s, "let octa = [];");
    let _ = writeln!(s, "for i in steps(0, {steps}, 2) {{");
    let _ = writeln!(s, "    octa.push([i, 2, 0]);");
    let _ = writeln!(s, "    octa.push([i, 2, 12]);");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "let BASS_PATTERNS = #{{ plain: plain, octa: octa }};");
    let _ = writeln!(s);

    // --- ドラム。拍から作る
    let _ = writeln!(s, "// ドラム。拍の頭でキック、裏でハット。");
    let _ = writeln!(s, "let four = [];");
    let _ = writeln!(s, "let four_hard = [];");
    let _ = writeln!(s, "let off_hat = [];");
    let _ = writeln!(s, "let shake = [];");
    let _ = writeln!(s, "for i in steps(0, {beats}, 1) {{");
    let _ = writeln!(s, "    four.push([i * {per_beat}, 100]);");
    let _ = writeln!(s, "    four_hard.push([i * {per_beat}, 126]);");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "for i in steps(0, {steps}, 2) {{ shake.push([i, 46]); }}");
    let _ = writeln!(s, "for i in steps({}, {steps}, {}) {{ off_hat.push([i, 54]); }}",
                     per_beat / 2, per_beat.max(1));
    let _ = writeln!(s, "let clap_at = [];");
    if beats >= 2 {
        let _ = writeln!(s, "clap_at.push([{}, 112]);", per_beat);
        if beats >= 4 {
            let _ = writeln!(s, "clap_at.push([{}, 112]);", per_beat * 3);
        }
    }
    let _ = writeln!(s, "let DRUM_KITS = #{{");
    let _ = writeln!(s, "    light: #{{ kick: [36, four], closedhat: [42, off_hat],");
    let _ = writeln!(s, "              openhat: [46, []], clap: [39, []], shaker: [70, shake] }},");
    let _ = writeln!(s, "    full:  #{{ kick: [36, four_hard], closedhat: [42, off_hat],");
    let _ = writeln!(s, "              openhat: [46, []], clap: [39, clap_at], shaker: [70, shake] }},");
    let _ = writeln!(s, "}};");
    let _ = writeln!(s, "let EXTRA_HITS = #{{ crash: [49, [[0, 112]]] }};");
    let _ = writeln!(s);

    // --- 音色とミックス
    let _ = writeln!(s, "// patch は音色名。69 種類ある（tone patches で一覧）。自分で作ることもできる。");
    let _ = writeln!(s, "let VOICES = #{{");
    let _ = writeln!(s, "    lead:   #{{ ch: 0,  patch: \"supersaw\", program: 81,  volume: 100,");
    let _ = writeln!(s, "               label: \"主旋律\",     color: \"#ff9f43\" }},");
    let _ = writeln!(s, "    chords: #{{ ch: 1,  patch: \"stab\",     program: 62,  volume: 82,");
    let _ = writeln!(s, "               label: \"コード\",     color: \"#9c88ff\" }},");
    let _ = writeln!(s, "    bass:   #{{ ch: 2,  patch: \"acid\",     program: 38,  volume: 104,");
    let _ = writeln!(s, "               label: \"ベース\",     color: \"#26de81\" }},");
    let _ = writeln!(s, "    // 40〜90Hz を持続音で支える土台。bass はその上で刻む");
    let _ = writeln!(s, "    sub:    #{{ ch: 5,  patch: \"sub\",      program: 39,  volume: 110,");
    let _ = writeln!(s, "               label: \"サブベース\", color: \"#00b894\" }},");
    let _ = writeln!(s, "    arp:    #{{ ch: 3,  patch: \"pluck\",    program: 80,  volume: 106,");
    let _ = writeln!(s, "               label: \"アルペジオ\", color: \"#54a0ff\" }},");
    let _ = writeln!(s, "    fx:     #{{ ch: 4,                     program: 119, volume: 96,");
    let _ = writeln!(s, "               label: \"効果音\",     color: \"#b2bec3\" }},");
    let _ = writeln!(s, "    drums:  #{{ ch: 9,  volume: 112, label: \"ドラム\",   color: \"#fd79a8\" }},");
    let _ = writeln!(s, "    perc:   #{{ ch: 9,  volume: 100, label: \"パーカス\", color: \"#54a0ff\" }},");
    let _ = writeln!(s, "    vocal:  #{{ ch: 10, volume: 100, label: \"歌\",       color: \"#ff4d6d\" }},");
    let _ = writeln!(s, "}};");
    let _ = writeln!(s, "let EDIT_PARTS = [\"lead\", \"arp\", \"chords\", \"bass\", \"sub\", \"drums\", \"perc\", \"vocal\"];");
    let _ = writeln!(s);
    let _ = writeln!(s, "// 外で作った歌（Synthesizer V など）を置く。パスは TONESCRIPT_ROOT からの相対。");
    let _ = writeln!(s, "let AUDIO_TRACKS = #{{");
    let _ = writeln!(s, "    // main: #{{ path: \"Vocal/main.wav\", gain: 1.00, label: \"メイン\" }},");
    let _ = writeln!(s, "}};");
    let _ = writeln!(s);
    let _ = writeln!(s, "let SCALE = [0, 2, 3, 5, 7, 8, 10];   // 自然短音階");
    let _ = writeln!(s, "let SCALE_ROOT = 9;                    // A のピッチクラス");
    let _ = writeln!(s);
    let _ = writeln!(s, "let GAINS = #{{ lead: 1.55, chords: 1.60, bass: 0.90, arp: 1.00,");
    let _ = writeln!(s, "               drums: 1.55, perc: 1.20, fx: 1.40, vocal: 2.30, sub: 1.60 }};");
    let _ = writeln!(s, "let MASTER_GAIN = 1.0;");
    let _ = writeln!(s);
    let _ = writeln!(s, "// width は左右の広がり（0=中央）、reverb は残響、duck はサイドチェイン。");
    let _ = writeln!(s, "// 低音を広げると芯がぼやけるので bass と sub は中央に置く。");
    let _ = writeln!(s, "let MIX = #{{");
    let _ = writeln!(s, "    lead:   #{{ width: 1.35, reverb: 0.26, duck: 0.55 }},");
    let _ = writeln!(s, "    arp:    #{{ width: 1.60, reverb: 0.30, duck: 0.90 }},");
    let _ = writeln!(s, "    chords: #{{ width: 1.50, reverb: 0.34, duck: 1.00 }},");
    let _ = writeln!(s, "    bass:   #{{ width: 0.00, reverb: 0.02, duck: 1.00 }},");
    let _ = writeln!(s, "    sub:    #{{ width: 0.00, reverb: 0.00, duck: 1.00 }},");
    let _ = writeln!(s, "    drums:  #{{ width: 0.45, reverb: 0.08, duck: 0.00 }},");
    let _ = writeln!(s, "    perc:   #{{ width: 0.85, reverb: 0.14, duck: 0.35 }},");
    let _ = writeln!(s, "    fx:     #{{ width: 1.20, reverb: 0.30, duck: 0.30 }},");
    let _ = writeln!(s, "    vocal:  #{{ width: 0.00, reverb: 0.22, duck: 0.40 }},");
    let _ = writeln!(s, "}};");
    let _ = writeln!(s);
    let _ = writeln!(s, "// 音量・左右を時間で動かす線。[位置, 値] を並べる。");
    let _ = writeln!(s, "let AUTOMATION = #{{");
    let _ = writeln!(s, "    // lead: #{{ gain: [[0, 1.0], [{}, 0.3]] }},", bars * 2 * steps);
    let _ = writeln!(s, "}};");
    let _ = writeln!(s);
    let _ = writeln!(s, "// [凹む深さ, アタック秒, 保持秒, 戻り秒]。EDM の「ポンプ感」の正体。");
    let _ = writeln!(s, "let SIDECHAIN = [0.70, 0.003, 0.020, 0.200];");
    let _ = writeln!(s, "let KICK = #{{ weight: 1.15, body: 1.20, click: 0.85, length: 1.15, tail_hz: 78.0 }};");
    let _ = writeln!(s, "let REVERB = [1.9, 4.2];");
    let _ = writeln!(s, "let MASTER_LUFS = -9.0;");
    let _ = writeln!(s, "let PREMIX_LUFS = -20.0;");
    s
}

/// 実際に作る。作ったファイルの場所を返す。
pub fn create(spec: &Spec, songs_dir: &Path) -> Result<PathBuf, String> {
    check(spec, songs_dir)?;
    let text = build(spec);

    // 書く前に読み直して確かめる。壊れたものを置かない
    let song = tonescript_song::load_str(&text).map_err(|e| format!("作った曲が読めません: {e}"))?;
    if song.bars() == 0 {
        return Err("小節が1つもありません".into());
    }
    // 譜面まで組めることも見ておく。ここで落ちると画面が開けない
    tonescript_render::build(&song).map_err(|e| format!("譜面を組めません: {e}"))?;

    let path = path_of(songs_dir, spec.name.trim());
    std::fs::create_dir_all(songs_dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("mp_new_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_new_song_loads_and_builds() {
        let spec = Spec::default();
        let text = build(&spec);
        let s = tonescript_song::load_str(&text).expect("読めるはず");
        assert_eq!(s.title, "新しい曲");
        assert_eq!(s.bpm, 128.0);
        assert_eq!(s.bars(), 16, "イントロ8 + サビ8");
        let score = tonescript_render::build(&s).expect("譜面が組めるはず");
        // 旋律は空。伴奏は鳴る
        assert!(score.get("lead").is_none_or(|v| v.is_empty()), "旋律が入っている");
        assert!(!score["bass"].is_empty(), "伴奏が鳴らない");
        assert!(!score["drums"].is_empty(), "ドラムが鳴らない");
    }

    #[test]
    fn any_bar_count_works() {
        // ここが雛形を写す作りで壊れていた所。
        for bars in [1u32, 2, 3, 4, 8, 16, 32, 64] {
            let spec = Spec { bars, ..Default::default() };
            let s = tonescript_song::load_str(&build(&spec))
                .unwrap_or_else(|e| panic!("{bars}小節で読めない: {e}"));
            assert_eq!(s.bars(), bars * 2);
            tonescript_render::build(&s).unwrap_or_else(|e| panic!("{bars}小節で組めない: {e}"));
        }
    }

    #[test]
    fn any_meter_works() {
        // 拍子を変えても、伴奏の型が小節からはみ出さないこと
        for (num, den) in [(4u32, 4u32), (3, 4), (6, 8), (7, 8), (5, 4), (2, 2), (12, 8)] {
            let spec = Spec { meter: Meter::new(num, den), bars: 4, ..Default::default() };
            let text = build(&spec);
            let s = tonescript_song::load_str(&text)
                .unwrap_or_else(|e| panic!("{num}/{den} で読めない: {e}"));
            assert_eq!(s.meter_at(1), Meter::new(num, den));
            let score = tonescript_render::build(&s)
                .unwrap_or_else(|e| panic!("{num}/{den} で組めない: {e}"));
            // 音符が小節からはみ出していないこと
            let total = s.total_steps();
            for (part, notes) in &score {
                for n in notes {
                    assert!(
                        n.pos + n.len <= total,
                        "{num}/{den} の {part} が曲の外へ出た（{} + {} > {total}）",
                        n.pos,
                        n.len
                    );
                }
            }
            assert!(!score["bass"].is_empty(), "{num}/{den} でベースが鳴らない");
        }
    }

    #[test]
    fn without_backing_only_the_lead_lane_exists() {
        let spec = Spec { with_backing: false, ..Default::default() };
        let s = tonescript_song::load_str(&build(&spec)).unwrap();
        let score = tonescript_render::build(&s).unwrap();
        assert!(score.get("bass").is_none_or(|v| v.is_empty()), "伴奏が鳴っている");
        assert!(score.get("drums").is_none_or(|v| v.is_empty()), "ドラムが鳴っている");
    }

    #[test]
    fn create_writes_a_file_that_opens() {
        let d = tmp("create");
        let spec = Spec {
            name: "mysong".into(),
            title: "私の曲".into(),
            bpm: 150,
            key: "C major".into(),
            bars: 4,
            meter: Meter::new(3, 4),
            with_backing: true,
        };
        let path = create(&spec, &d).expect("作れるはず");
        assert!(path.exists());
        let s = tonescript_song::load_file(&path).expect("開けるはず");
        assert_eq!(s.title, "私の曲");
        assert_eq!(s.bpm, 150.0);
        assert_eq!(s.bars(), 8);
        assert_eq!(s.meter_at(1), Meter::new(3, 4));
        assert_eq!(s.bar_steps(1), 12);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn bad_names_are_refused() {
        let d = tmp("names");
        for (name, why) in [
            ("", "空"),
            ("   ", "空白だけ"),
            ("a/b", "区切り"),
            ("a*b", "ワイルドカード"),
            ("a%b", "環境変数"),
            (".hidden", "先頭のドット"),
            ("_tpl", "先頭のアンダースコア"),
        ] {
            let spec = Spec { name: name.into(), ..Default::default() };
            assert!(check(&spec, &d).is_err(), "通ってしまった: {why}（{name}）");
        }
        for name in ["mysong", "曲1", "my-song", "my_song2", "スシトテトダンス"] {
            let spec = Spec { name: name.into(), ..Default::default() };
            assert!(check(&spec, &d).is_ok(), "弾かれた: {name}");
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn duplicate_name_is_refused() {
        let d = tmp("dup");
        std::fs::write(d.join("taken.rhai"), "x").unwrap();
        let spec = Spec { name: "taken".into(), ..Default::default() };
        let e = check(&spec, &d).unwrap_err();
        assert!(e.contains("もうあります"), "{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn out_of_range_values_are_refused() {
        let d = tmp("range");
        for (spec, what) in [
            (Spec { bpm: 5, ..Default::default() }, "BPM 低すぎ"),
            (Spec { bpm: 9999, ..Default::default() }, "BPM 高すぎ"),
            (Spec { bars: 0, ..Default::default() }, "小節 0"),
            (Spec { bars: 9999, ..Default::default() }, "小節 多すぎ"),
            (Spec { meter: Meter { num: 0, den: 4 }, ..Default::default() }, "分子 0"),
            (Spec { meter: Meter { num: 4, den: 3 }, ..Default::default() }, "分母 3"),
        ] {
            assert!(check(&spec, &d).is_err(), "通ってしまった: {what}");
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn quotes_in_the_title_do_not_break_the_file() {
        let spec = Spec {
            title: "変な\"名前\\です".into(),
            key: "A\"minor".into(),
            ..Default::default()
        };
        tonescript_song::load_str(&build(&spec)).expect("壊れた曲ができた");
    }

    #[test]
    fn the_generated_file_is_readable_by_a_person() {
        // 中身がコメント付きで読めること。数字の羅列だけにしない
        let text = build(&Spec::default());
        assert!(text.contains("// 音符は画面で描く"));
        assert!(text.contains("69 種類ある"));
        assert!(text.contains("低音を広げると芯がぼやける"));
        // 行が極端に長くないこと
        for (i, l) in text.lines().enumerate() {
            assert!(l.chars().count() <= 100, "{}行目が長すぎる: {l}", i + 1);
        }
    }
}
