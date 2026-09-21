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

// ---------------------------------------------------------------- 自分で作る音色

/// §10 に載せた「自分で音色を作る」の例。
///
/// 文書に載せた数がそのまま通り、**本当に音が出る**ことまで確かめる。
/// 読めるだけで無音なら、載せた意味がない。
const PATCHES_DOC: &str = r#"
let BPM = 120;
let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
let PATCHES = #{
    // いちばん短い書き方。これだけで鳴る
    plain: #{},

    // 分厚いのこぎり。少しずらして重ねる
    fatsaw: #{
        osc: [
            #{ wave: "saw", mix: 1.0, detune: -14 },
            #{ wave: "saw", mix: 1.0, detune: 0 },
            #{ wave: "saw", mix: 1.0, detune: 14 },
            #{ wave: "saw", mix: 0.6, octave: -1 },
        ],
        env: #{ a: 0.008, d: 0.20, s: 0.75, r: 0.12 },
        filter: #{ kind: "ladder", base: 700, sweep: 7000, res: 0.35, vel: 1500,
                   env: #{ a: 0.004, d: 0.25, curve: 2.2 } },
        drive: 1.6,
        gain: 0.55,
    },

    // ガラスの鈴。整数倍でない倍音を足す
    glassbell: #{
        osc: [],
        partials: [[1.0, 0.5, 2.4], [2.76, 0.28, 1.6], [5.40, 0.16, 1.0]],
        env: #{ a: 0.001, d: 2.5, s: 0.0, r: 0.6 },
        gain: 1.2,
        ring: 1.8,
    },

    // 撥いた弦。頭に雑音を混ぜる
    pickedstring: #{
        osc: [#{ wave: "saw", mix: 0.6 }, #{ wave: "square", mix: 0.4 }],
        env: #{ a: 0.001, d: 0.45, s: 0.0, r: 0.10 },
        filter: #{ base: 1400, sweep: 6000, res: 0.30 },
        attack: #{ amount: 0.22, hp: 2500, a: 0.0003, d: 0.006 },
        ring: 0.18,
    },

    // 金属質な FM
    fmbell: #{
        osc: [#{ wave: "sine" }],
        fm: #{ ratio: 3.5, index: 6.0, decay: 0.4 },
        env: #{ a: 0.001, d: 1.2, s: 0.0, r: 0.3 },
        gain: 0.8,
    },

    // 息もの。雑音を帯で削って笛にする
    airy: #{
        osc: [#{ wave: "noise", mix: 1.0 }],
        filter: #{ kind: "bandpass", base: 1200, res: 0.6 },
        env: #{ a: 0.08, d: 0.2, s: 0.8, r: 0.15 },
        vibrato: #{ rate: 5.2, depth: 0.008, delay: 0.3 },
        gain: 1.5,
    },
};
let VOICES = #{ lead: #{ ch: 0, patch: "fatsaw" } };
let MELODY = #{ "1": bar([[16, "A4"]]) };
let ARRANGE = #{ "1": ["lead"] };
"#;

#[test]
fn the_patch_examples_all_load() {
    let s = load(PATCHES_DOC);
    for name in ["plain", "fatsaw", "glassbell", "pickedstring", "fmbell", "airy"] {
        assert!(s.patches.contains_key(name), "{name} が読めていない");
    }
    // 書いた数がそのまま入っていること
    let f = &s.patches["fatsaw"];
    assert_eq!(f.osc.len(), 4);
    assert_eq!(f.osc[0].detune, -14.0);
    assert_eq!(f.osc[3].octave, -1);
    assert!((f.drive - 1.6).abs() < 1e-6);
    assert_eq!(f.filter.kind, tonescript_dsp::recipe::FilterKind::Ladder);
    assert!((f.filter.vel - 1500.0).abs() < 1e-6);
    // kind を書かなくても、他を書けばフィルタは掛かる
    assert_eq!(
        s.patches["pickedstring"].filter.kind,
        tonescript_dsp::recipe::FilterKind::Ladder,
        "kind 無しでフィルタが掛からない"
    );
    // 何も書かない音色も既定値で成立する
    assert_eq!(s.patches["plain"].osc.len(), 1);
    assert!((s.patches["glassbell"].ring - 1.8).abs() < 1e-6);
}

#[test]
fn every_patch_example_actually_makes_a_sound() {
    let s = load(PATCHES_DOC);
    for (name, r) in &s.patches {
        let w = tonescript_dsp::recipe::render(r, 440.0, 48_000, 1.0, 1);
        let rms = (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt();
        assert!(rms > 0.003, "{name} が無音（実効 {rms}）");
        assert!(w.iter().all(|v| v.is_finite()), "{name} に数でない値が出た");
        let peak = w.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak < 8.0, "{name} が大きすぎる（ピーク {peak}）");
    }
}

#[test]
fn the_instrument_list_in_the_doc_matches_the_code() {
    // §9 の表と実装がずれたら、渡した相手が「無い音色」を書いてしまう。
    // **文書に並んでいる名前が、全部そのまま使えること。**
    let doc = std::fs::read_to_string("../../SONGFILE.md").expect("SONGFILE.md が読めない");
    let head = doc
        .split("## 9.")
        .nth(1)
        .and_then(|s| s.split("## 10.").next())
        .expect("§9 が見つからない");
    let mut listed: Vec<String> = Vec::new();
    for line in head.lines().filter(|l| l.starts_with('|')) {
        for m in line.split('`').skip(1).step_by(2) {
            if m.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
                listed.push(m.to_string());
            }
        }
    }
    assert!(listed.len() > 100, "拾えた名前が {} 個しかない", listed.len());
    let all = tonescript_dsp::patch::all_names();
    for n in &listed {
        assert!(all.contains(&n.as_str()), "文書にある {n} が実装に無い");
    }
    // 逆も。実装にあるのに文書に無いと、誰にも見つけてもらえない
    for n in &all {
        assert!(listed.iter().any(|l| l == n), "実装の {n} が文書に無い");
    }
    assert_eq!(listed.len(), all.len(), "数が合わない");
}

#[test]
fn the_ordinary_instruments_are_usable_by_name() {
    // 名前を書くだけで鳴ること
    for name in tonescript_dsp::kit::NAMES {
        let src = format!(
            r#"let BPM = 120;
               let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
               let VOICES = #{{ lead: #{{ ch: 0, patch: "{name}" }} }};
               let MELODY = #{{ "1": bar([[16, "A4"]]) }};
               let ARRANGE = #{{ "1": ["lead"] }};"#
        );
        let s = load(&src);
        assert_eq!(s.voices["lead"].patch.as_deref(), Some(*name));
        // 名前が引けて、本当に音になること
        let w = tonescript_dsp::patch::render(
            name,
            &tonescript_dsp::patch::Cfg::default(),
            261.63,
            24_000,
            1.0,
            3,
        )
        .unwrap_or_else(|| panic!("{name} が引けない"));
        let rms = (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt();
        assert!(rms > 0.002, "{name} が無音（実効 {rms}）");
    }
}

#[test]
fn the_eq_example_loads_and_shapes() {
    // §8 に載せた EQ の例。読めて、値がそのまま入ること
    let s = load(
        r#"let BPM = 120;
           let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
           let VOICES = #{ bass: #{ ch: 0, patch: "sub" } };
           let MIX = #{
               bass:  #{ eq: #{ low: 2.0, mid: -3.0 } },
               drums: #{ eq: #{ low: -4.0, high: 3.0 } },
           };"#,
    );
    assert_eq!(s.mix["bass"].eq.low, 2.0);
    assert_eq!(s.mix["bass"].eq.mid, -3.0);
    assert_eq!(s.mix["drums"].eq.high, 3.0);
    // 書いていない所は素通し
    assert_eq!(s.mix["bass"].eq.high, 0.0);
    assert!(!s.mix["bass"].eq.is_flat());
    assert!(s.mix.get("nosuch").is_none());
}

#[test]
fn a_silly_eq_is_refused_with_a_reason() {
    // 暴れる値は読み込みで止める
    for bad in ["low: 40.0", "mid_q: 100.0", "mid_hz: 30000.0"] {
        let src = format!(
            r#"let BPM = 120;
               let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
               let VOICES = #{{ lead: #{{ ch: 0, patch: "piano" }} }};
               let MIX = #{{ lead: #{{ eq: #{{ {bad} }} }} }};"#
        );
        let e = match load_str(&src) {
            Ok(_) => panic!("通ってしまった: {bad}"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains("eq"), "理由が分からない: {bad} -> {e}");
    }
}

#[test]
fn a_song_patch_beats_the_built_in_one() {
    // 同じ名前なら曲ファイル側が勝つ。内蔵の音色を作り替えられる
    let src = r#"
        let BPM = 120;
        let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
        let PATCHES = #{ piano: #{ osc: [#{ wave: "square" }], gain: 0.5 } };
        let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
        let MELODY = #{ "1": bar([[16, "A4"]]) };
        let ARRANGE = #{ "1": ["lead"] };
    "#;
    let s = load(src);
    assert!(s.patches.contains_key("piano"), "内蔵の名前で上書きできない");
    assert_eq!(s.patches["piano"].osc[0].wave, tonescript_dsp::recipe::Wave::Square);
}

#[test]
fn a_broken_patch_is_refused_with_a_reason() {
    let cases = [
        // 音の素が無い
        (r#"let PATCHES = #{ dead: #{ osc: [] } };"#, "素"),
        // 知らない波の形
        (r#"let PATCHES = #{ x: #{ osc: [#{ wave: "triangle" }] } };"#, "wave"),
        // 知らないフィルタ
        (r#"let PATCHES = #{ x: #{ filter: #{ kind: "notch" } } };"#, "kind"),
        // 響きが高すぎる（発振する）
        (r#"let PATCHES = #{ x: #{ filter: #{ kind: "ladder", res: 5.0 } } };"#, "res"),
        // 倍音の書き方が足りない
        (r#"let PATCHES = #{ x: #{ partials: [[1.0, 0.5]] } };"#, "partials"),
    ];
    let head = r#"
        let BPM = 120;
        let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
        let VOICES = #{ lead: #{ ch: 0, patch: "x" } };
    "#;
    for (bad, want) in cases {
        let src = format!("{head}\n{bad}");
        let got = load_str(&src);
        let e = match got {
            Ok(_) => panic!("通ってしまった: {bad}"),
            Err(e) => e.to_string(),
        };
        assert!(
            e.contains(want) || e.contains("PATCHES"),
            "理由が分からない: {bad}\n -> {e}"
        );
    }
}
