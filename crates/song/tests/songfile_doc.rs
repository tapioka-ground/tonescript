//! `SONGFILE.md` に書いてあることが本当か確かめる。
//!
//! 仕様書は、実装とずれた瞬間に「無いより悪いもの」になる。
//! 人が読むだけでなく AI にも渡すので、なおさら嘘が混じると困る。
//! ここでは文書に載せた例をそのまま動かして、書いてある通りになるか見る。

use tonescript_song::model::Meter;
use tonescript_song::{load_str, Song};

fn load(src: &str) -> Song {
    load_str(src).unwrap_or_else(|e| panic!("SONGFILE.md の例が読めない: {e}\n---\n{src}"))
}

/// §0 の「30秒で分かる最小の曲」
const MINIMAL: &str = r#"
let BPM = 120;
let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
let MELODY = #{
    "1": bar([[4, "C4"], [4, "E4"], [8, "G4"]]),
    "2": bar([[16, "C5"]]),
};
let ARRANGE = #{ "1": ["lead"], "2": ["lead"] };
"#;

#[test]
fn the_minimal_example_works() {
    let s = load(MINIMAL);
    assert_eq!(s.bpm, 120.0);
    assert_eq!(s.bars(), 2);
    assert_eq!(s.melody.len(), 2);
    // 文書どおり「ド・ミ・ソー、ドー」になっているか
    let bar1 = &s.melody[&1];
    assert_eq!(bar1.len(), 3);
    assert_eq!(bar1[0].2, "C4");
    assert_eq!(bar1[2].1, 8, "ソが8目盛り（2分音符）");
}

#[test]
fn only_three_values_are_required() {
    // §3「必須は BPM SECTIONS VOICES の3つだけ」
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
                 let VOICES = #{ lead: #{ ch: 0 } };"#;
    assert!(load_str(src).is_ok(), "3つだけで通らない");

    for (drop, name) in [
        ("let BPM = 120;", "BPM"),
        (r#"let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];"#, "SECTIONS"),
        ("let VOICES = #{ lead: #{ ch: 0 } };", "VOICES"),
    ] {
        let e = load_str(&src.replace(drop, "")).unwrap_err().to_string();
        assert!(e.contains(name), "{name} が無いのに通った、または理由が違う: {e}");
    }
}

#[test]
fn bpm_range_is_20_to_400() {
    // §3「20〜400。範囲外は読み込みで弾かれる」
    let mk = |bpm: &str| MINIMAL.replace("let BPM = 120;", &format!("let BPM = {bpm};"));
    assert!(load_str(&mk("20")).is_ok());
    assert!(load_str(&mk("400")).is_ok());
    assert!(load_str(&mk("19")).is_err(), "19 が通った");
    assert!(load_str(&mk("401")).is_err(), "401 が通った");
}

#[test]
fn the_meter_table_is_right() {
    // §1 の表。1小節の目盛り = 分子 × 16 ÷ 分母
    for (num, den, steps, per_beat) in [
        (4u32, 4u32, 16u32, 4u32),
        (3, 4, 12, 4),
        (2, 4, 8, 4),
        (5, 4, 20, 4),
        (6, 8, 12, 2),
        (7, 8, 14, 2),
        (12, 8, 24, 2),
    ] {
        let m = Meter::new(num, den);
        assert_eq!(m.steps(), steps, "{num}/{den} の1小節");
        assert_eq!(m.steps_per_beat(), per_beat, "{num}/{den} の1拍");
    }
}

#[test]
fn the_note_length_table_is_right() {
    // §1「16分=1、8分=2、付点8分=3、4分=4、付点4分=6、2分=8、全音符=16」
    // 合計が 16 になる組み合わせが通ることで確かめる
    for parts in [
        "[[1,\"C4\"],[1,\"C4\"],[2,\"C4\"],[3,\"C4\"],[4,\"C4\"],[5,\"C4\"]]",
        "[[2,\"C4\"],[2,\"C4\"],[4,\"C4\"],[8,\"C4\"]]",
        "[[6,\"C4\"],[6,\"C4\"],[4,\"C4\"]]",
        "[[16,\"C4\"]]",
    ] {
        let src = format!("{MINIMAL}\nlet M2 = bar({parts});");
        assert!(load_str(&src).is_ok(), "合計 16 なのに通らない: {parts}");
    }
}

#[test]
fn a_wrong_melody_sum_says_both_numbers() {
    // §10「メッセージに 16/12 のように出る」
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 1, "p", "k", "m", 1.0, [3, 4]]];
                 let VOICES = #{ lead: #{ ch: 0 } };
                 let MELODY = #{ "1": [[16, "C4"]] };"#;
    let e = load_str(src).unwrap_err().to_string();
    assert!(e.contains("16/12"), "合計が出ていない: {e}");
    assert!(e.contains("3/4"), "拍子が出ていない: {e}");
}

#[test]
fn rest_is_written_as_rest() {
    // §4「休符は音名を "rest" にする」
    let src = MINIMAL.replace(
        r#""2": bar([[16, "C5"]]),"#,
        r#""2": bar([[4, "C5"], [4, "rest"], [8, "G4"]]),"#,
    );
    let s = load(&src);
    let score = tonescript_render::build(&s).unwrap();
    // 休符は音符にならない。1小節目3つ + 2小節目2つ
    assert_eq!(score["lead"].len(), 5, "休符が音になっている");
}

#[test]
fn the_seventh_element_of_a_section_is_the_meter() {
    // §3 の表
    let src = MINIMAL.replace(
        r#"[["A", 2, "p", "k", "m", 1.0]]"#,
        r#"[["A", 2, "p", "k", "m", 1.0, [7, 8]]]"#,
    );
    // 7/8 なので旋律の合計は 14
    let src = src
        .replace(r#"bar([[4, "C4"], [4, "E4"], [8, "G4"]])"#, r#"[[4, "C4"], [4, "E4"], [6, "G4"]]"#)
        .replace(r#"bar([[16, "C5"]])"#, r#"[[14, "C5"]]"#);
    let s = load(&src);
    assert_eq!(s.meter_at(1), Meter::new(7, 8));
    assert_eq!(s.bar_steps(1), 14);
}

#[test]
fn without_arrange_nothing_sounds() {
    // §4「ARRANGE を書かないと何も鳴らない。これが一番多い原因」
    let src = MINIMAL.replace(r#"let ARRANGE = #{ "1": ["lead"], "2": ["lead"] };"#, "");
    let s = load(&src);
    let score = tonescript_render::build(&s).unwrap();
    let total: usize = score.values().map(|v| v.len()).sum();
    assert_eq!(total, 0, "ARRANGE 無しで音が出た");
}

#[test]
fn an_unknown_pattern_name_makes_that_part_silent() {
    // §3「型の名前が無いとそのパートは黙る（エラーにはならない）」
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 1, "そんな型は無い", "k", "m", 1.0]];
                 let VOICES = #{ bass: #{ ch: 2, patch: "acid" } };
                 let CHORDS = #{ "1": ["Am", ["A3","C4","E4"], "A2"] };
                 let BASS_PATTERNS = #{ plain: [[0,4,0]] };
                 let ARRANGE = #{ "1": ["bass"] };"#;
    let s = load(src);
    let score = tonescript_render::build(&s).unwrap();
    assert!(score.get("bass").is_none_or(|v| v.is_empty()), "鳴ってしまった");
}

#[test]
fn sub_needs_no_pattern_and_fills_the_bar() {
    // §5「sub は ARRANGE に入れるだけで、1小節まるごと伸ばす」
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 1, "p", "k", "m", 1.0, [3, 4]]];
                 let VOICES = #{ sub: #{ ch: 5, patch: "sub" } };
                 let CHORDS = #{ "1": ["Am", ["A3","C4","E4"], "A2"] };
                 let ARRANGE = #{ "1": ["sub"] };"#;
    let s = load(src);
    let score = tonescript_render::build(&s).unwrap();
    let n = &score["sub"];
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].len, 12, "3/4 の1小節ぶんになっていない");
    // 低音の1オクターブ下
    assert_eq!(n[0].pitch, tonescript_song::note_number("A2").unwrap() - 12);
}

#[test]
fn patterns_do_not_spill_past_a_short_bar() {
    // §5「拍子が短い小節でははみ出したぶんが落ちる。次の小節へは食い込まない」
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 2, "plain", "k", "m", 1.0, [3, 4]]];
                 let VOICES = #{ bass: #{ ch: 2, patch: "acid" } };
                 let CHORDS = #{ "1": ["Am", ["A3","C4","E4"], "A2"],
                                 "2": ["Am", ["A3","C4","E4"], "A2"] };
                 let BASS_PATTERNS = #{ plain: [[0,4,0],[4,4,0],[8,4,0],[12,4,0]] };
                 let ARRANGE = #{ "1": ["bass"], "2": ["bass"] };"#;
    let s = load(src);
    let score = tonescript_render::build(&s).unwrap();
    // 3/4（12目盛り）なので、位置 12 の打点は入らない
    assert_eq!(score["bass"].len(), 6, "1小節3打 x 2小節のはず");
    for n in &score["bass"] {
        assert!(n.pos + n.len <= s.total_steps(), "曲の外へ出た");
    }
}

#[test]
fn transpose_skips_the_drums() {
    // §7「ドラムには掛からない」
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
                 let VOICES = #{ bass: #{ ch: 2, patch: "acid" }, drums: #{ ch: 9 } };
                 let CHORDS = #{ "1": ["Am", ["A3","C4","E4"], "A2"] };
                 let BASS_PATTERNS = #{ p: [[0,4,0]] };
                 let DRUM_KITS = #{ k: #{ kick: [36, [[0,100]]] } };
                 let ARRANGE = #{ "1": ["bass", "kick"] };
                 let TRANSPOSE = #{ "1": 5 };"#;
    let s = load(src);
    let score = tonescript_render::build(&s).unwrap();
    assert_eq!(score["bass"][0].pitch, tonescript_song::note_number("A2").unwrap() + 5);
    assert_eq!(score["drums"][0].pitch, 36, "ドラムまで移調した");
}

#[test]
fn the_drum_note_numbers_are_as_documented() {
    // §6 の表。番号で音が決まることを、番号がそのまま残ることで確かめる
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
                 let VOICES = #{ drums: #{ ch: 9 }, perc: #{ ch: 9 } };
                 let DRUM_KITS = #{ k: #{
                     kick: [36, [[0,100]]], snare: [38, [[4,100]]],
                     closedhat: [42, [[2,60]]], crash: [49, [[8,100]]] } };
                 let ARRANGE = #{ "1": ["kick", "snare", "closedhat", "crash"] };"#;
    let s = load(src);
    let score = tonescript_render::build(&s).unwrap();
    let drums: Vec<i32> = score["drums"].iter().map(|n| n.pitch).collect();
    let perc: Vec<i32> = score["perc"].iter().map(|n| n.pitch).collect();
    assert!(drums.contains(&36) && drums.contains(&38), "{drums:?}");
    // §6「closedhat と crash は perc レーンへ」
    assert!(perc.contains(&42) && perc.contains(&49), "{perc:?}");
}

#[test]
fn automation_positions_are_in_steps_not_bars() {
    // §7「位置だけは通しの目盛りで書く（小節ではない）」
    let src = format!(
        "{MINIMAL}\nlet AUTOMATION = #{{ lead: #{{ gain: [[0, 1.0], [32, 0.0]] }} }};"
    );
    let s = load(&src);
    let c = &s.automation["lead"][&tonescript_song::model::Lane::Gain];
    assert_eq!(c.at(0.0), Some(1.0));
    assert_eq!(c.at(16.0), Some(0.5), "目盛りで直線に繋がっていない");
    assert_eq!(c.at(32.0), Some(0.0));
}

#[test]
fn automation_ranges_are_enforced() {
    // §7 の表
    for (lane, bad) in [("gain", "9.0"), ("pan", "5.0"), ("reverb", "-1.0")] {
        let src = format!(
            "{MINIMAL}\nlet AUTOMATION = #{{ lead: #{{ {lane}: [[0, {bad}]] }} }};"
        );
        let e = load_str(&src).unwrap_err().to_string();
        assert!(e.contains("範囲の外"), "{lane} の {bad} が通った: {e}");
    }
}

#[test]
fn an_absolute_audio_path_is_refused() {
    // §8「絶対パスは読み込みで弾く」
    let src = format!(
        "{MINIMAL}\nlet AUDIO_TRACKS = #{{ main: #{{ path: \"D:\\\\x\\\\y.wav\" }} }};"
    );
    let e = load_str(&src).unwrap_err().to_string();
    assert!(e.contains("絶対パス"), "{e}");
}

#[test]
fn all_46_patch_names_in_the_doc_exist() {
    // §9 の一覧。文書に載せた名前が本当に使えるか
    let names = [
        "supersaw", "hardlead", "brightsaw", "squarelead", "pluck", "stab", "crystal", "brass",
        "piano", "harpsi", "organ", "strings", "choir",
        "acid", "sub", "reese", "fmbass", "donk", "808", "growl", "hardbass", "fingerbass",
        "upright", "rumble", "wobble",
        "bell", "steelpan", "marimba", "kalimba", "gamelan", "santur",
        "koto", "shamisen", "shakuhachi", "sinobue", "erhu",
        "flute", "quena", "bansuri", "ocarina", "whistle", "panflute", "duduk", "didge",
        "sitar", "oud",
    ];
    assert_eq!(names.len(), 46, "文書の一覧が 46 本ではない");
    let have = tonescript_dsp::patch::NAMES;
    for n in names {
        assert!(have.contains(&n), "文書にあるが実装に無い音色: {n}");
    }
    for n in have {
        assert!(names.contains(n), "実装にあるが文書に無い音色: {n}");
    }
}

#[test]
fn the_documented_ring_times_are_right() {
    // §9「gamelan は 1.6 秒、piano は 0.42 秒、808 は 0.70 秒」
    assert_eq!(tonescript_dsp::patch::ring("gamelan"), 1.60);
    assert_eq!(tonescript_dsp::patch::ring("piano"), 0.42);
    assert_eq!(tonescript_dsp::patch::ring("808"), 0.70);
    assert_eq!(tonescript_dsp::patch::ring("supersaw"), 0.0);
}

#[test]
fn the_defaults_table_is_right() {
    // §12 の既定値
    let s = load(r#"let BPM = 120;
                    let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
                    let VOICES = #{ lead: #{ ch: 0 } };"#);
    assert_eq!(s.title, "untitled");
    assert_eq!(s.key, "");
    assert_eq!(s.tempo_curve, "smooth");
    assert_eq!(s.master_gain, 1.0);
    assert_eq!(s.master_lufs, -9.0);
    assert_eq!(s.premix_lufs, -20.0);
    assert_eq!(s.scale_root, 0);
    assert_eq!(s.tempo_map[&1], 120.0, "TEMPO_MAP の既定が {{1: BPM}} でない");
    assert_eq!(s.sidechain, (0.70, 0.003, 0.020, 0.200));
    assert_eq!(s.reverb, (1.9, 4.2));
    // EDIT_PARTS の既定は VOICES の鍵
    assert_eq!(s.edit_parts, vec!["lead"]);
}

#[test]
fn the_steps_helper_matches_python_range() {
    // §2「Python の range と同じ。終わりは含まない」
    let src = format!(
        "{MINIMAL}\nlet out = []; for i in steps(0, 16, 2) {{ out.push([i, 2, 0]); }}\n\
         let BASS_PATTERNS = #{{ x: out }};"
    );
    let s = load(&src);
    let p = &s.bass_patterns["x"];
    assert_eq!(p.len(), 8);
    assert_eq!(p[0].0, 0);
    assert_eq!(p[7].0, 14, "終わりを含んでいる");
}

#[test]
fn the_note_helper_matches_the_doc() {
    // §2「A4 = 69、C4 = 60。# と b が使える」
    assert_eq!(tonescript_song::note_number("A4"), Some(69));
    assert_eq!(tonescript_song::note_number("C4"), Some(60));
    assert_eq!(tonescript_song::note_number("C#4"), Some(61));
    assert_eq!(tonescript_song::note_number("Db4"), Some(61));
}

#[test]
fn the_example_song_follows_its_own_spec() {
    // 同梱の雛形が、この文書どおりに書けていること
    let path = std::path::Path::new("../../songs/example.rhai");
    let path = if path.exists() { path } else { std::path::Path::new("songs/example.rhai") };
    let s = tonescript_song::load_file(path).expect("雛形が読めない");
    let score = tonescript_render::build(&s).expect("雛形の譜面が組めない");
    assert!(!score["lead"].is_empty(), "雛形の旋律が鳴らない");
    assert!(!score["drums"].is_empty(), "雛形のドラムが鳴らない");
}
