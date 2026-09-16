//! 鍵盤から弾く。
//!
//! 3つに分けてある。
//!
//! - [`parse`] — 届いたバイト列を読む。MIDI の決まりだけ
//! - [`Keyboard`] — 押した・離した・ペダルを、鳴らす／止めるへ変える
//! - [`Input`] — 実際の口を開ける（`midir`）
//!
//! 上の2つは機械が要らないので試験できる。**機械が要るのは最後だけ。**
//!
//! ペダルのこと
//! ------------
//! ダンパーペダルを踏んでいるあいだは、指を離しても音を止めない。離すのは
//! ペダルを上げたとき。これを入れないと、ペダルを使う人は「弾けない」と
//! 感じる（実際、鍵盤で最初に触るのはここ）。

use std::sync::mpsc::{channel, Receiver, Sender};

/// 鍵盤から届いたこと。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    /// 押した
    On { ch: u8, pitch: i32, vel: u8 },
    /// 離した
    Off { ch: u8, pitch: i32 },
    /// ダンパーペダル
    Sustain(bool),
    /// 全部止めろ（曲の切り替えや、繋ぎ直しのとき来る）
    AllOff,
}

/// 届いたバイト列を読む。読めないものは `None`（黙って捨てる）。
///
/// 機械によっては「速さ 0 の押した」を「離した」の代わりに送ってくる。
/// **これを見落とすと音が鳴りっぱなしになる。**
pub fn parse(msg: &[u8]) -> Option<Ev> {
    let status = *msg.first()?;
    let ch = status & 0x0f;
    match status & 0xf0 {
        0x90 => {
            let (pitch, vel) = (*msg.get(1)? as i32, *msg.get(2)?);
            if vel == 0 {
                Some(Ev::Off { ch, pitch })
            } else {
                Some(Ev::On { ch, pitch, vel })
            }
        }
        0x80 => Some(Ev::Off { ch, pitch: *msg.get(1)? as i32 }),
        0xb0 => match *msg.get(1)? {
            // 64 = ダンパーペダル。64 以上で踏んでいる
            64 => Some(Ev::Sustain(*msg.get(2)? >= 64)),
            // 120 = 音を切れ、123 = 鍵を全部離した扱いにしろ
            120 | 123 => Some(Ev::AllOff),
            _ => None,
        },
        _ => None,
    }
}

/// 鳴らす側への指示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    On { pitch: i32, vel: u8 },
    Off { pitch: i32 },
}

/// 鍵盤の今。押されている音とペダルを覚える。
#[derive(Default, Debug)]
pub struct Keyboard {
    sustain: bool,
    /// 指で押さえている音
    held: Vec<i32>,
    /// 指は離れたが、ペダルで残っている音
    sustained: Vec<i32>,
}

impl Keyboard {
    /// 届いたことを、鳴らす／止めるへ変える。
    pub fn apply(&mut self, ev: Ev) -> Vec<Act> {
        match ev {
            Ev::On { pitch, vel, .. } => {
                // 同じ音を押し直したら、前のを止めてから鳴らす。
                // 重ねると倍の大きさになって割れる
                let mut acts = Vec::new();
                if self.held.contains(&pitch) || self.sustained.contains(&pitch) {
                    acts.push(Act::Off { pitch });
                }
                self.sustained.retain(|p| *p != pitch);
                if !self.held.contains(&pitch) {
                    self.held.push(pitch);
                }
                acts.push(Act::On { pitch, vel });
                acts
            }
            Ev::Off { pitch, .. } => {
                self.held.retain(|p| *p != pitch);
                if self.sustain {
                    // ペダルを踏んでいる。まだ止めない
                    if !self.sustained.contains(&pitch) {
                        self.sustained.push(pitch);
                    }
                    Vec::new()
                } else {
                    vec![Act::Off { pitch }]
                }
            }
            Ev::Sustain(down) => {
                self.sustain = down;
                if down {
                    return Vec::new();
                }
                // 上げた。指が離れているものを今まとめて止める
                let acts = self.sustained.iter().map(|p| Act::Off { pitch: *p }).collect();
                self.sustained.clear();
                acts
            }
            Ev::AllOff => {
                let mut acts: Vec<Act> = Vec::new();
                for p in self.held.drain(..).chain(self.sustained.drain(..)) {
                    if !acts.contains(&Act::Off { pitch: p }) {
                        acts.push(Act::Off { pitch: p });
                    }
                }
                self.sustain = false;
                acts
            }
        }
    }

    /// 今いくつ押されているか。画面に出す。
    pub fn held_count(&self) -> usize {
        self.held.len() + self.sustained.len()
    }

    /// 全部離したことにして、止める指示を返す。曲を切り替えるときなど。
    pub fn panic(&mut self) -> Vec<Act> {
        self.apply(Ev::AllOff)
    }
}

/// 鍵盤の口。開けられなくても、画面と書き出しは動く。
pub struct Input {
    /// 持っているあいだだけ繋がっている
    conn: Option<midir::MidiInputConnection<Sender<Ev>>>,
    rx: Receiver<Ev>,
    /// 繋がっている機械の名前
    pub name: Option<String>,
    /// 開けなかった理由
    pub error: Option<String>,
}

impl Input {
    /// 何も繋がっていない状態。
    pub fn idle() -> Input {
        let (_tx, rx) = channel();
        Input { conn: None, rx, name: None, error: None }
    }

    /// 今ぶら下がっている機械の名前を並べる。
    pub fn ports() -> Vec<String> {
        let Ok(m) = midir::MidiInput::new("tonescript") else { return Vec::new() };
        m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect()
    }

    /// `want` 番目の機械へ繋ぐ。`None` なら最初の1つ。
    pub fn open(want: Option<usize>) -> Input {
        let (tx, rx) = channel();
        let mut me = Input { conn: None, rx, name: None, error: None };
        let mut input = match midir::MidiInput::new("tonescript") {
            Ok(m) => m,
            Err(e) => {
                me.error = Some(e.to_string());
                return me;
            }
        };
        // 届いたものを取りこぼさないよう、押し合いへし合いは許す
        input.ignore(midir::Ignore::None);
        let ports = input.ports();
        let Some(port) = ports.get(want.unwrap_or(0)) else {
            me.error = Some("鍵盤が見つかりません".into());
            return me;
        };
        me.name = input.port_name(port).ok();
        let port = port.clone();
        match input.connect(
            &port,
            "tonescript-in",
            |_t, msg, tx: &mut Sender<Ev>| {
                // ここは機械の側のスレッド。**何もせずに投げるだけ。**
                if let Some(ev) = parse(msg) {
                    let _ = tx.send(ev);
                }
            },
            tx,
        ) {
            Ok(c) => me.conn = Some(c),
            Err(e) => me.error = Some(e.to_string()),
        }
        me
    }

    pub fn is_open(&self) -> bool {
        self.conn.is_some()
    }

    /// 溜まっているぶんを取る。**待たない。**
    pub fn drain(&self) -> Vec<Ev> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
            if out.len() >= 256 {
                break;
            }
        }
        out
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::idle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_and_off_are_read() {
        assert_eq!(parse(&[0x90, 60, 100]), Some(Ev::On { ch: 0, pitch: 60, vel: 100 }));
        assert_eq!(parse(&[0x80, 60, 0]), Some(Ev::Off { ch: 0, pitch: 60 }));
        // チャンネルは下位4ビット
        assert_eq!(parse(&[0x95, 64, 90]), Some(Ev::On { ch: 5, pitch: 64, vel: 90 }));
        assert_eq!(parse(&[0x89, 38, 64]), Some(Ev::Off { ch: 9, pitch: 38 }));
    }

    #[test]
    fn a_note_on_with_zero_speed_means_off() {
        // ここを見落とすと音が鳴りっぱなしになる
        assert_eq!(parse(&[0x90, 60, 0]), Some(Ev::Off { ch: 0, pitch: 60 }));
    }

    #[test]
    fn the_pedal_and_the_panic_buttons_are_read() {
        assert_eq!(parse(&[0xb0, 64, 127]), Some(Ev::Sustain(true)));
        assert_eq!(parse(&[0xb0, 64, 64]), Some(Ev::Sustain(true)));
        assert_eq!(parse(&[0xb0, 64, 63]), Some(Ev::Sustain(false)));
        assert_eq!(parse(&[0xb0, 64, 0]), Some(Ev::Sustain(false)));
        assert_eq!(parse(&[0xb0, 120, 0]), Some(Ev::AllOff));
        assert_eq!(parse(&[0xb0, 123, 0]), Some(Ev::AllOff));
    }

    #[test]
    fn junk_is_thrown_away_not_guessed() {
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0x90]), None, "足りないのに読んだ");
        assert_eq!(parse(&[0x90, 60]), None, "速さが無いのに読んだ");
        assert_eq!(parse(&[0xf8]), None, "時計の合図を音にした");
        assert_eq!(parse(&[0xe0, 0, 64]), None, "まだ扱わないものを読んだ");
        assert_eq!(parse(&[0xb0, 1, 100]), None, "扱わない CC を読んだ");
    }

    #[test]
    fn pressing_and_releasing_sounds_and_stops() {
        let mut k = Keyboard::default();
        assert_eq!(k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 }), vec![Act::On { pitch: 60, vel: 100 }]);
        assert_eq!(k.held_count(), 1);
        assert_eq!(k.apply(Ev::Off { ch: 0, pitch: 60 }), vec![Act::Off { pitch: 60 }]);
        assert_eq!(k.held_count(), 0);
    }

    #[test]
    fn the_pedal_holds_the_sound_until_it_comes_up() {
        let mut k = Keyboard::default();
        k.apply(Ev::Sustain(true));
        k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 });
        k.apply(Ev::On { ch: 0, pitch: 64, vel: 100 });
        // 指を離しても止まらない
        assert!(k.apply(Ev::Off { ch: 0, pitch: 60 }).is_empty(), "ペダル中に止まった");
        assert!(k.apply(Ev::Off { ch: 0, pitch: 64 }).is_empty());
        assert_eq!(k.held_count(), 2, "ペダルで残っているぶんが数えられていない");
        // 上げたらまとめて止まる
        let acts = k.apply(Ev::Sustain(false));
        assert_eq!(acts.len(), 2, "上げても止まらない: {acts:?}");
        assert!(acts.contains(&Act::Off { pitch: 60 }));
        assert!(acts.contains(&Act::Off { pitch: 64 }));
        assert_eq!(k.held_count(), 0);
    }

    #[test]
    fn a_key_still_down_when_the_pedal_lifts_keeps_sounding() {
        let mut k = Keyboard::default();
        k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 });
        k.apply(Ev::Sustain(true));
        // 押さえたまま上げた。まだ止めない
        assert!(k.apply(Ev::Sustain(false)).is_empty(), "押さえているのに止まった");
        assert_eq!(k.held_count(), 1);
        assert_eq!(k.apply(Ev::Off { ch: 0, pitch: 60 }), vec![Act::Off { pitch: 60 }]);
    }

    #[test]
    fn hitting_the_same_key_again_stops_the_old_one_first() {
        // 重ねると倍の大きさになって割れる
        let mut k = Keyboard::default();
        k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 });
        let acts = k.apply(Ev::On { ch: 0, pitch: 60, vel: 120 });
        assert_eq!(acts, vec![Act::Off { pitch: 60 }, Act::On { pitch: 60, vel: 120 }]);
        assert_eq!(k.held_count(), 1, "同じ音が2つ数えられている");
    }

    #[test]
    fn a_pedalled_note_replayed_is_not_counted_twice() {
        let mut k = Keyboard::default();
        k.apply(Ev::Sustain(true));
        k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 });
        k.apply(Ev::Off { ch: 0, pitch: 60 });
        k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 });
        assert_eq!(k.held_count(), 1, "ペダルの残りと押し直しが二重になった");
    }

    #[test]
    fn all_off_stops_everything_once_each() {
        let mut k = Keyboard::default();
        k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 });
        k.apply(Ev::Sustain(true));
        k.apply(Ev::On { ch: 0, pitch: 64, vel: 100 });
        k.apply(Ev::Off { ch: 0, pitch: 64 });
        let acts = k.apply(Ev::AllOff);
        assert_eq!(acts.len(), 2, "止め漏れか、二重に止めている: {acts:?}");
        assert_eq!(k.held_count(), 0);
        // ペダルも上がっていること
        k.apply(Ev::On { ch: 0, pitch: 60, vel: 100 });
        assert_eq!(k.apply(Ev::Off { ch: 0, pitch: 60 }), vec![Act::Off { pitch: 60 }]);
    }

    #[test]
    fn an_unopened_input_is_quiet_not_broken() {
        let i = Input::idle();
        assert!(!i.is_open());
        assert!(i.drain().is_empty());
        assert!(i.error.is_none());
    }
}
