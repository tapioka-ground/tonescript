//! 標準 MIDI ファイル（SMF）の読み書き。
//!
//! MIDI は音を運ばない。音符・強さ・タイミングだけを運ぶ形式で、
//! 実際にどんな音で鳴るかは受け手が決める。だからここでは
//! 「音符のやり取り」だけを扱い、音色は番号として添えるにとどめる。
//!
//! 形式の作り
//! ----------
//! ```text
//! MThd  6  format  ntrks  division     ヘッダ
//! MTrk  長さ  イベント...                 トラック（曲の数だけ続く）
//! ```
//!
//! イベントの前には必ず「前のイベントから何 tick 経ったか」が付く。
//! この数は可変長で、7bit ずつ小分けにして、続きがあれば最上位を立てる。
//!
//! 走行状態（running status）
//! --------------------------
//! 同じ種類のイベントが続くとき、2回目以降は種類を書かなくてよい決まりが
//! ある。書くときは使わない（読みにくいので）が、読むときは必ず要る。
//! 世の中の MIDI ファイルはほぼ全部これを使っている。

use std::collections::HashMap;

/// 4分音符ひとつを何 tick に割るか。
///
/// 480 は広く使われている値。16分音符が 120 tick になり、
/// 3連符（160）も5連符（96）も割り切れる。
pub const TPQ: u16 = 480;

/// ひとつの音符。
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// 曲の頭からの位置（tick）
    pub at: u32,
    pub len: u32,
    pub pitch: u8,
    pub vel: u8,
    /// 歌詞。無ければ空
    pub lyric: String,
}

/// ひとつのトラック。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub name: String,
    /// MIDI のチャンネル。9 は打楽器
    pub channel: u8,
    /// 音色の番号。無ければ送らない
    pub program: Option<u8>,
    pub notes: Vec<Note>,
}

/// テンポの節目。(tick, BPM)
pub type Tempo = (u32, f32);
/// 拍子の節目。(tick, 分子, 分母)
pub type TimeSig = (u32, u8, u8);

/// ファイルまるごと。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smf {
    pub title: String,
    pub tempos: Vec<Tempo>,
    pub time_sigs: Vec<TimeSig>,
    pub tracks: Vec<Track>,
}

// ---------------------------------------------------------------- 書く

/// 可変長の数。7bit ずつ、続きがあれば最上位を立てる。
fn push_varlen(out: &mut Vec<u8>, mut v: u32) {
    let mut buf = [0u8; 5];
    let mut n = 0;
    buf[n] = (v & 0x7f) as u8;
    v >>= 7;
    while v > 0 {
        n += 1;
        buf[n] = ((v & 0x7f) as u8) | 0x80;
        v >>= 7;
    }
    // 上の桁から書く
    for i in (0..=n).rev() {
        out.push(buf[i]);
    }
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
}

/// メタイベント。
fn meta(out: &mut Vec<u8>, delta: u32, kind: u8, body: &[u8]) {
    push_varlen(out, delta);
    out.push(0xff);
    out.push(kind);
    push_varlen(out, body.len() as u32);
    out.extend_from_slice(body);
}

/// 書き出す。
pub fn write(smf: &Smf) -> Vec<u8> {
    let mut out = Vec::new();
    // 形式 1（トラックが並ぶ）。1本目は指揮者のトラックにする決まり
    let ntrks = smf.tracks.len() as u16 + 1;
    let mut head = Vec::new();
    head.extend_from_slice(&1u16.to_be_bytes());
    head.extend_from_slice(&ntrks.to_be_bytes());
    head.extend_from_slice(&TPQ.to_be_bytes());
    push_chunk(&mut out, b"MThd", &head);

    // --- 指揮者のトラック。テンポと拍子だけ
    let mut c = Vec::new();
    if !smf.title.is_empty() {
        meta(&mut c, 0, 0x03, smf.title.as_bytes());
    }
    // テンポと拍子を時刻の順に混ぜる
    let mut events: Vec<(u32, u8, Vec<u8>)> = Vec::new();
    for &(at, bpm) in &smf.tempos {
        // 4分音符ひとつが何マイクロ秒か
        let us = (60_000_000.0 / bpm.max(1.0)) as u32;
        events.push((at, 0x51, us.to_be_bytes()[1..].to_vec()));
    }
    for &(at, num, den) in &smf.time_sigs {
        // 分母は「2の何乗か」で書く。4 なら 2、8 なら 3
        let dd = den.max(1).ilog2() as u8;
        // 24 = メトロノームが鳴る間隔、8 = 4分音符あたりの32分音符の数
        events.push((at, 0x58, vec![num, dd, 24, 8]));
    }
    events.sort_by_key(|e| (e.0, e.1));
    let mut last = 0u32;
    for (at, kind, body) in events {
        meta(&mut c, at - last, kind, &body);
        last = at;
    }
    meta(&mut c, 0, 0x2f, &[]); // トラックの終わり
    push_chunk(&mut out, b"MTrk", &c);

    // --- 音符のトラック
    for t in &smf.tracks {
        let mut b = Vec::new();
        if !t.name.is_empty() {
            meta(&mut b, 0, 0x03, t.name.as_bytes());
        }
        if let Some(p) = t.program {
            push_varlen(&mut b, 0);
            b.push(0xc0 | (t.channel & 0x0f));
            b.push(p & 0x7f);
        }
        // 音の開始と終了を時刻の順に並べる。
        // 終了を先に置かないと、同じ高さの音が重なったときに消え方が狂う
        let mut ev: Vec<(u32, u8, u8, u8, String)> = Vec::new();
        for n in &t.notes {
            ev.push((n.at, 1, n.pitch, n.vel.max(1), n.lyric.clone()));
            ev.push((n.at + n.len.max(1), 0, n.pitch, 0, String::new()));
        }
        // 同じ時刻なら、終了を先に
        ev.sort_by_key(|e| (e.0, e.1, e.2));
        let mut last = 0u32;
        for (at, on, pitch, vel, lyric) in ev {
            if on == 1 && !lyric.is_empty() {
                meta(&mut b, at - last, 0x05, lyric.as_bytes());
                last = at;
            }
            push_varlen(&mut b, at - last);
            b.push(if on == 1 { 0x90 } else { 0x80 } | (t.channel & 0x0f));
            b.push(pitch & 0x7f);
            b.push(vel & 0x7f);
            last = at;
        }
        meta(&mut b, 0, 0x2f, &[]);
        push_chunk(&mut out, b"MTrk", &b);
    }
    out
}

// ---------------------------------------------------------------- 読む

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.i).ok_or("途中で終わっています")?;
        self.i += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(((self.u8()? as u16) << 8) | self.u8()? as u16)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(((self.u16()? as u32) << 16) | self.u16()? as u32)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let e = self.i.checked_add(n).ok_or("長さが大きすぎます")?;
        let s = self.b.get(self.i..e).ok_or("途中で終わっています")?;
        self.i = e;
        Ok(s)
    }
    fn varlen(&mut self) -> Result<u32, String> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            v = (v << 7) | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("可変長の数が長すぎます".into())
    }
}

/// 読む。
pub fn read(bytes: &[u8]) -> Result<Smf, String> {
    let mut r = Reader { b: bytes, i: 0 };
    if r.take(4)? != b"MThd" {
        return Err("MIDI ファイルに見えません（MThd がありません）".into());
    }
    let head_len = r.u32()?;
    let head_end = r.i + head_len as usize;
    let format = r.u16()?;
    let ntrks = r.u16()?;
    let division = r.u16()?;
    if division & 0x8000 != 0 {
        return Err("秒で刻む MIDI は読めません（tick で刻むものだけ）".into());
    }
    let tpq = (division & 0x7fff).max(1) as u32;
    if format > 1 {
        return Err(format!("形式 {format} の MIDI は読めません（0 か 1 のみ）"));
    }
    r.i = head_end;

    let mut smf = Smf::default();
    for _ in 0..ntrks {
        // MTrk 以外の塊は飛ばす決まり
        let id = r.take(4)?;
        let len = r.u32()? as usize;
        if id != b"MTrk" {
            r.take(len)?;
            continue;
        }
        let body = r.take(len)?;
        read_track(body, &mut smf, tpq)?;
    }
    // 音符が1つも無いトラックは残さない
    smf.tracks.retain(|t| !t.notes.is_empty());
    if smf.tempos.is_empty() {
        // テンポが書かれていなければ 120。MIDI の決まり
        smf.tempos.push((0, 120.0));
    }
    Ok(smf)
}

fn read_track(body: &[u8], smf: &mut Smf, tpq: u32) -> Result<(), String> {
    let mut r = Reader { b: body, i: 0 };
    let mut t = Track::default();
    let mut at = 0u32;
    let mut status = 0u8;
    // 鳴っている音。(チャンネル, 高さ) -> (始まった時刻, 強さ, 歌詞)
    let mut on: HashMap<(u8, u8), (u32, u8, String)> = HashMap::new();
    let mut pending_lyric = String::new();
    let mut named = false;

    while r.i < body.len() {
        at += r.varlen()?;
        let mut b = r.u8()?;
        if b < 0x80 {
            // 走行状態。種類が省かれているので、前のを使って1バイト戻す
            if status == 0 {
                return Err("イベントの種類が分かりません".into());
            }
            r.i -= 1;
            b = status;
        } else if b < 0xf0 {
            status = b;
        }

        match b {
            0xff => {
                let kind = r.u8()?;
                let len = r.varlen()? as usize;
                let data = r.take(len)?;
                match kind {
                    // トラック名。1本目なら曲名として扱う
                    0x03 => {
                        let s = String::from_utf8_lossy(data).to_string();
                        if smf.tracks.is_empty() && !named && t.notes.is_empty() {
                            if smf.title.is_empty() {
                                smf.title = s.clone();
                            }
                        }
                        t.name = s;
                        named = true;
                    }
                    0x05 => pending_lyric = String::from_utf8_lossy(data).to_string(),
                    0x51 if data.len() == 3 => {
                        let us = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | data[2] as u32;
                        let bpm = 60_000_000.0 / us.max(1) as f32;
                        smf.tempos.push((scale(at, tpq), bpm));
                    }
                    0x58 if data.len() >= 2 => {
                        let den = 1u8 << data[1].min(7);
                        smf.time_sigs.push((scale(at, tpq), data[0].max(1), den));
                    }
                    _ => {}
                }
            }
            // システム共通・システム専用。長さを読んで飛ばす
            0xf0 | 0xf7 => {
                let len = r.varlen()? as usize;
                r.take(len)?;
            }
            _ => {
                let kind = b & 0xf0;
                let ch = b & 0x0f;
                match kind {
                    0x80 | 0x90 => {
                        let pitch = r.u8()? & 0x7f;
                        let vel = r.u8()? & 0x7f;
                        // 強さ 0 の「開始」は「終了」と同じ意味。よく使われる
                        if kind == 0x90 && vel > 0 {
                            t.channel = ch;
                            on.insert(
                                (ch, pitch),
                                (at, vel, std::mem::take(&mut pending_lyric)),
                            );
                        } else if let Some((from, v, lyric)) = on.remove(&(ch, pitch)) {
                            t.notes.push(Note {
                                at: scale(from, tpq),
                                len: scale(at.saturating_sub(from), tpq).max(1),
                                pitch,
                                vel: v,
                                lyric,
                            });
                        }
                    }
                    0xc0 => {
                        t.program = Some(r.u8()? & 0x7f);
                        t.channel = ch;
                    }
                    0xd0 => {
                        r.u8()?;
                    }
                    0xa0 | 0xb0 | 0xe0 => {
                        r.u8()?;
                        r.u8()?;
                    }
                    _ => return Err(format!("知らないイベントです: {b:#04x}")),
                }
            }
        }
    }
    // 閉じられていない音は、そこまでの長さで閉じる
    for ((_, pitch), (from, vel, lyric)) in on {
        t.notes.push(Note {
            at: scale(from, tpq),
            len: scale(at.saturating_sub(from), tpq).max(1),
            pitch,
            vel,
            lyric,
        });
    }
    t.notes.sort_by_key(|n| (n.at, n.pitch));
    smf.tracks.push(t);
    Ok(())
}

/// 相手の刻みを、こちらの刻み（TPQ）へ直す。
fn scale(tick: u32, from_tpq: u32) -> u32 {
    if from_tpq == TPQ as u32 {
        return tick;
    }
    ((tick as u64 * TPQ as u64) / from_tpq.max(1) as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(at: u32, len: u32, pitch: u8) -> Note {
        Note { at, len, pitch, vel: 100, lyric: String::new() }
    }

    fn sample() -> Smf {
        Smf {
            title: "テスト曲".into(),
            tempos: vec![(0, 128.0), (1920, 174.0)],
            time_sigs: vec![(0, 4, 4), (1920, 7, 8)],
            tracks: vec![
                Track {
                    name: "主旋律".into(),
                    channel: 0,
                    program: Some(81),
                    notes: vec![note(0, 480, 69), note(480, 240, 72), note(960, 960, 76)],
                },
                Track {
                    name: "ドラム".into(),
                    channel: 9,
                    program: None,
                    notes: vec![note(0, 120, 36), note(480, 120, 38)],
                },
            ],
        }
    }

    #[test]
    fn varlen_round_trip() {
        for v in [0u32, 1, 127, 128, 255, 8192, 16383, 16384, 0x0fff_ffff] {
            let mut b = Vec::new();
            push_varlen(&mut b, v);
            let mut r = Reader { b: &b, i: 0 };
            assert_eq!(r.varlen().unwrap(), v, "{v}");
            assert_eq!(r.i, b.len(), "{v}: 余りが出た");
        }
    }

    #[test]
    fn write_then_read_keeps_the_notes() {
        let a = sample();
        let bytes = write(&a);
        let b = read(&bytes).expect("読めるはず");

        assert_eq!(b.title, "テスト曲");
        assert_eq!(b.tracks.len(), 2);
        for (x, y) in a.tracks.iter().zip(&b.tracks) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.channel, y.channel);
            assert_eq!(x.program, y.program);
            assert_eq!(x.notes, y.notes, "{} の音符が変わった", x.name);
        }
    }

    #[test]
    fn tempo_and_time_signature_survive() {
        let b = read(&write(&sample())).unwrap();
        assert_eq!(b.tempos.len(), 2);
        assert!((b.tempos[0].1 - 128.0).abs() < 0.5, "{:?}", b.tempos);
        assert!((b.tempos[1].1 - 174.0).abs() < 0.5);
        assert_eq!(b.tempos[1].0, 1920);
        assert_eq!(b.time_sigs, vec![(0, 4, 4), (1920, 7, 8)]);
    }

    #[test]
    fn lyrics_ride_along() {
        let smf = Smf {
            title: String::new(),
            tempos: vec![(0, 120.0)],
            time_sigs: vec![],
            tracks: vec![Track {
                name: "歌".into(),
                channel: 10,
                program: None,
                notes: vec![
                    Note { at: 0, len: 480, pitch: 60, vel: 90, lyric: "ら".into() },
                    Note { at: 480, len: 480, pitch: 62, vel: 90, lyric: "らー".into() },
                ],
            }],
        };
        let b = read(&write(&smf)).unwrap();
        assert_eq!(b.tracks[0].notes[0].lyric, "ら");
        assert_eq!(b.tracks[0].notes[1].lyric, "らー");
    }

    #[test]
    fn running_status_is_understood() {
        // 世の中の MIDI はほぼ全部これを使っている。読めないと話にならない
        let mut trk = Vec::new();
        push_varlen(&mut trk, 0);
        trk.extend_from_slice(&[0x90, 60, 100]); // 種類を書く
        push_varlen(&mut trk, 480);
        trk.extend_from_slice(&[62, 100]); // 種類を省く（走行状態）
        push_varlen(&mut trk, 0);
        trk.extend_from_slice(&[60, 0]); // 強さ 0 = 終了
        push_varlen(&mut trk, 480);
        trk.extend_from_slice(&[62, 0]);
        meta(&mut trk, 0, 0x2f, &[]);

        let mut f = Vec::new();
        let mut head = Vec::new();
        head.extend_from_slice(&0u16.to_be_bytes());
        head.extend_from_slice(&1u16.to_be_bytes());
        head.extend_from_slice(&TPQ.to_be_bytes());
        push_chunk(&mut f, b"MThd", &head);
        push_chunk(&mut f, b"MTrk", &trk);

        let smf = read(&f).expect("読めるはず");
        let n = &smf.tracks[0].notes;
        assert_eq!(n.len(), 2, "走行状態を読めていない: {n:?}");
        assert_eq!(n[0].pitch, 60);
        assert_eq!(n[0].len, 480);
        assert_eq!(n[1].pitch, 62);
        assert_eq!(n[1].at, 480);
    }

    #[test]
    fn a_different_tpq_is_converted() {
        // 刻みが 96 のファイル。こちらの 480 へ直る
        let mut trk = Vec::new();
        push_varlen(&mut trk, 0);
        trk.extend_from_slice(&[0x90, 60, 100]);
        push_varlen(&mut trk, 96); // 4分音符ひとつ
        trk.extend_from_slice(&[0x80, 60, 0]);
        meta(&mut trk, 0, 0x2f, &[]);

        let mut f = Vec::new();
        let mut head = Vec::new();
        head.extend_from_slice(&0u16.to_be_bytes());
        head.extend_from_slice(&1u16.to_be_bytes());
        head.extend_from_slice(&96u16.to_be_bytes());
        push_chunk(&mut f, b"MThd", &head);
        push_chunk(&mut f, b"MTrk", &trk);

        let smf = read(&f).unwrap();
        assert_eq!(smf.tracks[0].notes[0].len, 480, "刻みが直っていない");
    }

    #[test]
    fn overlapping_same_pitch_does_not_get_lost() {
        // 同じ高さが隙間なく続く。終了を先に書いていないと1本消える
        let smf = Smf {
            title: String::new(),
            tempos: vec![(0, 120.0)],
            time_sigs: vec![],
            tracks: vec![Track {
                name: "x".into(),
                channel: 0,
                program: None,
                notes: vec![note(0, 480, 60), note(480, 480, 60), note(960, 480, 60)],
            }],
        };
        let b = read(&write(&smf)).unwrap();
        assert_eq!(b.tracks[0].notes.len(), 3, "音が消えた: {:?}", b.tracks[0].notes);
    }

    #[test]
    fn unfinished_notes_are_closed() {
        // 終了が書かれていないファイル。閉じて返すこと
        let mut trk = Vec::new();
        push_varlen(&mut trk, 0);
        trk.extend_from_slice(&[0x90, 60, 100]);
        // 終了を書かずに、960 tick 後にトラックを閉じる
        meta(&mut trk, 960, 0x2f, &[]);
        let mut f = Vec::new();
        let mut head = Vec::new();
        head.extend_from_slice(&0u16.to_be_bytes());
        head.extend_from_slice(&1u16.to_be_bytes());
        head.extend_from_slice(&TPQ.to_be_bytes());
        push_chunk(&mut f, b"MThd", &head);
        push_chunk(&mut f, b"MTrk", &trk);
        let smf = read(&f).unwrap();
        assert_eq!(smf.tracks[0].notes.len(), 1);
        assert_eq!(smf.tracks[0].notes[0].len, 960);
    }

    #[test]
    fn garbage_is_refused_with_a_reason() {
        for (bytes, what) in [
            (b"".to_vec(), "空"),
            ("これは MIDI ではない".as_bytes().to_vec(), "別のもの"),
            (b"MThd\x00\x00\x00\x06\x00\x02\x00\x01\x01\xe0".to_vec(), "形式 2"),
        ] {
            let e = read(&bytes).unwrap_err();
            assert!(!e.is_empty(), "理由が無い: {what}");
        }
    }

    #[test]
    fn extra_chunks_are_skipped() {
        // 知らない塊が混ざっていても飛ばして読む決まり
        let mut f = Vec::new();
        let mut head = Vec::new();
        head.extend_from_slice(&0u16.to_be_bytes());
        head.extend_from_slice(&2u16.to_be_bytes());
        head.extend_from_slice(&TPQ.to_be_bytes());
        push_chunk(&mut f, b"MThd", &head);
        push_chunk(&mut f, b"XYZW", b"knowhere");
        let mut trk = Vec::new();
        push_varlen(&mut trk, 0);
        trk.extend_from_slice(&[0x90, 60, 100]);
        push_varlen(&mut trk, 480);
        trk.extend_from_slice(&[0x80, 60, 0]);
        meta(&mut trk, 0, 0x2f, &[]);
        push_chunk(&mut f, b"MTrk", &trk);
        let smf = read(&f).expect("読めるはず");
        assert_eq!(smf.tracks.len(), 1);
    }

    #[test]
    fn empty_song_writes_something_readable() {
        let smf = Smf { title: "空".into(), ..Default::default() };
        let b = read(&write(&smf)).expect("読めるはず");
        assert_eq!(b.title, "空");
        assert!(b.tracks.is_empty());
        assert_eq!(b.tempos, vec![(0, 120.0)], "テンポの既定が入っていない");
    }
}
