//! 曲を音にする。
//!
//! 流れは Python 版と同じ。
//!
//!   曲ファイル -> 譜面（arrange）-> パートごとの音（synth）
//!             -> ミックス（mix）-> マスター（master）-> WAV
//!
//! Python 版と決定的に違うのは、ノート合成を並列にしていること。
//! 9,000 本のノートは互いに独立なので、そのまま全コアへ配れる。
//! Python には GIL があってここが取れなかった。

pub mod arrange;
pub mod export;
pub mod mix;
pub mod wav;

use tonescript_dsp::drum;
use tonescript_dsp::osc::SR;
use tonescript_dsp::patch::{self, Cfg};
use tonescript_song::model::{freq_of, Lane, Note};
use tonescript_song::Song;
use rayon::prelude::*;
use std::collections::HashMap;

pub use arrange::{build, Score};
pub use mix::Stereo;

/// パートごとに鳴らした結果。
pub type Stems = HashMap<String, Vec<f32>>;

/// 進み具合の知らせ。GUI からも CLI からも使う。
pub type Progress<'a> = &'a (dyn Fn(&str) + Sync);

/// MIDI のドラム番号 -> 音の作り方。Python 版の `DRUMS` と同じ割り当て。
pub fn drum_voice(note: i32, n: usize, vel: f32, seed: u64, kick: &drum::KickCfg) -> Vec<f32> {
    match note {
        35 => drum::hardkick(n, vel, seed, kick, None), // 歪んだキック
        36 => drum::kick(n, vel, seed),
        37 => drum::rim(n, vel, seed),
        38 => drum::snare(n, vel, seed),
        39 => drum::clap(n, vel, seed),
        41 => drum::tom(n, vel, 110.0, seed), // ロータム
        42 => drum::hat(n, vel, 0.035, seed), // 閉じたハット
        45 => drum::tom(n, vel, 160.0, seed), // ミドルタム
        46 => drum::hat(n, vel, 0.16, seed),  // 開いたハット
        48 => drum::tom(n, vel, 220.0, seed), // ハイタム
        49 => drum::crash(n, vel, seed),
        51 => drum::ride(n, vel, seed),
        52 => drum::reverse(n, vel, seed),
        70 => drum::shaker(n, vel, seed),
        _ => drum::hat(n, vel, 0.05, seed),
    }
}

/// 1本の音符を音にする。
///
/// 鍵を握ったときに鳴らす側（`tonescript-engine`）もこれを呼ぶ。
/// **書き出しと聞こえ方が違わないよう、作る所は1つにする。**
pub fn render_note(
    song: &Song,
    cfg: &Cfg,
    part: &str,
    note: &Note,
    start: usize,
    end: usize,
    ring: f32,
) -> Option<(usize, Vec<f32>)> {
    let vel = note.vel as f32 / 127.0;
    // 余韻ぶん長く鳴らす。音価ぴったりで切ると音のあいだに穴が空く
    let n = (end - start) + (ring * SR) as usize;
    if n == 0 {
        return None;
    }
    // 種は「位置と音程」から作る。同じ音符は毎回同じ音になり、
    // 隣り合う音符は別の雑音になる。
    let seed = (note.pos as u64) * 131 + (note.pitch as u64) * 17 + part.len() as u64;

    let wave = if part == "drums" || part == "perc" {
        drum_voice(note.pitch, n, vel, seed, &song.kick)
    } else {
        let bar = song.bar_of_step(note.pos);
        let name = arrange::patch_for(song, part, bar)?;
        voice_of(song, cfg, &name, freq_of(note.pitch), n, vel, seed)?
    };
    Some((start, wave))
}

/// 名前から音を作る。**曲ファイルで作った音色を先に見る。**
///
/// 同じ名前なら曲ファイル側が勝つ。内蔵の `piano` が好みでなければ、
/// 曲ファイルに `piano` を書いて作り替えられる。
pub fn voice_of(
    song: &Song,
    cfg: &Cfg,
    name: &str,
    freq: f32,
    n: usize,
    vel: f32,
    seed: u64,
) -> Option<Vec<f32>> {
    if let Some(r) = song.patches.get(name) {
        return Some(tonescript_dsp::recipe::render(r, freq, n, vel, seed));
    }
    patch::render(name, cfg, freq, n, vel, seed)
}

/// その音色の余韻（秒）。曲ファイルで作った音色は自分で持っている。
pub fn ring_of(song: &Song, name: &str) -> f32 {
    match song.patches.get(name) {
        Some(r) => r.ring,
        None => patch::ring(name),
    }
}

/// パートごとに音を作る。ノートは並列に回す。
pub fn render_stems(song: &Song, score: &Score, progress: Progress) -> Stems {
    let times = arrange::step_times(song);
    let total = ((times.last().copied().unwrap_or(0.0) + 4.0) * SR as f64) as usize;
    let cfg = Cfg::default();

    let at = |step: u32| -> usize {
        let i = (step as usize).min(times.len() - 1);
        (times[i] * SR as f64) as usize
    };

    let mut parts: Vec<&String> = score.keys().collect();
    parts.sort();

    parts
        .par_iter()
        .map(|part| {
            let notes = &score[*part];
            let ring = if *part == "drums" || *part == "perc" {
                0.0
            } else {
                arrange::patch_for(song, part, 1).map(|p| ring_of(song, &p)).unwrap_or(0.0)
            };
            // ノートを並列に作ってから、1本の帯へ足し込む
            let rendered: Vec<(usize, Vec<f32>)> = notes
                .par_iter()
                .filter_map(|nt| {
                    let s = at(nt.pos);
                    let e = at(nt.pos + nt.len.max(1));
                    render_note(song, &cfg, part, nt, s, e.max(s + 1), ring)
                })
                .collect();
            let mut buf = vec![0.0f32; total];
            for (start, wave) in rendered {
                for (i, v) in wave.iter().enumerate() {
                    let j = start + i;
                    if j >= total {
                        break;
                    }
                    buf[j] += v;
                }
            }
            progress(&format!("  合成 {:<8} {:>5} ノート", part, notes.len()));
            ((*part).clone(), buf)
        })
        .collect()
}

/// 音声を、曲の上へ置ける形に整える。
///
/// 頭と尻を落とし、出入りを滑らかにして、曲の位置まで前へ送る。
/// **鳴らす側も書き出す側も、ここを通る。**
pub fn place(l: &[f32], r: &[f32], t: &tonescript_song::model::AudioTrack, at: usize) -> Stereo {
    let n = l.len().min(r.len());
    let cut_in = ((t.trim_in * SR) as usize).min(n);
    let cut_out = ((t.trim_out * SR) as usize).min(n - cut_in);
    let body = n - cut_in - cut_out;
    let mut out = Stereo { l: vec![0.0; at + body], r: vec![0.0; at + body] };
    if body == 0 {
        return out;
    }
    // 出入り。足して body を超えるときは、半分ずつに収める
    let mut fi = ((t.fade_in * SR) as usize).min(body);
    let mut fo = ((t.fade_out * SR) as usize).min(body);
    if fi + fo > body {
        let half = body / 2;
        fi = fi.min(half);
        fo = fo.min(body - half);
    }
    for i in 0..body {
        let mut g = 1.0f32;
        if fi > 0 && i < fi {
            g *= i as f32 / fi as f32;
        }
        if fo > 0 && i >= body - fo {
            g *= (body - i) as f32 / fo as f32;
        }
        out.l[at + i] = l[cut_in + i] * g;
        out.r[at + i] = r[cut_in + i] * g;
    }
    out
}

/// 外で作った歌（AUDIO_TRACKS）を読む。
///
/// 譜面ではなく WAV をそのまま鳴らす。Synthesizer V などで書き出した歌を
/// ここに置く。同じパートの別テイクを重ねればダブリングになるし、
/// 3度下のハモリを別トラックにすれば和声が付く。
///
/// パスは TONESCRIPT_ROOT からの相対。曲ファイル側で絶対パスは弾いてある。
/// 曲の 0 秒から始まっている前提で、頭を詰めたりはしない。
pub fn load_audio_tracks(
    song: &Song,
    root: &std::path::Path,
    progress: Progress,
) -> (Vec<(String, Stereo, f32)>, Vec<String>) {
    let mut out = Vec::new();
    let mut missing = Vec::new();
    let mut names: Vec<&String> = song.audio_tracks.keys().collect();
    names.sort();
    for name in names {
        let t = &song.audio_tracks[name];
        if t.gain <= 0.0 {
            progress(&format!("       {:<10} 音量 0 なので使いません", t.label));
            continue;
        }
        let path = root.join(&t.path);
        match wav::read(&path) {
            Ok((l, r, sr)) => {
                if sr != SR as u32 {
                    // 読み替えはしない。黙って音程がずれるより、言って止まるほうがいい
                    missing.push(format!(
                        "{}: {}Hz で書き出されています（{}Hz にしてください）",
                        t.label, sr, SR as u32
                    ));
                    continue;
                }
                let secs = l.len() as f32 / SR;
                // 置き場所・切り詰め・出入りを当てる
                let times = arrange::step_times(song);
                let at = (times[(t.at as usize).min(times.len() - 1)] * SR as f64) as usize;
                let placed = place(&l, &r, t, at);
                let mut note = String::new();
                if t.at > 0 {
                    note += &format!("  {}目盛りから", t.at);
                }
                if t.trim_in > 0.0 || t.trim_out > 0.0 {
                    note += &format!("  切り詰め {:.2}/{:.2}秒", t.trim_in, t.trim_out);
                }
                if t.fade_in > 0.0 || t.fade_out > 0.0 {
                    note += &format!("  出入り {:.2}/{:.2}秒", t.fade_in, t.fade_out);
                }
                progress(&format!(
                    "       {:<10} x{:.2}  {:.1}秒  {}{}",
                    t.label, t.gain, secs, t.path, note
                ));
                out.push((t.label.clone(), placed, t.gain));
            }
            Err(e) => missing.push(format!("{}: {e}", t.label)),
        }
    }
    (out, missing)
}

/// ミックス。パートごとの音を左右2本へまとめる。
pub fn mix_down(song: &Song, stems: &mut Stems, score: &Score, progress: Progress) -> Stereo {
    mix_down_with(song, stems, score, &[], progress)
}

/// 歌（AUDIO_TRACKS）も一緒にまとめる版。
pub fn mix_down_with(
    song: &Song,
    stems: &mut Stems,
    score: &Score,
    vocals: &[(String, Stereo, f32)],
    progress: Progress,
) -> Stereo {
    let total = stems.values().map(|v| v.len()).max().unwrap_or(0);
    let times = arrange::step_times(song);
    let at = |step: u32| -> usize {
        let i = (step as usize).min(times.len() - 1);
        (times[i] * SR as f64) as usize
    };

    // キックの位置からサイドチェインの曲線を作る
    let kicks: Vec<usize> = score
        .get("drums")
        .map(|ns| ns.iter().filter(|n| n.pitch == 35 || n.pitch == 36).map(|n| at(n.pos)).collect())
        .unwrap_or_default();
    let (depth, a, h, r) = song.sidechain;
    let duck_env = mix::sidechain_env(&kicks, total, depth, a, h, r);
    progress(&format!(
        "  サイドチェイン: キック {} 発で最大 {:.0}% 凹ませる",
        kicks.len(),
        depth * 100.0
    ));

    let mut out = Stereo::silent(total);
    // 残響へ送る音をここへ集める。パートごとに別々に掛けるのではなく、
    // 1つの部屋へまとめて送る。実際の部屋と同じで、そのほうが馴染む
    // （別々に掛けると、パートごとに違う部屋で鳴っているように聞こえる）
    let mut send = vec![0.0f32; total];
    let mut sent = 0usize;

    let mut names: Vec<&String> = stems.keys().collect();
    names.sort();
    let names: Vec<String> = names.into_iter().cloned().collect();

    progress("  トラック    ピーク  実効   尖り   上限");
    for name in &names {
        let buf = stems.get_mut(name).unwrap();
        let before_peak = tonescript_dsp::peak(buf);
        let before_rms = tonescript_dsp::rms(buf);
        let crest = if before_rms > 1e-9 { before_peak / before_rms } else { 0.0 };
        // 打楽器は頭が命。触らない
        let limit = if name == "drums" || name == "perc" { None } else { Some(14.0) };
        let cut = mix::tame_crest(buf, limit, 0.72);
        progress(&format!(
            "  {:<10} {:>6.3} {:>6.4} {:>6.1}倍  {}{}",
            name,
            before_peak,
            before_rms,
            crest,
            limit.map(|v| format!("{v:.0}倍")).unwrap_or_else(|| "—".into()),
            cut.map(|d| format!("  ★ {d:.1}dB")).unwrap_or_default()
        ));

        let cfg = song.mix.get(name).copied().unwrap_or_default();
        let gain = song.gains.get(name).copied().unwrap_or(1.0);
        let lanes = song.automation.get(name);
        let lane_of = |l: Lane| -> Option<Vec<f32>> {
            lanes
                .and_then(|m| m.get(&l))
                .and_then(|c| mix::curve_to_samples(c, &times, total, SR))
        };

        // サイドチェイン。掛かり具合も線で動かせる
        let duck_curve = lane_of(Lane::Duck);
        if cfg.duck > 0.0 || duck_curve.is_some() {
            for (i, v) in buf.iter_mut().enumerate() {
                let amount = duck_curve.as_ref().map(|c| c[i]).unwrap_or(cfg.duck);
                *v *= 1.0 - (1.0 - duck_env[i]) * amount;
            }
        }
        // 音量の線
        if let Some(g) = lane_of(Lane::Gain) {
            for (v, a) in buf.iter_mut().zip(&g) {
                *v *= a;
            }
            progress(&format!("             オートメーション: 音量 {} 節", 
                lanes.and_then(|m| m.get(&Lane::Gain)).map(|c| c.points.len()).unwrap_or(0)));
        }

        // 残響への送り。線で動かせる
        let rv_curve = lane_of(Lane::Reverb);
        if cfg.reverb > 0.0 || rv_curve.is_some() {
            let mut any = false;
            for (i, v) in buf.iter().enumerate() {
                let amount = rv_curve.as_ref().map(|c| c[i]).unwrap_or(cfg.reverb);
                if amount > 0.0 {
                    send[i] += v * amount;
                    any = true;
                }
            }
            if any {
                sent += 1;
            }
        }

        let mut st = mix::widen(buf, cfg.width);
        // 左右の線。widen（広がり）とは別物で、こちらは位置を動かす
        if let Some(p) = lane_of(Lane::Pan) {
            for i in 0..st.l.len() {
                let (gl, gr) = mix::pan_gains(p[i]);
                // 等出力の配り方は中央で 1/√2 ずつ。中央を 1.0 に直して掛ける
                st.l[i] *= gl * std::f32::consts::SQRT_2;
                st.r[i] *= gr * std::f32::consts::SQRT_2;
            }
            progress(&format!("             オートメーション: 左右 {} 節",
                lanes.and_then(|m| m.get(&Lane::Pan)).map(|c| c.points.len()).unwrap_or(0)));
        }
        out.add(&st, gain);
    }
    // 残響。集めた送りを1つの部屋へ通して、戻ってきたぶんを足す
    if sent > 0 {
        let (secs, spread) = song.reverb;
        let mut rv = tonescript_dsp::reverb::Reverb::new(secs, spread);
        let (rl, rr) = rv.process(&send);
        for i in 0..total {
            out.l[i] += rl[i];
            out.r[i] += rr[i];
        }
        progress(&format!(
            "  残響 {:.1}秒 / 広がり {:.1}  （{} パートから送り）",
            secs, spread, sent
        ));
    }

    // 歌を重ねる。譜面のパートより後に足す。
    // 伴奏側のサイドチェインや尖り制限は、歌には掛けない
    // （外のソフトで既に整っているものへ二重に掛けると割れる）
    if !vocals.is_empty() {
        let mut longest = out.len();
        for (_, v, _) in vocals {
            longest = longest.max(v.len());
        }
        if longest > out.len() {
            out.l.resize(longest, 0.0);
            out.r.resize(longest, 0.0);
        }
        for (label, v, gain) in vocals {
            for i in 0..v.len().min(out.len()) {
                out.l[i] += v.l[i] * gain;
                out.r[i] += v.r[i] * gain;
            }
            progress(&format!("  歌 {:<10} x{:.2}", label, gain));
        }
    }
    out.scale(song.master_gain);
    out
}

/// マスター。直流を落として、音圧を合わせて、天井を丸める。
pub fn master(s: &mut Stereo, target: f32, progress: Progress) {
    let dc = mix::remove_dc(&mut s.l);
    mix::remove_dc(&mut s.r);
    if dc.abs() > 1e-4 {
        progress(&format!("  直流ずれ {dc:+.5} を落とした（18Hz 以下）"));
    }
    let (g, before) = mix::normalize_lufs(s, target);
    let peak_before = s.peak();
    let cut = mix::limit(s, 0.99);
    progress(&format!(
        "  音圧 {before:.1} → {target:.1} LUFS  倍率 x{g:.3}  ピーク {peak_before:.3}→{:.3}{}",
        s.peak(),
        if cut < 0.0 { format!("  リミッタ {cut:.1}dB") } else { String::new() }
    ));
}

/// 曲まるごと。譜面から完成品まで。歌も込みで作る。
///
/// `root` は AUDIO_TRACKS のパスを解決する基点（TONESCRIPT_ROOT）。
pub fn render_song_at(
    song: &Song,
    root: &std::path::Path,
    progress: Progress,
) -> Result<(Stereo, Stems), String> {
    let score = build(song)?;
    let notes: usize = score.values().map(|v| v.len()).sum();
    progress(&format!(
        "[build] 全{}小節（{}） / {} ノート / BPM {} / {}",
        song.bars(),
        song.sections.iter().map(|s| format!("{}{}", s.name, s.bars)).collect::<Vec<_>>().join(" + "),
        notes,
        song.bpm,
        song.key
    ));
    let mut stems = render_stems(song, &score, progress);

    let vocals = if song.audio_tracks.is_empty() {
        Vec::new()
    } else {
        progress(&format!("[歌] AUDIO_TRACKS から {} 本", song.audio_tracks.len()));
        let (v, missing) = load_audio_tracks(song, root, progress);
        if !missing.is_empty() {
            // 黙って歌なしで書き出すと、出来上がりを聴くまで気づけない
            return Err(format!("歌のファイルが読めません:
    {}", missing.join("
    ")));
        }
        v
    };

    progress("[mix] 空間処理");
    let mut out = mix_down_with(song, &mut stems, &score, &vocals, progress);
    progress("[master] 仕上げ");
    master(&mut out, song.master_lufs, progress);
    Ok((out, stems))
}

/// 歌を読まずに作る。試すときや、譜面だけ確かめたいとき。
pub fn render_song(song: &Song, progress: Progress) -> Result<(Stereo, Stems), String> {
    let score = build(song)?;
    let notes: usize = score.values().map(|v| v.len()).sum();
    progress(&format!(
        "[build] 全{}小節（{}） / {} ノート / BPM {} / {}",
        song.bars(),
        song.sections.iter().map(|s| format!("{}{}", s.name, s.bars)).collect::<Vec<_>>().join(" + "),
        notes,
        song.bpm,
        song.key
    ));
    let mut stems = render_stems(song, &score, progress);
    progress("[mix] 空間処理");
    let mut out = mix_down(song, &mut stems, &score, progress);
    progress("[master] 仕上げ");
    master(&mut out, song.master_lufs, progress);
    Ok((out, stems))
}


#[cfg(test)]
mod place_tests {
    use super::*;
    use tonescript_song::model::AudioTrack;

    fn track() -> AudioTrack {
        AudioTrack { path: "x.wav".into(), ..Default::default() }
    }

    fn flat(n: usize) -> Vec<f32> {
        vec![1.0; n]
    }

    #[test]
    fn plain_audio_is_left_alone() {
        let x = flat(100);
        let out = place(&x, &x, &track(), 0);
        assert_eq!(out.len(), 100);
        assert!(out.l.iter().all(|v| *v == 1.0), "何もしていないのに触った");
    }

    #[test]
    fn it_lands_where_it_was_put() {
        let x = flat(100);
        let out = place(&x, &x, &track(), 50);
        assert_eq!(out.len(), 150);
        assert!(out.l[..50].iter().all(|v| *v == 0.0), "前が無音になっていない");
        assert_eq!(out.l[50], 1.0, "置いた所から始まっていない");
        assert_eq!(out.l[149], 1.0);
    }

    #[test]
    fn trimming_takes_off_both_ends() {
        let n = (1.0 * SR) as usize;
        let mut x = vec![1.0f32; n];
        x[0] = 9.0; // 頭の物音
        x[n - 1] = 9.0; // 尻の物音
        let t = AudioTrack { trim_in: 0.1, trim_out: 0.1, ..track() };
        let out = place(&x, &x, &t, 0);
        let want = n - (0.2 * SR) as usize;
        assert!((out.len() as i32 - want as i32).abs() < 2, "長さが {}", out.len());
        assert!(out.l.iter().all(|v| *v < 9.0), "落としたはずの物音が残っている");
    }

    #[test]
    fn fades_go_from_nothing_to_all() {
        let n = (1.0 * SR) as usize;
        let x = flat(n);
        let t = AudioTrack { fade_in: 0.2, fade_out: 0.2, ..track() };
        let out = place(&x, &x, &t, 0);
        assert_eq!(out.l[0], 0.0, "頭から鳴っている");
        let mid_in = (0.1 * SR) as usize;
        assert!((out.l[mid_in] - 0.5).abs() < 0.01, "入りの半ばが {}", out.l[mid_in]);
        assert!((out.l[n / 2] - 1.0).abs() < 1e-6, "真ん中で下がっている");
        assert!(*out.l.last().unwrap() < 0.01, "尻が消えていない");
    }

    #[test]
    fn overlapping_fades_share_the_space() {
        // 1秒の音に、入り1秒・出1秒。足すと足りないので半分ずつに収める
        let n = (1.0 * SR) as usize;
        let x = flat(n);
        let t = AudioTrack { fade_in: 1.0, fade_out: 1.0, ..track() };
        let out = place(&x, &x, &t, 0);
        assert_eq!(out.len(), n, "長さが変わった");
        assert!(out.l.iter().all(|v| *v <= 1.0 + 1e-6), "1 を超えた");
        // 真ん中がいちばん大きいこと
        let peak_at = out
            .l
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert!(
            (peak_at as i32 - (n / 2) as i32).abs() < (n / 5) as i32,
            "山が真ん中に無い: {peak_at}"
        );
    }

    #[test]
    fn trimming_everything_leaves_silence_not_a_crash() {
        let x = flat(100);
        let t = AudioTrack { trim_in: 10.0, trim_out: 10.0, ..track() };
        let out = place(&x, &x, &t, 10);
        assert!(out.l.iter().all(|v| *v == 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonescript_song::load_str;

    const SRC: &str = r#"
        let TITLE = "t"; let BPM = 120;
        let SECTIONS = [["A", 2, "plain", "k", "main", 1.0]];
        let VOICES = #{ lead: #{ch:0, patch:"supersaw"}, bass: #{ch:2, patch:"acid"},
                        drums: #{ch:9}, perc: #{ch:9} };
        let CHORDS = #{ "1": ["Am", ["A3","C4","E4"], "A2"],
                        "2": ["F",  ["F3","A3","C4"], "F2"] };
        let MELODY = #{ "1": bar([[8,"A4"],[8,"C5"]]), "2": bar([[16,"E5"]]) };
        let ARRANGE = #{ "1": ["lead","bass","kick","closedhat"],
                         "2": ["lead","bass","kick","closedhat"] };
        let BASS_PATTERNS = #{ plain: [[0,4,0],[4,4,0],[8,4,0],[12,4,0]] };
        let DRUM_KITS = #{ k: #{ kick: [36, [[0,100],[8,100]]],
                                 closedhat: [42, [[2,52],[6,52]]] } };
        let GAINS = #{ lead: 1.0, bass: 1.0, drums: 1.0, perc: 1.0 };
        let MIX = #{ lead: #{width: 1.0, reverb: 0.0, duck: 0.5},
                     bass: #{width: 0.0, reverb: 0.0, duck: 1.0},
                     drums: #{width: 0.4, reverb: 0.0, duck: 0.0},
                     perc: #{width: 0.8, reverb: 0.0, duck: 0.3} };
        let MASTER_LUFS = -14.0;
    "#;

    fn quiet(_: &str) {}

    #[test]
    fn renders_end_to_end() {
        let s = load_str(SRC).unwrap();
        let (out, stems) = render_song(&s, &quiet).unwrap();
        // 120BPM の2小節 = 4秒。余韻ぶん少し長い
        let secs = out.len() as f32 / SR;
        assert!(secs > 3.9 && secs < 9.0, "長さが変: {secs}秒");
        assert!(out.l.iter().all(|v| v.is_finite()), "値が飛んだ");
        assert!(tonescript_dsp::rms(&out.l) > 1e-3, "無音");
        assert!(out.peak() <= 1.0, "天井超え: {}", out.peak());
        for (name, buf) in &stems {
            assert!(buf.iter().all(|v| v.is_finite()), "{name}: 値が飛んだ");
        }
    }

    #[test]
    fn master_hits_the_target_loudness() {
        let s = load_str(SRC).unwrap();
        let (out, _) = render_song(&s, &quiet).unwrap();
        let got = mix::lufs(&out.l, &out.r);
        // リミッタが少し削るので、ぴったりにはならない
        assert!((got + 14.0).abs() < 2.0, "音圧が {got} LUFS");
    }

    #[test]
    fn silent_song_does_not_panic() {
        let src = SRC.replace(r#"let ARRANGE = #{ "1": ["lead","bass","kick","closedhat"],
                         "2": ["lead","bass","kick","closedhat"] };"#,
                              r#"let ARRANGE = #{};"#);
        let s = load_str(&src).unwrap();
        let (out, _) = render_song(&s, &quiet).unwrap();
        assert!(out.l.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn same_song_renders_the_same_twice() {
        let s = load_str(SRC).unwrap();
        let (a, _) = render_song(&s, &quiet).unwrap();
        let (b, _) = render_song(&s, &quiet).unwrap();
        assert_eq!(a.l.len(), b.l.len());
        // 並列にしても結果が変わらないこと
        let d = a.l.iter().zip(&b.l).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        assert!(d < 1e-6, "2回で結果が違う: {d}");
    }

    #[test]
    fn gain_automation_actually_fades() {
        // lead だけを頭から尻へフェードアウトさせて、前半と後半を比べる
        let src = format!(
            "{SRC}
let AUTOMATION = #{{ lead: #{{ gain: [[0, 1.0], [32, 0.0]] }} }};"
        );
        let plain = load_str(SRC).unwrap();
        let faded = load_str(&src).unwrap();
        let (_, a) = render_song(&plain, &quiet).unwrap();
        let (_, b) = render_song(&faded, &quiet).unwrap();
        // 曲は 120BPM の2小節 = 4秒。書き出しの帯はそれより長いので、
        // 比べる窓は曲の中に取る（末尾は無音で、どちらも 0 になる）
        let sr = SR as usize;
        let head = ..sr / 2; // 0〜0.5秒
        let tail = sr * 3..sr * 4; // 3〜4秒
        let ha = tonescript_dsp::rms(&a["lead"][head]);
        let hb = tonescript_dsp::rms(&b["lead"][head]);
        assert!(ha > 1e-4 && hb > 1e-4, "頭が無音 {ha} {hb}");
        assert!((ha - hb).abs() / ha < 0.15, "頭まで変わった {ha} -> {hb}");
        let ta = tonescript_dsp::rms(&a["lead"][tail.clone()]);
        let tb = tonescript_dsp::rms(&b["lead"][tail]);
        assert!(ta > 1e-4, "比べる所が無音 {ta}");
        assert!(tb < ta * 0.35, "後ろが下がっていない {ta} -> {tb}");
    }

    #[test]
    fn pan_automation_moves_the_image() {
        // bass を左へ振り切る。左右の大きさが変わるはず
        let src = format!("{SRC}
let AUTOMATION = #{{ bass: #{{ pan: [[0, -1.0]] }} }};");
        let s = load_str(&src).unwrap();
        let (out, _) = render_song(&s, &quiet).unwrap();
        let l = tonescript_dsp::rms(&out.l);
        let r = tonescript_dsp::rms(&out.r);
        assert!(l > r * 1.2, "左へ寄っていない 左{l} 右{r}");
    }

    #[test]
    fn automation_does_not_break_when_tempo_moves() {
        let src = format!(
            "{SRC}
let TEMPO_MAP = #{{ \"1\": 90, \"2\": 180 }};             
let AUTOMATION = #{{ lead: #{{ gain: [[0, 1.0], [32, 0.2]] }} }};"
        );
        let s = load_str(&src).unwrap();
        let (out, _) = render_song(&s, &quiet).unwrap();
        assert!(out.l.iter().all(|v| v.is_finite()));
        assert!(tonescript_dsp::rms(&out.l) > 1e-4);
    }

    #[test]
    fn odd_meter_song_renders() {
        let src = SRC.replace(
            r#"let SECTIONS = [["A", 2, "plain", "k", "main", 1.0]];"#,
            r#"let SECTIONS = [["A", 2, "plain", "k", "main", 1.0, [7, 8]]];"#,
        )
        .replace(r#"let MELODY = #{ "1": bar([[8,"A4"],[8,"C5"]]), "2": bar([[16,"E5"]]) };"#,
                 r#"let MELODY = #{ "1": [[7,"A4"],[7,"C5"]], "2": [[14,"E5"]] };"#);
        let s = load_str(&src).expect("7/8 の曲が読めるはず");
        assert_eq!(s.total_steps(), 28);
        let (out, _) = render_song(&s, &quiet).unwrap();
        assert!(out.l.iter().all(|v| v.is_finite()));
        assert!(tonescript_dsp::rms(&out.l) > 1e-4, "無音");
    }

    #[test]
    fn reverb_actually_rings() {
        // 残響を送ると、音が止まったあとも鳴りが残るはず
        let dry = SRC.replace("reverb: 0.0, duck: 0.5", "reverb: 0.0, duck: 0.5");
        let wet = SRC.replace(
            "lead: #{width: 1.0, reverb: 0.0, duck: 0.5}",
            "lead: #{width: 1.0, reverb: 0.8, duck: 0.5}",
        );
        let a = load_str(&dry).unwrap();
        let b = load_str(&wet).unwrap();
        let (da, _) = render_song(&a, &quiet).unwrap();
        let (db, _) = render_song(&b, &quiet).unwrap();

        // 曲は 4 秒。そのあとの「鳴り終わり」を比べる
        let sr = SR as usize;
        let tail = sr * 4 + sr / 4..sr * 5;
        let ra = tonescript_dsp::rms(&da.l[tail.clone()]);
        let rb = tonescript_dsp::rms(&db.l[tail]);
        assert!(rb > ra * 2.0, "残響が出ていない（無し {ra} / 有り {rb}）");
        assert!(db.l.iter().all(|v| v.is_finite()));
        assert!(db.peak() <= 1.0, "天井を超えた");
    }

    #[test]
    fn no_reverb_send_means_no_tail() {
        // どのパートも送っていなければ、残響は掛からない
        let s = load_str(SRC).unwrap();
        assert!(s.mix.values().all(|m| m.reverb == 0.0), "前提が崩れている");
        let (out, _) = render_song(&s, &quiet).unwrap();
        let sr = SR as usize;
        let tail = tonescript_dsp::rms(&out.l[sr * 4 + sr / 2..sr * 5]);
        assert!(tail < 1e-3, "送っていないのに鳴っている: {tail}");
    }

    #[test]
    fn reverb_automation_moves_the_send() {
        // 線で送りを動かせること
        let src = format!(
            "{SRC}
let AUTOMATION = #{{ lead: #{{ reverb: [[0, 0.0], [32, 1.0]] }} }};"
        );
        let s = load_str(&src).unwrap();
        let (out, _) = render_song(&s, &quiet).unwrap();
        assert!(out.l.iter().all(|v| v.is_finite()));
        // 後半のほうが残響が乗っているので、尻尾が残る
        let sr = SR as usize;
        let tail = tonescript_dsp::rms(&out.l[sr * 4 + sr / 4..sr * 5]);
        assert!(tail > 1e-4, "線で送っても鳴らない: {tail}");
    }

    #[test]
    fn stems_cover_every_sounding_part() {
        let s = load_str(SRC).unwrap();
        let (_, stems) = render_song(&s, &quiet).unwrap();
        let mut names: Vec<&String> = stems.keys().collect();
        names.sort();
        assert_eq!(names, vec!["bass", "drums", "lead", "perc"]);
    }
}
