//! 編曲。曲ファイルの指定から、実際に鳴らす音符を組み立てる。
//!
//! Python 版の `mp.py build` に当たる。パートごとに音符の列を作る。
//! ここでは音は出さない。何をどこで鳴らすかだけを決める。

use tonescript_song::model::{note_number, Note};
use tonescript_song::Song;
use std::collections::HashMap;

/// パート名 -> 音符の列。
pub type Score = HashMap<String, Vec<Note>>;

/// 打楽器として扱うパート。音程ではなくキットの打点で鳴る。
pub const PERC_PARTS: &[&str] = &["closedhat", "openhat", "shaker", "ride", "crash", "reverse"];

/// 打楽器のうち、drums レーンではなく perc レーンへ入れるもの。
pub fn is_perc(name: &str) -> bool {
    PERC_PARTS.contains(&name)
}

/// テンポの並び。16分1つごとの BPM。
///
/// 節目の「間」は連続で補間する。小節頭で数値を飛ばすと段差になって、
/// 加速ではなく事故に聞こえる。
pub fn tempo_curve(song: &Song) -> Vec<f32> {
    let total = song.total_steps();
    // 節目は小節番号で書かれている。拍子が混ざると小節の位置が
    // 一定でなくなるので、bar_start で目盛りへ直す。
    let mut anchors: Vec<(u32, f32)> = song
        .tempo_map
        .iter()
        .map(|(bar, bpm)| (song.bar_start(*bar), *bpm))
        .collect();
    anchors.sort_by_key(|a| a.0);
    if anchors.is_empty() {
        anchors.push((0, song.bpm));
    }
    if anchors[0].0 != 0 {
        anchors.insert(0, (0, anchors[0].1));
    }
    let smooth = song.tempo_curve == "smooth";
    let mut out = Vec::with_capacity(total as usize);
    for i in 0..total {
        // i を挟む2つの節目を探す
        let mut lo = anchors[0];
        let mut hi = *anchors.last().unwrap();
        for w in anchors.windows(2) {
            if i >= w[0].0 && i < w[1].0 {
                lo = w[0];
                hi = w[1];
                break;
            }
        }
        let bpm = if i >= hi.0 || hi.0 == lo.0 {
            if i >= anchors.last().unwrap().0 { anchors.last().unwrap().1 } else { lo.1 }
        } else {
            let t = (i - lo.0) as f32 / (hi.0 - lo.0) as f32;
            // smooth は両端の傾きが 0 の曲線。そっと動き出してそっと収まる。
            let k = if smooth { t * t * (3.0 - 2.0 * t) } else { t };
            lo.1 + (hi.1 - lo.1) * k
        };
        out.push(bpm.clamp(20.0, 400.0));
    }
    out
}

/// 16分1つごとの「始まる時刻（秒）」。長さは total+1（末尾は曲の終わり）。
pub fn step_times(song: &Song) -> Vec<f64> {
    let bpm = tempo_curve(song);
    let starts = song.bar_starts();
    let mut out = Vec::with_capacity(bpm.len() + 1);
    let mut t = 0.0f64;
    out.push(0.0);
    for (i, b) in bpm.iter().enumerate() {
        // 1目盛りの長さは「1拍ぶんが何目盛りか」で決まる。
        // BPM は拍を数えるものなので、6/8 の曲で BPM 120 と書いたら
        // 8分音符が1分間に120個という意味になる。
        let bar = match starts.binary_search(&(i as u32)) {
            Ok(k) => k as u32 + 1,
            Err(0) => 1,
            Err(k) => k as u32,
        };
        let per_beat = song.meter_at(bar).steps_per_beat() as f64;
        let mut step = 60.0 / *b as f64 / per_beat;
        // ハネ。**音符は動かさず、目盛りの長さを変える。**
        //
        // 2つで1組にして、前を伸ばし後ろを縮める。合計は変えないので、
        // 小節の長さもテンポも変わらない。深さ1で前:後ろ = 2:1（三連符）
        if song.swing > 0.0 {
            let g = song.swing_grid.max(1) as usize;
            let bar_start = starts.get(bar as usize - 1).copied().unwrap_or(0) as usize;
            let into = i.saturating_sub(bar_start) % (g * 2);
            let d = (song.swing.clamp(0.0, 1.0) / 3.0) as f64;
            step *= if into < g { 1.0 + d } else { 1.0 - d };
        }
        t += step;
        out.push(t);
    }
    out
}

/// その小節の移調ぶん。
fn transpose_of(song: &Song, bar: u32) -> i32 {
    song.transpose.get(&bar).copied().unwrap_or(0)
}

/// その小節のベロシティ倍率。
fn accent_of(song: &Song, bar: u32) -> f32 {
    let sec = song.bar_gain().get(&bar).copied().unwrap_or(1.0);
    sec * song.bar_accent.get(&bar).copied().unwrap_or(1.0)
}

#[inline]
fn vel_of(song: &Song, bar: u32, base: f32) -> u8 {
    (base * accent_of(song, bar)).round().clamp(1.0, 127.0) as u8
}

/// 曲から譜面を組み立てる。
pub fn build(song: &Song) -> Result<Score, String> {
    let mut score: Score = HashMap::new();
    let bars = song.bars();
    let bar_bass = song.bar_bass();
    let bar_arp = song.bar_arp();
    let bar_kit = song.bar_kit();

    for bar in 1..=bars {
        let base = song.bar_start(bar);
        let bar_len = song.bar_steps(bar);
        let tr = transpose_of(song, bar);

        // --- 主旋律
        if song.plays(bar, "lead") {
            if let Some(notes) = song.melody.get(&bar) {
                let mut at = base;
                for (mora, len, name) in notes {
                    if name != "rest" {
                        let p = note_number(name)
                            .ok_or_else(|| format!("{bar}小節目: 音名として読めません: {name}"))?;
                        score.entry("lead".into()).or_default().push(Note {
                            pos: at,
                            len: *len,
                            pitch: p + tr,
                            vel: vel_of(song, bar, 108.0),
                            mora: mora.clone().unwrap_or_default(),
                        });
                    }
                    at += len;
                }
            }
        }

        let chord = song.chords.get(&bar);

        // --- ベース
        if song.plays(bar, "bass") {
            if let (Some(c), Some(pat)) =
                (chord, bar_bass.get(&bar).and_then(|k| song.bass_patterns.get(k)))
            {
                for (pos, len, semi) in pat {
                    // 型は 4/4 を前提に書かれていることが多い。拍子が短い
                    // 小節では、はみ出すぶんを落とす（次の小節へ食い込ませない）
                    if *pos >= bar_len {
                        continue;
                    }
                    let len = (*len).min(bar_len - pos);
                    score.entry("bass".into()).or_default().push(Note {
                        pos: base + pos,
                        len,
                        pitch: c.bass + semi + tr,
                        vel: vel_of(song, bar, 100.0),
                        mora: String::new(),
                    });
                }
            }
        }

        // --- サブベース。根音を1オクターブ下げて支える
        if song.plays(bar, "sub") {
            if let Some(c) = chord {
                score.entry("sub".into()).or_default().push(Note {
                    pos: base,
                    len: bar_len,
                    pitch: c.bass - 12 + tr,
                    vel: vel_of(song, bar, 96.0),
                    mora: String::new(),
                });
            }
        }

        // --- リフ
        if song.plays(bar, "arp") {
            if let (Some(c), Some(pat)) =
                (chord, bar_arp.get(&bar).and_then(|k| song.arp_patterns.get(k)))
            {
                for (pos, len, idx, oct) in pat {
                    if c.tones.is_empty() || *pos >= bar_len {
                        continue;
                    }
                    let len = (*len).min(bar_len - pos);
                    let p = c.tones[idx % c.tones.len()] + oct * 12 + tr;
                    score.entry("arp".into()).or_default().push(Note {
                        pos: base + pos,
                        len,
                        pitch: p,
                        vel: vel_of(song, bar, 92.0),
                        mora: String::new(),
                    });
                }
            }
        }

        // --- コード
        if song.plays(bar, "chords") {
            if let Some(c) = chord {
                for (pos, len, v) in &song.chord_pattern {
                    if *pos >= bar_len {
                        continue;
                    }
                    let len = (*len).min(bar_len - pos);
                    for tone in &c.tones {
                        score.entry("chords".into()).or_default().push(Note {
                            pos: base + pos,
                            len,
                            pitch: tone + tr,
                            vel: vel_of(song, bar, *v as f32),
                            mora: String::new(),
                        });
                    }
                }
            }
        }

        // --- ドラム。移調は掛けない
        if let Some(kit) = bar_kit.get(&bar).and_then(|k| song.drum_kits.get(k)) {
            for (part, (note, hits)) in kit {
                if !song.plays(bar, part) {
                    continue;
                }
                let lane = if is_perc(part) { "perc" } else { "drums" };
                for (pos, v) in hits {
                    if *pos >= bar_len {
                        continue;
                    }
                    score.entry(lane.into()).or_default().push(Note {
                        pos: base + pos,
                        len: 1,
                        pitch: *note as i32,
                        vel: vel_of(song, bar, *v as f32),
                        mora: part.clone(),
                    });
                }
            }
        }
        // --- 一発もの
        for (name, (note, hits)) in &song.extra_hits {
            if !song.plays(bar, name) {
                continue;
            }
            let lane = if is_perc(name) { "perc" } else { "drums" };
            for (pos, v) in hits {
                if *pos >= bar_len {
                    continue;
                }
                score.entry(lane.into()).or_default().push(Note {
                    pos: base + pos,
                    len: 1,
                    pitch: *note as i32,
                    vel: vel_of(song, bar, *v as f32),
                    mora: name.clone(),
                });
            }
        }
    }

    for notes in score.values_mut() {
        notes.sort_by_key(|n| (n.pos, n.pitch));
    }
    Ok(score)
}

/// そのセクションでパートに使う音色。
pub fn patch_for(song: &Song, part: &str, bar: u32) -> Option<String> {
    let sec = song.section_of().get(&bar).cloned().unwrap_or_default();
    if let Some(m) = song.section_patch.get(&sec) {
        if let Some(p) = m.get(part) {
            return Some(p.clone());
        }
    }
    if part == "lead" {
        if let Some(p) = song.lead_patch.get(&sec) {
            return Some(p.clone());
        }
    }
    song.voices.get(part).and_then(|v| v.patch.clone())
}

#[cfg(test)]
mod swing_tests {
    use super::*;

    fn song(extra: &str) -> Song {
        let src = format!(
            r#"let BPM = 120;
               let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
               let VOICES = #{{ lead: #{{ ch: 0, patch: "piano" }} }};
               {extra}"#
        );
        tonescript_song::load_str(&src).expect("読めるはず")
    }

    #[test]
    fn straight_stays_even() {
        let t = step_times(&song(""));
        // 120BPM の16分 = 0.125 秒。どれも同じ長さ
        for i in 0..16 {
            let d = t[i + 1] - t[i];
            assert!((d - 0.125).abs() < 1e-9, "{i} 番目が {d} 秒");
        }
    }

    #[test]
    fn swing_stretches_the_first_of_each_pair() {
        let t = step_times(&song("let SWING = 1.0;"));
        // 8分でハネる（既定 SWING_GRID = 2）。
        // 前2目盛りが伸びて、後ろ2目盛りが縮む
        let first = t[2] - t[0];
        let second = t[4] - t[2];
        assert!(first > second, "前 {first} / 後ろ {second}（ハネていない）");
        // 深さ1 で 2:1（三連符）
        let ratio = first / second;
        assert!((ratio - 2.0).abs() < 0.01, "比が {ratio}（2 のはず）");
    }

    #[test]
    fn swing_does_not_change_the_length_of_the_song() {
        // **ここが肝。** 伸ばしたぶんと縮めたぶんが釣り合っていないと、
        // ハネただけで曲が伸び縮みしてテンポが狂う
        let straight = step_times(&song(""));
        for depth in ["0.3", "0.6", "1.0"] {
            let swung = step_times(&song(&format!("let SWING = {depth};")));
            let (a, b) = (*straight.last().unwrap(), *swung.last().unwrap());
            assert!((a - b).abs() < 1e-9, "深さ {depth} で長さが {a} -> {b}");
        }
    }

    #[test]
    fn each_bar_starts_on_time() {
        // 小節の頭がずれると、他のパートと合わなくなる
        let s = song("let SWING = 1.0;");
        let t = step_times(&s);
        let straight = step_times(&song(""));
        for bar in s.bar_starts() {
            let i = bar as usize;
            assert!(
                (t[i] - straight[i]).abs() < 1e-9,
                "{i} 目盛り（小節の頭）が {} -> {}",
                straight[i],
                t[i]
            );
        }
    }

    #[test]
    fn a_coarser_grid_swings_bigger_units() {
        // SWING_GRID = 4 なら4分でハネる。最初の4目盛りが伸びる
        let t = step_times(&song("let SWING = 1.0; let SWING_GRID = 4;"));
        let first = t[4] - t[0];
        let second = t[8] - t[4];
        assert!((first / second - 2.0).abs() < 0.01, "比が {}", first / second);
        // 8分の位置では割れていない（4目盛りの中は均等）
        assert!((t[1] - t[0] - (t[2] - t[1])).abs() < 1e-9, "組の中で長さが違う");
    }

    #[test]
    fn a_silly_swing_is_refused() {
        for bad in ["let SWING = 2.0;", "let SWING = -0.5;", "let SWING_GRID = 99;"] {
            let src = format!(
                r#"let BPM = 120;
                   let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
                   let VOICES = #{{ lead: #{{ ch: 0, patch: "piano" }} }};
                   {bad}"#
            );
            assert!(tonescript_song::load_str(&src).is_err(), "通った: {bad}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonescript_song::load_str;
    use tonescript_song::model::Note;

    const SRC: &str = r#"
        let TITLE = "t"; let BPM = 120;
        let SECTIONS = [["A", 2, "plain", "k", "main", 1.0]];
        let VOICES = #{ lead: #{ch:0, patch:"supersaw"}, bass: #{ch:2, patch:"acid"},
                        arp: #{ch:3, patch:"pluck"}, chords: #{ch:1, patch:"stab"},
                        sub: #{ch:5, patch:"sub"}, drums: #{ch:9}, perc: #{ch:9} };
        let CHORDS = #{ "1": ["Am", ["A3","C4","E4"], "A2"],
                        "2": ["F",  ["F3","A3","C4"], "F2"] };
        let MELODY = #{ "1": bar([[8,"A4"],[8,"C5"]]), "2": bar([[16,"E5"]]) };
        let ARRANGE = #{ "1": ["lead","bass","arp","chords","sub","kick","closedhat"],
                         "2": ["lead","bass","arp","chords","sub","kick","closedhat"] };
        let BASS_PATTERNS = #{ plain: [[0,4,0],[4,4,0],[8,4,0],[12,4,0]] };
        let ARP_PATTERNS = #{ main: [[0,2,0,0],[2,2,1,0],[4,2,2,0],[6,2,0,1]] };
        let CHORD_PATTERN = [[2,1,84],[10,1,80]];
        let DRUM_KITS = #{ k: #{ kick: [36, [[0,100],[8,100]]],
                                 closedhat: [42, [[2,52],[6,52]]] } };
    "#;

    fn song() -> Song {
        load_str(SRC).expect("読めるはず")
    }

    #[test]
    fn melody_becomes_lead_notes() {
        let s = song();
        let sc = build(&s).unwrap();
        let lead = &sc["lead"];
        assert_eq!(lead.len(), 3, "1小節目2音 + 2小節目1音");
        assert_eq!(lead[0].pos, 0);
        assert_eq!(lead[0].len, 8);
        assert_eq!(lead[0].pitch, note_number("A4").unwrap());
        assert_eq!(lead[1].pos, 8);
        assert_eq!(lead[2].pos, 16, "2小節目は16から");
    }

    #[test]
    fn bass_follows_the_chord_root() {
        let s = song();
        let sc = build(&s).unwrap();
        let bass = &sc["bass"];
        assert_eq!(bass.len(), 8, "1小節4打 x 2小節");
        assert_eq!(bass[0].pitch, note_number("A2").unwrap(), "1小節目は Am");
        assert_eq!(bass[4].pitch, note_number("F2").unwrap(), "2小節目は F");
    }

    #[test]
    fn arp_walks_the_chord_tones() {
        let s = song();
        let sc = build(&s).unwrap();
        let arp = &sc["arp"];
        assert_eq!(arp[0].pitch, note_number("A3").unwrap());
        // 4つめは和音の1番目の音を1オクターブ上げたもの
        let up = arp.iter().find(|n| n.pos == 6).unwrap();
        assert_eq!(up.pitch, note_number("A3").unwrap() + 12);
    }

    #[test]
    fn chords_stack_all_tones() {
        let s = song();
        let sc = build(&s).unwrap();
        // 1小節に2打点 x 3音 = 6、2小節で12
        assert_eq!(sc["chords"].len(), 12);
    }

    #[test]
    fn drums_go_to_the_right_lane() {
        let s = song();
        let sc = build(&s).unwrap();
        // kick は drums、closedhat は perc
        assert_eq!(sc["drums"].len(), 4, "キック 2打 x 2小節");
        assert_eq!(sc["perc"].len(), 4, "ハット 2打 x 2小節");
        assert!(sc["drums"].iter().all(|n| n.pitch == 36));
        assert!(sc["perc"].iter().all(|n| n.pitch == 42));
    }

    #[test]
    fn parts_not_in_arrange_are_silent() {
        let src = SRC.replace(r#""2": ["lead","bass","arp","chords","sub","kick","closedhat"]"#,
                              r#""2": ["lead"]"#);
        let s = load_str(&src).unwrap();
        let sc = build(&s).unwrap();
        assert_eq!(sc["bass"].len(), 4, "2小節目のベースは鳴らない");
        assert_eq!(sc["lead"].len(), 3);
    }

    #[test]
    fn transpose_lifts_pitched_parts_only() {
        let src = format!("{SRC}\nlet TRANSPOSE = #{{ \"2\": 2 }};");
        let s = load_str(&src).unwrap();
        let sc = build(&s).unwrap();
        let bar2_bass = sc["bass"].iter().find(|n| n.pos == 16).unwrap();
        assert_eq!(bar2_bass.pitch, note_number("F2").unwrap() + 2);
        let bar2_kick = sc["drums"].iter().find(|n| n.pos == 16).unwrap();
        assert_eq!(bar2_kick.pitch, 36, "ドラムは移調しない");
    }

    #[test]
    fn tempo_is_constant_when_there_is_one_anchor() {
        let s = song();
        let c = tempo_curve(&s);
        assert_eq!(c.len(), 32, "2小節 x 16");
        assert!(c.iter().all(|b| (b - 120.0).abs() < 1e-3));
        let t = step_times(&s);
        // 120BPM の16分は 0.125 秒
        assert!((t[1] - 0.125).abs() < 1e-9, "{}", t[1]);
        assert!((t[32] - 4.0).abs() < 1e-9, "2小節 = 4秒: {}", t[32]);
    }

    #[test]
    fn tempo_ramps_smoothly() {
        let src = format!("{SRC}\nlet TEMPO_MAP = #{{ \"1\": 120, \"2\": 160 }};");
        let s = load_str(&src).unwrap();
        let c = tempo_curve(&s);
        assert!((c[0] - 120.0).abs() < 1e-3, "頭は 120");
        assert!((c[16] - 160.0).abs() < 1e-3, "2小節目頭は 160: {}", c[16]);
        // 途中は単調に上がる
        for i in 0..16 {
            assert!(c[i + 1] >= c[i] - 1e-4, "段差がある @{i}");
        }
        // smooth なので、真ん中が直線より急（両端が緩い）
        assert!(c[8] > 138.0 && c[8] < 142.0, "真ん中 {}", c[8]);
    }

    // ---------------------------------------------------------- 拍子

    const ODD: &str = r#"
        let TITLE = "t"; let BPM = 120;
        let SECTIONS = [
            ["A", 2, "plain", "k", "main", 1.0],            // 4/4 = 16目盛り
            ["B", 2, "plain", "k", "main", 1.0, [3, 4]],    // 3/4 = 12目盛り
            ["C", 1, "plain", "k", "main", 1.0, [7, 8]],    // 7/8 = 14目盛り
        ];
        let VOICES = #{ lead: #{ch:0, patch:"supersaw"}, bass: #{ch:2, patch:"acid"},
                        sub: #{ch:5, patch:"sub"}, drums: #{ch:9}, perc: #{ch:9} };
        let CHORDS = #{ "1": ["Am", ["A3","C4","E4"], "A2"],
                        "2": ["Am", ["A3","C4","E4"], "A2"],
                        "3": ["F",  ["F3","A3","C4"], "F2"],
                        "4": ["F",  ["F3","A3","C4"], "F2"],
                        "5": ["G",  ["G3","B3","D4"], "G2"] };
        let MELODY = #{
            "1": [[16,"A4"]],
            "3": [[12,"C5"]],          // 3/4 なので 12
            "5": [[14,"E5"]],          // 7/8 なので 14
        };
        let ARRANGE = #{ "1": ["lead","bass","sub","kick"], "2": ["lead","bass","sub","kick"],
                         "3": ["lead","bass","sub","kick"], "4": ["lead","bass","sub","kick"],
                         "5": ["lead","bass","sub","kick"] };
        let BASS_PATTERNS = #{ plain: [[0,4,0],[4,4,0],[8,4,0],[12,4,0]] };
        let DRUM_KITS = #{ k: #{ kick: [36, [[0,100],[4,100],[8,100],[12,100]]] } };
    "#;

    #[test]
    fn odd_meters_place_bars_correctly() {
        let s = load_str(ODD).expect("読めるはず");
        assert_eq!(s.bar_starts(), vec![0, 16, 32, 44, 56, 70]);
        let sc = build(&s).unwrap();
        // 旋律は各小節の頭から
        let lead = &sc["lead"];
        assert_eq!(lead[0].pos, 0);
        assert_eq!(lead[0].len, 16);
        assert_eq!(lead[1].pos, 32, "3小節目は 32 から");
        assert_eq!(lead[1].len, 12);
        assert_eq!(lead[2].pos, 56, "5小節目は 56 から");
        assert_eq!(lead[2].len, 14);
    }

    #[test]
    fn sub_fills_exactly_one_bar_of_any_meter() {
        let s = load_str(ODD).unwrap();
        let sc = build(&s).unwrap();
        let sub = &sc["sub"];
        assert_eq!(sub[0].len, 16, "4/4 の小節");
        assert_eq!(sub[2].len, 12, "3/4 の小節");
        assert_eq!(sub[4].len, 14, "7/8 の小節");
        // 隙間も重なりも無いこと
        for w in sub.windows(2) {
            assert_eq!(w[0].pos + w[0].len, w[1].pos, "小節が繋がっていない");
        }
    }

    #[test]
    fn patterns_do_not_spill_into_the_next_bar() {
        // ベースの型は 4/4 前提（0,4,8,12）。3/4 の小節では 12 が入らない。
        let s = load_str(ODD).unwrap();
        let sc = build(&s).unwrap();
        let bass = &sc["bass"];
        let bar3 = song_bar_notes(&s, bass, 3);
        assert_eq!(bar3.len(), 3, "3/4 に4打は入らない: {:?}", bar3);
        for n in &bar3 {
            assert!(n.pos + n.len <= s.bar_start(3) + 12, "次の小節へ食い込んだ");
        }
        let bar5 = song_bar_notes(&s, bass, 5);
        assert_eq!(bar5.len(), 4, "7/8(14目盛り) なら 12 も入る");
        // 最後の打点は小節の端で切られる
        let last = bar5.last().unwrap();
        assert_eq!(last.pos + last.len, s.bar_start(5) + 14);
    }

    fn song_bar_notes(s: &Song, notes: &[Note], bar: u32) -> Vec<Note> {
        let from = s.bar_start(bar);
        let to = from + s.bar_steps(bar);
        notes.iter().filter(|n| n.pos >= from && n.pos < to).cloned().collect()
    }

    #[test]
    fn eight_meter_counts_eighths_as_the_beat() {
        // 7/8 で BPM 120 なら、8分音符が 1 分間に 120 個。
        // 1目盛り（16分）は 0.25 秒、1小節（14目盛り）は 3.5 秒。
        let s = load_str(ODD).unwrap();
        let t = step_times(&s);
        let b5 = s.bar_start(5) as usize;
        let one = t[b5 + 1] - t[b5];
        assert!((one - 0.25).abs() < 1e-6, "7/8 の1目盛りが {one} 秒");
        let bar = t[b5 + 14] - t[b5];
        assert!((bar - 3.5).abs() < 1e-6, "7/8 の1小節が {bar} 秒");
        // 4/4 の小節は 4分が1拍なので 1目盛り 0.125 秒
        assert!((t[1] - 0.125).abs() < 1e-9);
    }

    #[test]
    fn melody_sum_is_checked_against_the_meter() {
        // 3/4 の小節に 16 を書いたら止まる
        let bad = ODD.replace(r#""3": [[12,"C5"]],"#, r#""3": [[16,"C5"]],"#);
        let e = load_str(&bad).unwrap_err().to_string();
        assert!(e.contains("16/12") && e.contains("3/4"), "{e}");
    }

    #[test]
    fn bad_meter_is_reported() {
        let bad = ODD.replace("[3, 4]", "[3, 5]");
        let e = load_str(&bad).unwrap_err().to_string();
        assert!(e.contains("分母"), "{e}");
    }

    #[test]
    fn bad_note_name_in_melody_is_reported() {
        let src = SRC.replace(r#"bar([[16,"E5"]])"#, r#"[[16,"むり"]]"#);
        let s = load_str(&src).unwrap();
        let e = build(&s).unwrap_err();
        assert!(e.contains("読めません"), "{e}");
    }

    #[test]
    fn section_patch_overrides_the_voice() {
        let src = format!("{SRC}\nlet SECTION_PATCH = #{{ \"A\": #{{ lead: \"koto\" }} }};");
        let s = load_str(&src).unwrap();
        assert_eq!(patch_for(&s, "lead", 1).as_deref(), Some("koto"));
        assert_eq!(patch_for(&s, "bass", 1).as_deref(), Some("acid"), "指定の無いパートはそのまま");
    }
}
