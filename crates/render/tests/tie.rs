//! 小節をまたぐ音。
//!
//! 画面では置ける（音符は目盛りの位置と長さで持っているだけで、小節を
//! 知らない）。曲ファイルの `MELODY` は**1小節の合計がぴったり**でないと
//! 読めないので、そちらでは書けない。
//!
//! ここで確かめるのは「**置いたぶんは鳴るし、書き出しにも出る**」こと。
//! 書けない形式があることと、持てない音があることは別。

use std::collections::HashMap;

use tonescript_dsp::osc::SR;
use tonescript_render::{arrange, mix_down, render_stems, Score};
use tonescript_song::model::Note;
use tonescript_song::Song;

fn song() -> Song {
    tonescript_song::load_str(
        r#"let BPM = 120;
           let SECTIONS = [["A", 4, "none", "none", "none", 1.0]];
           let VOICES = #{ lead: #{ ch: 0, patch: "flute" } };
           let CHORDS = #{ "1": ["Am", ["A3", "C4", "E4"], "A2"] };
           let ARRANGE = #{ "1": ["lead"] };
           let GAINS = #{ lead: 1.0 };
           let MASTER_LUFS = -14.0;"#,
    )
    .expect("読めるはず")
}

fn render(s: &Song, score: Score) -> Vec<f32> {
    let quiet = |_: &str| {};
    let sc = arrange::build(s).unwrap();
    let mut stems = render_stems(s, &score, &quiet);
    let out = mix_down(s, &mut stems, &sc, &quiet);
    out.l
}

/// 鳴っている所の長さ（秒）。
fn sounding_secs(x: &[f32]) -> f32 {
    let peak = x.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    if peak <= 0.0 {
        return 0.0;
    }
    let live = peak * 0.05;
    let from = x.iter().position(|v| v.abs() > live).unwrap_or(0);
    let to = x.iter().rposition(|v| v.abs() > live).unwrap_or(0);
    (to.saturating_sub(from)) as f32 / SR
}

#[test]
fn a_note_may_run_past_the_bar_line() {
    let s = song();
    // 1小節は16目盛り。24目盛り = 1小節半の音を置く
    let mut score: Score = HashMap::new();
    score.insert(
        "lead".into(),
        vec![Note { pos: 0, len: 24, pitch: 69, vel: 100, mora: String::new() }],
    );
    let out = render(&s, score);

    // 120BPM の16分は 0.125 秒。24目盛り = 3.0 秒
    let secs = sounding_secs(&out);
    assert!(
        (secs - 3.0).abs() < 0.4,
        "小節をまたいだ所で切れている（{secs:.2}秒 / 3.0秒のはず）"
    );
}

#[test]
fn it_is_not_cut_at_the_bar_line() {
    // 小節の境目（1.0秒 = 2小節目の頭）で黙っていないこと。
    // ここで切れると「またげているつもりで切れていた」になる
    let s = song();
    let mut score: Score = HashMap::new();
    score.insert(
        "lead".into(),
        vec![Note { pos: 0, len: 24, pitch: 69, vel: 100, mora: String::new() }],
    );
    let out = render(&s, score);
    let at = |t: f32| {
        let i = (t * SR) as usize;
        let w = (0.05 * SR) as usize;
        let seg = &out[i.min(out.len())..(i + w).min(out.len())];
        (seg.iter().map(|v| (v * v) as f64).sum::<f64>() / seg.len().max(1) as f64).sqrt() as f32
    };
    let before = at(1.8);
    let after = at(2.1); // 2小節目の頭をまたいだ直後
    assert!(after > before * 0.5, "境目で落ちた（{before:.4} -> {after:.4}）");
}

#[test]
fn the_song_file_refuses_what_it_cannot_express() {
    // **黙って切り詰めないこと。** 読めないなら読めないと言う。
    // 切り詰めると、開くたびに音が短くなっていく
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 4, "none", "none", "none", 1.0]];
                 let VOICES = #{ lead: #{ ch: 0, patch: "flute" } };
                 let MELODY = #{ "1": bar([[24, "A4"]]) };"#;
    let e = match tonescript_song::load_str(src) {
        Ok(_) => panic!("1小節に収まらない旋律が通ってしまった"),
        Err(e) => e.to_string(),
    };
    assert!(e.contains("24"), "何目盛りだったのかを言うべき: {e}");
    assert!(e.contains("16"), "何目盛りにすべきかを言うべき: {e}");
}
