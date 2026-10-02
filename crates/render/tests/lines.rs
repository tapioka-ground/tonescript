//! 追加の旋律 `LINES`（ハモリ・対旋律）。
//!
//! `MELODY` は `lead` の1本だけだった。ハモリを重ねたい曲は、曲ファイルに
//! 書く手が無く、画面で音符を描くしか無かった。
//!
//! ここで確かめるのは次の3つ。
//!   - 書いたとおりの位置と音程で譜面に出る（`TRANSPOSE` も掛かる）
//!   - `ARRANGE` に居ない小節では鳴らない
//!   - 間違いは読み込みで止まる（鳴らない理由が分からなくなるのが一番困る）

use tonescript_render::arrange;
use tonescript_song::Song;

fn base(extra: &str) -> String {
    format!(
        r#"let BPM = 120;
           let SECTIONS = [["A", 4, "p", "k", "m", 1.0]];
           let VOICES = #{{
               lead: #{{ ch: 0, patch: "aah" }},
               harm: #{{ ch: 1, patch: "aah" }},
           }};
           let CHORDS = #{{ "1": ["Am", ["A3", "C4", "E4"], "A2"] }};
           let MELODY = #{{ "1": bar([[16, "A4"]]), "2": bar([[16, "C5"]]) }};
           let ARRANGE = #{{ "1": ["lead", "harm"], "2": ["lead"], "3": ["lead", "harm"] }};
           {extra}"#
    )
}

fn load(extra: &str) -> Result<Song, String> {
    tonescript_song::load_str(&base(extra)).map_err(|e| e.to_string())
}

#[test]
fn a_second_line_comes_out_where_it_was_written() {
    let s = load(r#"let LINES = #{ harm: #{ "1": bar([[8, "F#4"], [8, "G4"]]) } };"#).unwrap();
    let sc = arrange::build(&s).unwrap();
    let h = &sc["harm"];
    assert_eq!(h.len(), 2, "音符の数が違う");
    // 1小節目の頭から。長さは書いたとおり
    assert_eq!((h[0].pos, h[0].len), (0, 8));
    assert_eq!((h[1].pos, h[1].len), (8, 8));
    // 音程は音名のとおり（F#4 = 66, G4 = 67）
    assert_eq!((h[0].pitch, h[1].pitch), (66, 67));
    // lead は影響を受けない
    assert_eq!(sc["lead"].len(), 2);
}

#[test]
fn the_second_line_follows_the_key_change() {
    // 全部を 3 半音上げる。lead と同じだけ動かないと、転調した所で
    // ハモリだけ元のキーに取り残される
    let s = load(
        r#"let TRANSPOSE = #{ "1": 3 };
           let LINES = #{ harm: #{ "1": bar([[16, "F#4"]]) } };"#,
    )
    .unwrap();
    let sc = arrange::build(&s).unwrap();
    assert_eq!(sc["harm"][0].pitch, 66 + 3, "ハモリが転調に付いてこない");
    assert_eq!(sc["lead"][0].pitch, 69 + 3);
}

#[test]
fn it_is_silent_in_bars_where_arrange_leaves_it_out() {
    // 2小節目は ARRANGE に harm が居ない
    let s = load(
        r#"let LINES = #{ harm: #{ "1": bar([[16, "F#4"]]),
                                  "2": bar([[16, "A4"]]) } };"#,
    )
    .unwrap();
    let sc = arrange::build(&s).unwrap();
    let h = &sc["harm"];
    assert_eq!(h.len(), 1, "居ない小節で鳴っている");
    assert_eq!(h[0].pos, 0);
}

#[test]
fn a_rest_leaves_a_gap() {
    let s = load(r#"let LINES = #{ harm: #{ "1": bar([[8, "rest"], [8, "E4"]]) } };"#).unwrap();
    let sc = arrange::build(&s).unwrap();
    let h = &sc["harm"];
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].pos, 8, "休符のぶん後ろへずれていない");
}

#[test]
fn the_second_line_sits_below_the_lead() {
    // 同じ強さで鳴らすと、主役が入れ替わって聞こえる
    let s = load(r#"let LINES = #{ harm: #{ "1": bar([[16, "F#4"]]) } };"#).unwrap();
    let sc = arrange::build(&s).unwrap();
    assert!(sc["harm"][0].vel < sc["lead"][0].vel, "ハモリが主旋律より強い");
}

// ---------------------------------------------------------------- 間違い

#[test]
fn a_wrong_sum_is_refused_and_says_which_line() {
    let e = load(r#"let LINES = #{ harm: #{ "1": bar([[8, "F#4"]]) } };"#)
        .expect_err("1小節に足りないのに通った");
    assert!(e.contains("LINES.harm"), "どの欄か分からない: {e}");
    assert!(e.contains("1小節目"), "どの小節か分からない: {e}");
    assert!(e.contains("8/16"), "何が足りないか分からない: {e}");
}

#[test]
fn a_bar_past_the_end_is_refused() {
    let e = load(r#"let LINES = #{ harm: #{ "9": bar([[16, "F#4"]]) } };"#)
        .expect_err("曲の外なのに通った");
    assert!(e.contains("LINES.harm"), "{e}");
}

#[test]
fn a_part_with_no_voice_is_refused() {
    // 音色が無いと鳴らない。鳴らない理由が分からなくなるので、読み込みで止める
    let e = load(r#"let LINES = #{ ghost: #{ "1": bar([[16, "F#4"]]) } };"#)
        .expect_err("音色の無いパートが通った");
    assert!(e.contains("ghost"), "{e}");
    assert!(e.contains("VOICES"), "どこへ書けばいいか分からない: {e}");
}

#[test]
fn lead_cannot_be_written_in_lines() {
    // lead の旋律は MELODY。二重に書けると、どちらが鳴るのか分からない
    let e = load(r#"let LINES = #{ lead: #{ "1": bar([[16, "A4"]]) } };"#)
        .expect_err("lead が通った");
    assert!(e.contains("MELODY"), "{e}");
}

#[test]
fn a_note_that_is_not_a_note_is_refused_when_the_score_is_built() {
    let s = load(r#"let LINES = #{ harm: #{ "1": bar([[16, "H9"]]) } };"#).unwrap();
    let e = arrange::build(&s).expect_err("音名でないものが通った");
    assert!(e.contains("harm"), "{e}");
}

#[test]
fn a_song_without_lines_is_unchanged() {
    // 書かない曲に何も起きないこと。既存の曲が壊れるのが一番困る
    let s = load("").unwrap();
    assert!(s.lines.is_empty());
    let sc = arrange::build(&s).unwrap();
    assert!(!sc.contains_key("harm"));
}
