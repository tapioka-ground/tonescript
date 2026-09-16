//! MIDI ファイルの読み書き。
//!
//! MIDI は Tonescript の保存形式ではない。曲は `.rhai`、編集は `.tsp` に
//! ある。MIDI は**外と音符をやり取りするための形式**。
//!
//! - 持ち出す: 別の DAW や譜面ソフトへ渡す
//! - 持ち込む: よそで打ち込んだ音符や、買ってきた MIDI 素材を取り込む
//!
//! MIDI は音を運ばない。音色は番号でしか伝わらないので、
//! 持ち出した先では別の音で鳴る。ここが一番の注意点で、
//! 自作シンセで作った音はそのままでは持ち出せない（WAV で書き出すこと）。

pub mod smf;

use tonescript_song::model::{Note, STEPS_PER_BAR};
use tonescript_song::Song;
use std::collections::HashMap;
use std::path::Path;

pub use smf::{Smf, Track, TPQ};

/// 目盛り（16分）ひとつぶんの tick。
pub const TICKS_PER_STEP: u32 = TPQ as u32 / 4;

/// 譜面（パート名 -> 音符）。`tonescript_render::Score` と同じ形。
pub type Score = HashMap<String, Vec<Note>>;

// ---------------------------------------------------------------- 持ち出す

/// 曲と譜面を MIDI にする。
///
/// テンポと拍子は曲から取る。同じ値が続くところは1つにまとめる
/// （目盛りごとに書くと、1曲で数千のテンポ変化が並んで読む側が困る）。
pub fn to_smf(song: &Song, score: &Score) -> Smf {
    let mut tempos: Vec<(u32, f32)> = Vec::new();
    let curve = tempo_of(song);
    let mut last = f32::NAN;
    for (i, bpm) in curve.iter().enumerate() {
        // 0.05 BPM 未満の差は書かない。滑らかに変わる所で山ほど並ぶのを防ぐ
        if last.is_nan() || (bpm - last).abs() >= 0.05 {
            tempos.push((i as u32 * TICKS_PER_STEP, *bpm));
            last = *bpm;
        }
    }
    if tempos.is_empty() {
        tempos.push((0, song.bpm));
    }

    // 拍子。変わる所だけ
    let mut time_sigs: Vec<(u32, u8, u8)> = Vec::new();
    let mut prev = None;
    for bar in 1..=song.bars() {
        let m = song.meter_at(bar);
        if prev != Some(m) {
            time_sigs.push((
                song.bar_start(bar) * TICKS_PER_STEP,
                m.num.min(255) as u8,
                m.den.min(255) as u8,
            ));
            prev = Some(m);
        }
    }

    let mut names: Vec<&String> = score.keys().collect();
    names.sort();
    let tracks = names
        .into_iter()
        .map(|part| {
            let voice = song.voices.get(part);
            Track {
                name: voice.map(|v| v.label.clone()).unwrap_or_else(|| part.clone()),
                channel: voice.map(|v| v.ch).unwrap_or(0).min(15),
                program: voice.and_then(|v| v.program),
                notes: score[part]
                    .iter()
                    .map(|n| smf::Note {
                        at: n.pos * TICKS_PER_STEP,
                        len: n.len.max(1) * TICKS_PER_STEP,
                        pitch: n.pitch.clamp(0, 127) as u8,
                        vel: n.vel.max(1),
                        lyric: n.mora.clone(),
                    })
                    .collect(),
            }
        })
        .collect();

    Smf { title: song.title.clone(), tempos, time_sigs, tracks }
}

/// 目盛りごとの BPM。テンポが動く曲でも合うように、曲から取る。
fn tempo_of(song: &Song) -> Vec<f32> {
    let total = song.total_steps().max(1);
    let mut anchors: Vec<(u32, f32)> =
        song.tempo_map.iter().map(|(b, v)| (song.bar_start(*b), *v)).collect();
    anchors.sort_by_key(|a| a.0);
    if anchors.is_empty() {
        anchors.push((0, song.bpm));
    }
    if anchors[0].0 != 0 {
        anchors.insert(0, (0, anchors[0].1));
    }
    let smooth = song.tempo_curve == "smooth";
    (0..total)
        .map(|i| {
            let mut lo = anchors[0];
            let mut hi = *anchors.last().unwrap();
            for w in anchors.windows(2) {
                if i >= w[0].0 && i < w[1].0 {
                    lo = w[0];
                    hi = w[1];
                    break;
                }
            }
            if i >= hi.0 || hi.0 == lo.0 {
                return if i >= anchors.last().unwrap().0 {
                    anchors.last().unwrap().1
                } else {
                    lo.1
                };
            }
            let t = (i - lo.0) as f32 / (hi.0 - lo.0) as f32;
            let k = if smooth { t * t * (3.0 - 2.0 * t) } else { t };
            lo.1 + (hi.1 - lo.1) * k
        })
        .collect()
}

/// ファイルへ書く。
pub fn export(song: &Song, score: &Score, path: &Path) -> Result<usize, String> {
    let smf = to_smf(song, score);
    let bytes = smf::write(&smf);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes.len())
}

// ---------------------------------------------------------------- 持ち込む

/// 取り込んだ結果。
#[derive(Debug)]
pub struct Imported {
    pub score: Score,
    /// 拾えなかったトラックの名前と理由
    pub skipped: Vec<String>,
    /// 相手のファイルにあったテンポ（先頭のもの）
    pub bpm: Option<f32>,
    /// 何小節ぶんあったか（4/4 換算）
    pub bars: u32,
}

/// MIDI のトラック名から、どのパートに入れるか決める。
///
/// 名前が合わなければチャンネルで見る。9 は打楽器という決まりがある。
fn part_for(track: &Track, song: &Song, used: &mut Vec<String>) -> Option<String> {
    // 曲の音色の表示名と突き合わせる
    for (part, v) in &song.voices {
        if v.label == track.name || *part == track.name {
            return Some(part.clone());
        }
    }
    // 打楽器のチャンネル
    if track.channel == 9 {
        return Some("drums".into());
    }
    // 同じチャンネルのパート
    for (part, v) in &song.voices {
        if v.ch == track.channel && !used.contains(part) {
            return Some(part.clone());
        }
    }
    // 空いている編集用パートへ
    song.edit_parts.iter().find(|p| !used.contains(p)).cloned()
}

/// MIDI を譜面へ。
///
/// 相手の刻みや拍子が違っても、目盛り（16分）へ丸めて取り込む。
/// 丸めきれない音符（32分など）は、いちばん近い目盛りへ寄せる。
pub fn from_smf(smf: &Smf, song: &Song) -> Imported {
    let mut score: Score = HashMap::new();
    let mut skipped = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut last_end = 0u32;

    for t in &smf.tracks {
        let Some(part) = part_for(t, song, &mut used) else {
            skipped.push(format!("{}: 入れ先のパートがありません", t.name));
            continue;
        };
        used.push(part.clone());
        let notes: Vec<Note> = t
            .notes
            .iter()
            .map(|n| {
                let pos = round_step(n.at);
                let len = round_step(n.len).max(1);
                last_end = last_end.max(pos + len);
                Note {
                    pos,
                    len,
                    pitch: n.pitch as i32,
                    vel: n.vel.max(1),
                    mora: n.lyric.clone(),
                }
            })
            .collect();
        if notes.is_empty() {
            continue;
        }
        score.entry(part).or_default().extend(notes);
    }
    for v in score.values_mut() {
        v.sort_by_key(|n| (n.pos, n.pitch));
    }
    Imported {
        score,
        skipped,
        bpm: smf.tempos.first().map(|t| t.1),
        bars: last_end.div_ceil(STEPS_PER_BAR),
    }
}

/// tick を目盛りへ。いちばん近い所へ寄せる。
fn round_step(tick: u32) -> u32 {
    (tick + TICKS_PER_STEP / 2) / TICKS_PER_STEP
}

/// ファイルから読む。
pub fn import(path: &Path, song: &Song) -> Result<Imported, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let smf = smf::read(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(from_smf(&smf, song))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        tonescript_song::load_str(
            r#"let TITLE = "テスト曲"; let BPM = 128;
               let SECTIONS = [["A", 2, "p", "k", "m", 1.0],
                               ["B", 2, "p", "k", "m", 1.0, [7, 8]]];
               let VOICES = #{
                   lead:  #{ ch: 0, patch: "piano", program: 81, label: "主旋律" },
                   bass:  #{ ch: 2, patch: "acid",  program: 38, label: "ベース" },
                   drums: #{ ch: 9, label: "ドラム" },
                   vocal: #{ ch: 10, label: "歌" },
               };
               let EDIT_PARTS = ["lead", "bass", "drums", "vocal"];
            "#,
        )
        .unwrap()
    }

    fn note(pos: u32, len: u32, pitch: i32, mora: &str) -> Note {
        Note { pos, len, pitch, vel: 100, mora: mora.into() }
    }

    fn score() -> Score {
        let mut s: Score = HashMap::new();
        s.insert("lead".into(), vec![note(0, 4, 69, ""), note(4, 2, 72, ""), note(8, 8, 76, "")]);
        s.insert("bass".into(), vec![note(0, 4, 45, ""), note(4, 4, 45, "")]);
        s.insert("drums".into(), vec![note(0, 1, 36, ""), note(4, 1, 38, "")]);
        s.insert("vocal".into(), vec![note(0, 4, 60, "ら"), note(4, 4, 62, "らー")]);
        s
    }

    #[test]
    fn export_then_import_keeps_the_notes() {
        let sg = song();
        let sc = score();
        let smf = to_smf(&sg, &sc);
        let back = from_smf(&smf::read(&smf::write(&smf)).unwrap(), &sg);

        for part in ["lead", "bass", "drums", "vocal"] {
            let a = &sc[part];
            let b = back.score.get(part).unwrap_or_else(|| panic!("{part} が消えた"));
            assert_eq!(a.len(), b.len(), "{part} の数が違う");
            for (x, y) in a.iter().zip(b) {
                assert_eq!((x.pos, x.len, x.pitch), (y.pos, y.len, y.pitch), "{part}");
            }
        }
        assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    }

    #[test]
    fn lyrics_survive_the_round_trip() {
        let sg = song();
        let smf = to_smf(&sg, &score());
        let back = from_smf(&smf::read(&smf::write(&smf)).unwrap(), &sg);
        let v = &back.score["vocal"];
        assert_eq!(v[0].mora, "ら");
        assert_eq!(v[1].mora, "らー");
    }

    #[test]
    fn tempo_is_not_written_once_per_step() {
        // 目盛りごとに書くと、1曲で何千も並んで読む側が困る
        let src = r#"let TITLE = "t"; let BPM = 120;
                     let TEMPO_MAP = #{ "1": 120, "3": 180 };
                     let SECTIONS = [["A", 4, "p", "k", "m", 1.0]];
                     let VOICES = #{ lead: #{ ch: 0 } };"#;
        let sg = tonescript_song::load_str(src).unwrap();
        let smf = to_smf(&sg, &HashMap::new());
        // 64 目盛りあるが、テンポは変わる所だけ
        assert!(smf.tempos.len() < 40, "{} 個も書いている", smf.tempos.len());
        assert!(smf.tempos.len() > 1, "テンポの変化が消えた");
        assert!((smf.tempos[0].1 - 120.0).abs() < 0.5);
        assert!((smf.tempos.last().unwrap().1 - 180.0).abs() < 1.0);
    }

    #[test]
    fn time_signature_changes_are_written() {
        let smf = to_smf(&song(), &HashMap::new());
        assert_eq!(smf.time_sigs.len(), 2, "{:?}", smf.time_sigs);
        assert_eq!(smf.time_sigs[0], (0, 4, 4));
        // 3小節目から 7/8。4/4 が2小節 = 32目盛り、1目盛り 120 tick なので 3840
        assert_eq!(smf.time_sigs[1], (32 * TICKS_PER_STEP, 7, 8));
        assert_eq!(TICKS_PER_STEP, 120);
    }

    #[test]
    fn a_constant_tempo_writes_one_event() {
        let src = r#"let TITLE = "t"; let BPM = 128;
                     let SECTIONS = [["A", 4, "p", "k", "m", 1.0]];
                     let VOICES = #{ lead: #{ ch: 0 } };"#;
        let sg = tonescript_song::load_str(src).unwrap();
        let smf = to_smf(&sg, &HashMap::new());
        assert_eq!(smf.tempos.len(), 1, "{:?}", smf.tempos);
    }

    #[test]
    fn imported_tracks_land_on_the_right_parts() {
        let sg = song();
        let smf = Smf {
            title: "よそで作った曲".into(),
            tempos: vec![(0, 140.0)],
            time_sigs: vec![],
            tracks: vec![
                // 名前で当てる
                Track { name: "ベース".into(), channel: 5, program: None,
                        notes: vec![smf::Note { at: 0, len: 480, pitch: 40, vel: 90,
                                                lyric: String::new() }] },
                // チャンネル 9 は打楽器
                Track { name: "なにか".into(), channel: 9, program: None,
                        notes: vec![smf::Note { at: 0, len: 120, pitch: 36, vel: 100,
                                                lyric: String::new() }] },
            ],
        };
        let got = from_smf(&smf, &sg);
        assert!(got.score.contains_key("bass"), "名前で当てられていない");
        assert!(got.score.contains_key("drums"), "打楽器のチャンネルを見ていない");
        assert_eq!(got.bpm, Some(140.0));
    }

    #[test]
    fn odd_timing_is_snapped_to_the_grid() {
        let sg = song();
        let smf = Smf {
            title: String::new(),
            tempos: vec![(0, 120.0)],
            time_sigs: vec![],
            tracks: vec![Track {
                name: "主旋律".into(),
                channel: 0,
                program: None,
                notes: vec![
                    // 16分（120 tick）から少しずれた所
                    smf::Note { at: 118, len: 122, pitch: 60, vel: 100, lyric: String::new() },
                    // 32分（60 tick）。いちばん近い目盛りへ寄る
                    smf::Note { at: 300, len: 60, pitch: 62, vel: 100, lyric: String::new() },
                ],
            }],
        };
        let got = from_smf(&smf, &sg);
        let n = &got.score["lead"];
        assert_eq!(n[0].pos, 1, "ずれが直っていない");
        assert_eq!(n[0].len, 1);
        assert_eq!(n[1].pos, 3, "300/120 = 2.5 -> 3");
        assert_eq!(n[1].len, 1, "長さ 0 にしない");
    }

    #[test]
    fn a_track_that_has_nowhere_to_go_is_reported() {
        let src = r#"let TITLE = "t"; let BPM = 120;
                     let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
                     let VOICES = #{ lead: #{ ch: 0 } };
                     let EDIT_PARTS = ["lead"];"#;
        let sg = tonescript_song::load_str(src).unwrap();
        let smf = Smf {
            title: String::new(),
            tempos: vec![(0, 120.0)],
            time_sigs: vec![],
            tracks: (0..3)
                .map(|i| Track {
                    name: format!("t{i}"),
                    channel: i as u8 + 1,
                    program: None,
                    notes: vec![smf::Note { at: 0, len: 480, pitch: 60, vel: 100,
                                            lyric: String::new() }],
                })
                .collect(),
        };
        let got = from_smf(&smf, &sg);
        assert_eq!(got.score.len(), 1, "入れ先が1つしか無いのに詰め込んだ");
        assert_eq!(got.skipped.len(), 2, "黙って捨てている: {:?}", got.skipped);
    }

    #[test]
    fn files_round_trip_on_disk() {
        let mut p = std::env::temp_dir();
        p.push(format!("mp_midi_{}.mid", std::process::id()));
        let sg = song();
        let sc = score();
        let n = export(&sg, &sc, &p).expect("書けるはず");
        assert!(n > 100, "小さすぎる: {n} バイト");
        let got = import(&p, &sg).expect("読めるはず");
        assert_eq!(got.score["lead"].len(), 3);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn importing_something_that_is_not_midi_says_why() {
        let mut p = std::env::temp_dir();
        p.push(format!("mp_notmidi_{}.mid", std::process::id()));
        std::fs::write(&p, "これは MIDI ではない").unwrap();
        let e = import(&p, &song()).unwrap_err();
        assert!(e.contains("MIDI"), "{e}");
        std::fs::remove_file(&p).ok();
    }
}
