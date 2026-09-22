//! バス（グループ）が本当に効いているか。
//!
//! パート → バス → マスター の道が繋がっていること、そしてバスで触ると
//! そこへ来ているパート全部に効くことを確かめる。

use tonescript_render::{arrange, mix_down, render_stems};
use tonescript_song::Song;

fn song(extra: &str) -> Song {
    let src = format!(
        r#"let BPM = 120;
           let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
           let VOICES = #{{
               lead: #{{ ch: 0, patch: "piano" }},
               bass: #{{ ch: 1, patch: "sub" }},
           }};
           let Am = ["Am", ["A3", "C4", "E4"], "A2"];
           let CHORDS = #{{ "1": Am }};
           let BASS_PATTERNS = #{{ p: [[0, 4, 0], [8, 4, 0]] }};
           let MELODY = #{{ "1": bar([[16, "A4"]]) }};
           let ARRANGE = #{{ "1": ["lead", "bass"] }};
           let GAINS = #{{ lead: 1.0, bass: 1.0 }};
           {extra}"#
    );
    tonescript_song::load_str(&src).unwrap_or_else(|e| panic!("読めない: {e}"))
}

/// 書き出して、実効値を返す。
fn render(s: &Song) -> f32 {
    let quiet = |_: &str| {};
    let sc = arrange::build(s).unwrap();
    let mut stems = render_stems(s, &sc, &quiet);
    let out = mix_down(s, &mut stems, &sc, &quiet);
    (out.l.iter().map(|v| (v * v) as f64).sum::<f64>() / out.l.len() as f64).sqrt() as f32
}

fn db(a: f32, b: f32) -> f32 {
    20.0 * (b / a.max(1e-12)).log10()
}

#[test]
fn a_bus_with_no_settings_changes_nothing() {
    // 通すだけなら、通さないのと同じ音でなければならない。
    // ここがずれると、バスへ送った瞬間に音が変わってしまう
    let direct = render(&song(""));
    let routed = render(&song(
        r#"let BUSES = #{ g: #{} };
           let MIX = #{ lead: #{ bus: "g" }, bass: #{ bus: "g" } };"#,
    ));
    let d = db(direct, routed);
    assert!(d.abs() < 0.1, "素通しのバスで音が {d:+.2}dB 変わった");
}

#[test]
fn the_bus_fader_moves_everything_on_it() {
    let one = render(&song(
        r#"let BUSES = #{ g: #{ gain: 1.0 } };
           let MIX = #{ lead: #{ bus: "g" }, bass: #{ bus: "g" } };"#,
    ));
    let half = render(&song(
        r#"let BUSES = #{ g: #{ gain: 0.5 } };
           let MIX = #{ lead: #{ bus: "g" }, bass: #{ bus: "g" } };"#,
    ));
    let d = db(one, half);
    assert!((d + 6.02).abs() < 0.3, "半分にして {d:+.2}dB（-6 のはず）");
}

#[test]
fn only_the_parts_on_the_bus_are_affected() {
    // bass だけバスへ送る。バスを絞っても lead は無傷であること
    let both = render(&song(
        r#"let BUSES = #{ g: #{ gain: 1.0 } };
           let MIX = #{ bass: #{ bus: "g" } };"#,
    ));
    let cut = render(&song(
        r#"let BUSES = #{ g: #{ gain: 0.0 } };
           let MIX = #{ bass: #{ bus: "g" } };"#,
    ));
    assert!(cut > 0.0, "lead まで消えた");
    assert!(cut < both, "バスを絞っても変わらない");
    // lead だけを鳴らしたときと同じ音になるはず
    let lead_only = render(&song(r#"let ARRANGE = #{ "1": ["lead"] };"#));
    let d = db(lead_only, cut);
    assert!(d.abs() < 0.5, "lead が {d:+.2}dB 変わった（無傷のはず）");
}

#[test]
fn the_bus_eq_reaches_every_part_on_it() {
    let flat = render(&song(
        r#"let BUSES = #{ g: #{} };
           let MIX = #{ lead: #{ bus: "g" }, bass: #{ bus: "g" } };"#,
    ));
    let cut = render(&song(
        r#"let BUSES = #{ g: #{ eq: #{ low: -24.0 } } };
           let MIX = #{ lead: #{ bus: "g" }, bass: #{ bus: "g" } };"#,
    ));
    let d = db(flat, cut);
    assert!(d < -2.0, "バスの EQ が効いていない（{d:+.2}dB）");
}

#[test]
fn the_bus_compressor_holds_it_down() {
    let open = render(&song(
        r#"let BUSES = #{ g: #{} };
           let MIX = #{ lead: #{ bus: "g" }, bass: #{ bus: "g" } };"#,
    ));
    let held = render(&song(
        r#"let BUSES = #{ g: #{ comp: #{ threshold: -36, ratio: 12, attack: 1 } } };
           let MIX = #{ lead: #{ bus: "g" }, bass: #{ bus: "g" } };"#,
    ));
    let d = db(open, held);
    assert!(d < -3.0, "バスの押さえ込みが効いていない（{d:+.2}dB）");
}

#[test]
fn a_bus_that_does_not_exist_is_refused_at_load() {
    // 黙ってマスターへ流すと「送ったのに効かない」という分かりにくい形で出る
    let src = r#"let BPM = 120;
                 let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
                 let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
                 let MIX = #{ lead: #{ bus: "nosuch" } };"#;
    let e = match tonescript_song::load_str(src) {
        Ok(_) => panic!("通ってしまった"),
        Err(e) => e.to_string(),
    };
    assert!(e.contains("nosuch"), "理由が分からない: {e}");
}

#[test]
fn a_silly_bus_value_is_refused() {
    for bad in ["gain: 50", "pan: 3", "reverb: 9"] {
        let src = format!(
            r#"let BPM = 120;
               let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
               let VOICES = #{{ lead: #{{ ch: 0, patch: "piano" }} }};
               let BUSES = #{{ g: #{{ {bad} }} }};"#
        );
        assert!(tonescript_song::load_str(&src).is_err(), "通った: {bad}");
    }
}
