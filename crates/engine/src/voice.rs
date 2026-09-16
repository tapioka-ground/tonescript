//! 鳴っている音1本。
//!
//! 作り方の要
//! ----------
//! 1本の音符は**先に丸ごと作ってから**音側へ渡す。音側は足し込むだけ。
//!
//! 46種の音色を「1サンプルずつ進む状態機械」に書き直す道もあったが、
//! そうすると書き出しと再生で音を作る所が2つになる。**2つあれば必ず
//! ずれる。** 作る所は [`tonescript_render::render_note`] 1つに保って、
//! 締め切りは「先に作っておく」ほうで守る。
//!
//! 1本は 1ミリ秒未満でできる（920本で 183ms）。先回りは 300ミリ秒あれば
//! 足りる。
//!
//! 押しっぱなしの鍵はどうするか
//! ----------------------------
//! 譜面の音符は長さが決まっているが、**鍵盤は離すまで長さが分からない。**
//! 押した瞬間に「6秒ぶん」を作ると、ピアノで 52ミリ秒掛かる。鍵盤としては
//! 遅すぎる（指と音がずれて弾けない）。
//!
//! そこで**少しずつ作り足す**。まず 0.3 秒ぶんを作って鳴らし、鳴っている
//! あいだに次の長いぶんを作って、途中で差し替える。
//!
//! ```text
//!   押す
//!    │  0.3秒ぶん ──────┐
//!    │                  ├ここで差し替え
//!    │      1.2秒ぶん ──┴──────────────┐
//!    │                                 ├ここで差し替え
//!    │           3.6秒ぶん ────────────┴───────────→
//! ```
//!
//! 差し替えられるのは、**短く作った音と長く作った音が途中まで同じだから。**
//! 違うのは終わりの減衰だけなので、そこへ入る前に乗り換えれば繋がる。
//! 実測で、乗り換え位置での違いは信号より 48dB 以上小さい（多くの音色では
//! 1ビットも違わない）。念のため 2ミリ秒だけ重ねて渡す。

use std::sync::Arc;

use tonescript_dsp::osc::SR;
use tonescript_dsp::patch::Cfg;
use tonescript_song::model::Note;
use tonescript_song::Song;

/// 鳴っている音1本。
#[derive(Debug)]
pub struct Voice {
    /// どのパートへ足すか（[`crate::plan::Plan`] の何番目か）
    pub part: usize,
    /// 曲の頭から何サンプル目で鳴り始めるか
    pub start: u64,
    /// 音そのもの
    pub buf: Vec<f32>,
    /// どこまで鳴らしたか
    pub done: usize,
    /// どの代のものか。頭出しすると代が変わり、古いものは捨てる
    pub gen: u64,
    /// 今すぐ鳴らす音か。
    ///
    /// 譜面の音は「曲のどこか」に居るので、止めれば黙る。鍵を押した音は
    /// **止まっていても鳴らないと困る**ので、止まらない時計のほうへ乗せる。
    pub live: bool,
    /// どの音程か。鍵を離したときに探すため
    pub pitch: i32,
    /// 離された。`(掛ける長さ, 残り)` サンプル。
    /// **ぶつ切りにすると必ずプツッと鳴る**ので、短く下げて消す
    pub off: Option<(u32, u32)>,
    /// 頭を上げながら入る長さ（サンプル）。作り足しの継ぎ目で使う
    pub fade_in: u32,
    /// `(この位置から, 何サンプルで)` 下げて終わる。作り足しの継ぎ目で使う
    pub cut: Option<(u64, u32)>,
}

/// 鍵を離してから消えるまで。
pub const RELEASE: f32 = 0.025;

/// 作り足しの継ぎ目で重ねる長さ。
pub const XFADE: f32 = 0.002;

impl Voice {
    /// その絶対位置での倍率。1.0 なら素通し。
    ///
    /// 頭の立ち上がり・継ぎ目の下がり・離したときの下がりを、まとめてここで見る。
    #[inline]
    pub fn gain_at(&self, at: u64) -> f32 {
        let mut g = 1.0f32;
        if self.fade_in > 0 {
            let i = at.saturating_sub(self.start);
            if i < self.fade_in as u64 {
                g *= i as f32 / self.fade_in as f32;
            }
        }
        if let Some((from, span)) = self.cut {
            if at >= from {
                let i = at - from;
                g *= if i >= span as u64 { 0.0 } else { 1.0 - i as f32 / span as f32 };
            }
        }
        g
    }

    /// 継ぎ目。ここから下げて終わる。
    pub fn cut_at(&mut self, at: u64) {
        if self.cut.is_none() {
            self.cut = Some((at, (XFADE * SR) as u32));
        }
    }

    /// 離す。すでに離してあるなら何もしない。
    pub fn release(&mut self) {
        if self.off.is_none() {
            let n = (RELEASE * SR) as u32;
            self.off = Some((n, n));
        }
    }

    pub fn end(&self) -> u64 {
        self.start + self.buf.len() as u64
    }

    /// 鳴らし終わったか。
    pub fn finished(&self) -> bool {
        self.done >= self.buf.len()
    }
}

/// 譜面の音符1本を音にする。
///
/// `part` がドラムなら打楽器、それ以外は `SECTION_PATCH` などで決まる音色。
pub fn render(song: &Song, part: &str, note: &Note, times: &[f64]) -> Option<Vec<f32>> {
    let at = |step: u32| -> usize {
        let i = (step as usize).min(times.len().saturating_sub(1));
        (times.get(i).copied().unwrap_or(0.0) * SR as f64) as usize
    };
    let s = at(note.pos);
    let e = at(note.pos + note.len.max(1)).max(s + 1);
    let ring = if part == "drums" || part == "perc" {
        0.0
    } else {
        tonescript_render::arrange::patch_for(song, part, 1)
            .map(|p| tonescript_dsp::patch::ring(&p))
            .unwrap_or(0.0)
    };
    tonescript_render::render_note(song, &Cfg::default(), part, note, s, e, ring).map(|(_, w)| w)
}

/// 鍵を押したときの1本。
///
/// 譜面に無い音を、その場で鳴らす。長さは秒で受ける（押しっぱなしの
/// 扱いはまだ無い。押した長さぶんを最初に決めて鳴らし切る）。
pub fn render_live(song: &Song, part: &str, pitch: i32, vel: u8, secs: f32) -> Option<Vec<f32>> {
    let n = ((secs.max(0.01) * SR) as usize).max(1);
    let note = Note { pos: 0, len: 1, pitch, vel, mora: String::new() };
    let ring = if part == "drums" || part == "perc" {
        0.0
    } else {
        tonescript_render::arrange::patch_for(song, part, 1)
            .map(|p| tonescript_dsp::patch::ring(&p))
            .unwrap_or(0.0)
    };
    tonescript_render::render_note(song, &Cfg::default(), part, &note, 0, n, ring).map(|(_, w)| w)
}

/// 音側へ渡すもの。
#[derive(Debug)]
pub enum Msg {
    /// 鳴らす音1本
    Voice(Voice),
    /// 設定の差し替え
    Plan(Arc<crate::plan::Plan>),
    /// この代より古い音を捨てる（頭出し・曲の差し替え）
    Flush(u64),
    /// キックがここで鳴る。サイドチェインを凹ませる合図
    Kick(u64),
    /// 鍵を離した。そのパートのその音程を、短く下げて消す
    Off { part: usize, pitch: i32 },
    /// 作り足した。古いほうをこの位置で終わらせる
    Cut { part: usize, pitch: i32, at: u64 },
    /// トラックごとの尖り止めの閾値 `(始まり, 天井)`。パートの順に並ぶ。
    /// 曲まるごとの実効値から決まるので、裏で測ってから届く
    Trim(Vec<Option<(f32, f32)>>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        tonescript_song::load_str(
            r#"let BPM = 120;
               let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
               let VOICES = #{ lead: #{ ch: 0, patch: "piano" },
                               drums: #{ ch: 9 } };
            "#,
        )
        .unwrap()
    }

    fn times(s: &Song) -> Vec<f64> {
        tonescript_render::arrange::step_times(s)
    }

    #[test]
    fn a_note_becomes_sound() {
        let s = song();
        let t = times(&s);
        let n = Note { pos: 0, len: 4, pitch: 60, vel: 100, mora: String::new() };
        let w = render(&s, "lead", &n, &t).expect("鳴るはず");
        assert!(!w.is_empty());
        assert!(w.iter().any(|v| v.abs() > 0.01), "音が入っていない");
        // 4目盛り = 0.5秒。ピアノの余韻 0.42秒ぶん長い
        let want = (0.5 + 0.42) * SR;
        assert!((w.len() as f32 - want).abs() < SR * 0.05, "長さが {}", w.len());
    }

    #[test]
    fn the_same_note_sounds_the_same_every_time() {
        // 同じ音符は何度作っても同じ音。でないと再生のたびに音が変わる
        let s = song();
        let t = times(&s);
        let n = Note { pos: 8, len: 4, pitch: 67, vel: 100, mora: String::new() };
        let a = render(&s, "lead", &n, &t).unwrap();
        let b = render(&s, "lead", &n, &t).unwrap();
        assert_eq!(a, b, "同じ音符が違う音になった");
    }

    #[test]
    fn drums_go_by_midi_number() {
        let s = song();
        let t = times(&s);
        let kick = Note { pos: 0, len: 1, pitch: 36, vel: 120, mora: String::new() };
        let hat = Note { pos: 0, len: 1, pitch: 42, vel: 120, mora: String::new() };
        let a = render(&s, "drums", &kick, &t).unwrap();
        let b = render(&s, "drums", &hat, &t).unwrap();
        assert_ne!(a, b, "キックとハットが同じ音になった");
        assert!(a.iter().any(|v| v.abs() > 0.01));
    }

    #[test]
    fn a_live_note_lasts_what_it_was_asked_for() {
        let s = song();
        let w = render_live(&s, "lead", 69, 100, 0.25).expect("鳴るはず");
        // 頼んだ 0.25 秒 + 余韻 0.42 秒
        let want = (0.25 + 0.42) * SR;
        assert!((w.len() as f32 - want).abs() < SR * 0.05, "長さが {}", w.len());
        assert!(w.iter().any(|v| v.abs() > 0.01), "音が入っていない");
    }

    #[test]
    fn a_part_with_no_instrument_is_silent_not_a_crash() {
        let s = song();
        let t = times(&s);
        let n = Note { pos: 0, len: 4, pitch: 60, vel: 100, mora: String::new() };
        assert!(render(&s, "nosuch", &n, &t).is_none(), "無い音色で鳴ってしまった");
        assert!(render_live(&s, "nosuch", 60, 100, 0.2).is_none());
    }

    #[test]
    fn a_voice_knows_when_it_is_done() {
        let mut v = Voice {
            part: 0,
            start: 100,
            buf: vec![0.0; 10],
            done: 0,
            gen: 1,
            live: false,
            pitch: 60,
            off: None,
            fade_in: 0,
            cut: None,
        };
        assert_eq!(v.end(), 110);
        assert!(!v.finished());
        v.done = 10;
        assert!(v.finished());
    }

    #[test]
    fn the_seam_hands_over_without_a_gap() {
        // 古いほうが下がりきるのと、新しいほうが上がりきるのが同じ長さ。
        // 足して 1.0 のままなので、継ぎ目で膨らみも凹みもしない
        let span = (XFADE * SR) as u32;
        let mut old = Voice {
            part: 0,
            start: 0,
            buf: vec![1.0; 100_000],
            done: 0,
            gen: 1,
            live: true,
            pitch: 60,
            off: None,
            fade_in: 0,
            cut: None,
        };
        old.cut_at(1000);
        let new = Voice { start: 1000, fade_in: span, ..Voice {
            part: 0,
            start: 0,
            buf: vec![1.0; 100_000],
            done: 0,
            gen: 1,
            live: true,
            pitch: 60,
            off: None,
            fade_in: 0,
            cut: None,
        }};
        for i in 0..span as u64 {
            let sum = old.gain_at(1000 + i) + new.gain_at(1000 + i);
            assert!((sum - 1.0).abs() < 1e-6, "継ぎ目で {sum} になった（{i} 目）");
        }
        assert_eq!(old.gain_at(1000 + span as u64), 0.0, "古いほうが残った");
        assert_eq!(new.gain_at(1000 + span as u64), 1.0, "新しいほうが上がりきっていない");
        // 継ぎ目の前は古いほうがそのまま
        assert_eq!(old.gain_at(999), 1.0);
    }

    #[test]
    fn cutting_twice_keeps_the_first_seam() {
        let mut v = Voice {
            part: 0,
            start: 0,
            buf: vec![1.0; 10],
            done: 0,
            gen: 1,
            live: true,
            pitch: 60,
            off: None,
            fade_in: 0,
            cut: None,
        };
        v.cut_at(100);
        v.cut_at(200);
        assert_eq!(v.cut.unwrap().0, 100, "継ぎ目が後ろへずれた");
    }

    #[test]
    fn releasing_twice_does_not_restart_the_fade() {
        let mut v = Voice {
            part: 0,
            start: 0,
            buf: vec![0.0; 100_000],
            done: 0,
            gen: 1,
            live: true,
            pitch: 60,
            off: None,
            fade_in: 0,
            cut: None,
        };
        v.release();
        let first = v.off;
        assert!(first.is_some());
        // 半分まで進んだことにして、もう一度離す
        v.off = Some((first.unwrap().0, first.unwrap().1 / 2));
        let half = v.off;
        v.release();
        assert_eq!(v.off, half, "離し直しで音が戻った");
    }
}
