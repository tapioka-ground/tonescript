//! 先回りして音を作る係。
//!
//! 音を出す側は締め切りに追われているので、音を作る仕事はここへ寄せる。
//! やることは1つ：**再生の頭より少し先の音符を、鳴る前に作って渡す。**
//!
//! ```text
//!   今ここ        ここまで作っておく
//!     │<─── 先回り 0.3 秒 ───>│
//!   ──┴───────────────────────┴──────────────→ 曲
//! ```
//!
//! 0.3 秒あれば、1本 1ミリ秒未満で作れる音符は 300 本ぶん入る。実際の曲で
//! 0.3 秒に 300 本は来ない。
//!
//! 頭出しされたら
//! --------------
//! 「代」が1つ進む。ここは音符を数え直し、音側は古い代の音を捨てる。
//! どちらも相手を待たずに、自分の側だけで辻褄が合う。

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use tonescript_dsp::osc::SR;
use tonescript_render::Score;
use tonescript_song::model::Note;
use tonescript_song::Song;

use crate::mixer::Shared;
use crate::plan::Plan;
use crate::ring::{Rx, Tx};
use crate::voice::{self, Msg, Voice};

/// どれだけ先まで作っておくか。
const LOOKAHEAD: f64 = 0.30;

/// 画面側から係への頼みごと。
pub enum Cmd {
    /// 曲を差し替える
    Song(Arc<Song>, Arc<Score>),
    /// 設定だけ差し替える（フェーダーを動かした等）
    Plan(Arc<Plan>),
    /// 今すぐ鳴らす（鍵を押した・音符を置いた）
    Live { part: String, pitch: i32, vel: u8, secs: f32 },
    Quit,
    /// 音圧と尖り止めを測り終えた（裏の測定係から戻ってくる）
    Measured { makeup: f32, trim: Vec<Option<(f32, f32)>> },
}

struct Part {
    name: String,
    notes: Vec<Note>,
    /// 次に出す音符
    next: usize,
}

struct Sched {
    song: Option<Arc<Song>>,
    plan: Arc<Plan>,
    parts: Vec<Part>,
    tx: Tx<Msg>,
    gc: Rx<Voice>,
    shared: Arc<Shared>,
    /// 出しそびれ。輪がいっぱいだったぶんを次の回へ持ち越す
    pending: Vec<Msg>,
    gen: u64,
    /// ここまで作った（サンプル）
    upto: u64,
    /// 前に見た再生位置。**戻っていたら**繰り返しの折り返し
    last_pos: u64,
    /// 音圧の測定を頼む先
    back: Sender<Cmd>,
}

impl Sched {
    /// 曲を入れ替える。譜面をパートごとに並べ直して数え直す。
    fn set_song(&mut self, song: Arc<Song>, score: Arc<Score>) {
        let mut names: Vec<String> = score.keys().cloned().collect();
        names.sort();
        let plan = Plan::from_song(&song, &names);
        self.parts = names
            .iter()
            .map(|n| {
                let mut notes = score[n].clone();
                notes.sort_by_key(|x| x.pos);
                Part { name: n.clone(), notes, next: 0 }
            })
            .collect();
        self.plan = Arc::new(plan);
        self.song = Some(song.clone());
        self.send(Msg::Plan(self.plan.clone()));
        self.restart();
        self.measure(song, score, names);
    }

    /// 書き出したときと同じ音で聞けるように、裏で1回だけ測る。
    ///
    /// 「先頭から順に鳴らす」側からは見えない値が2つある。
    ///
    /// - **音圧合わせ（LUFS）** — 曲まるごとの大きさから決まる
    /// - **トラックごとの尖り止め** — トラックまるごとの実効値から決まる
    ///
    /// どちらも曲を1回作れば出る。作るのに 0.24 秒しか掛からないので、
    /// 読み込んだ裏で1回走らせて、出た数字を音側へ渡す。
    /// **速いからこそ取れる手。**
    fn measure(&self, song: Arc<Song>, score: Arc<Score>, names: Vec<String>) {
        let back = self.back.clone();
        let target = song.master_lufs;
        std::thread::spawn(move || {
            let quiet = |_: &str| {};
            let mut stems = tonescript_render::render_stems(&song, &score, &quiet);
            // 尖り止めは「実効値の何倍まで許すか」。打楽器は頭が命なので掛けない
            let trim: Vec<Option<(f32, f32)>> = names
                .iter()
                .map(|n| {
                    if n == "drums" || n == "perc" {
                        return None;
                    }
                    let buf = stems.get(n)?;
                    let r = tonescript_dsp::rms(buf);
                    if r <= 1e-9 {
                        return None;
                    }
                    let ceiling = r * 14.0;
                    Some((ceiling * 0.72, ceiling))
                })
                .collect();
            let out = tonescript_render::mix_down(&song, &mut stems, &score, &quiet);
            let now = tonescript_render::mix::lufs(&out.l, &out.r);
            let makeup =
                if now.is_finite() { 10f32.powf((target - now) / 20.0).clamp(0.05, 20.0) } else { 1.0 };
            let _ = back.send(Cmd::Measured { makeup, trim });
        });
    }

    fn measured(&mut self, makeup: f32, trim: Vec<Option<(f32, f32)>>) {
        // 音側は共有の数字として読む。画面が設定を差し替えても消えない
        self.shared.set_makeup(makeup);
        self.send(Msg::Trim(trim));
    }

    /// 数え直す。頭出し・曲の差し替え・繰り返しの折り返しで呼ぶ。
    fn restart(&mut self) {
        self.gen = self.shared.gen.load(Ordering::Relaxed);
        let pos = self.shared.pos.load(Ordering::Relaxed);
        self.upto = pos;
        self.last_pos = pos;
        let plan = self.plan.clone();
        for p in &mut self.parts {
            // その位置以降の最初の音符を探す
            p.next = p.notes.partition_point(|n| plan.sample_of(n.pos) < pos);
        }
        self.pending.clear();
    }

    /// 渡す。いっぱいなら持ち越す。
    fn send(&mut self, m: Msg) {
        if let Err(m) = self.tx.push(m) {
            if self.pending.len() < 4096 {
                self.pending.push(m);
            }
        }
    }

    /// 持ち越したぶんを先に流す。
    fn flush_pending(&mut self) {
        while let Some(m) = self.pending.pop() {
            if let Err(m) = self.tx.push(m) {
                self.pending.push(m);
                return;
            }
        }
    }

    /// 先回りぶんを作る。
    fn fill_ahead(&mut self) {
        let Some(song) = self.song.clone() else { return };
        let plan = self.plan.clone();
        let pos = self.shared.pos.load(Ordering::Relaxed);
        // 折り返した（繰り返しで頭へ戻った）ら数え直す。
        //
        // **「作った先」ではなく「前に見た位置」と比べること。** 作った先は
        // いつも再生位置より前にあるので、そちらと比べると毎回「戻った」と
        // 見なして数え直し、同じ音符を何度も出してしまう（音が重なって割れる）
        if pos + 16 < self.last_pos {
            self.restart();
        }
        self.last_pos = pos;
        let until = (pos + (LOOKAHEAD * SR as f64) as u64).max(self.upto);
        if until <= self.upto && self.upto > pos {
            return;
        }
        let times = plan.step_times.clone();
        for pi in 0..self.parts.len() {
            loop {
                let (name, note) = {
                    let p = &self.parts[pi];
                    let Some(n) = p.notes.get(p.next) else { break };
                    (p.name.clone(), n.clone())
                };
                let at = plan.sample_of(note.pos);
                if at > until {
                    break;
                }
                self.parts[pi].next += 1;
                if at + ((8.0 * SR) as u64) < pos {
                    continue; // とっくに過ぎている
                }
                if let Some(buf) = voice::render(&song, &name, &note, &times) {
                    if (name == "drums" || name == "perc")
                        && (note.pitch == 35 || note.pitch == 36)
                    {
                        // キックはサイドチェインの合図でもある
                        self.send(Msg::Kick(at));
                    }
                    self.send(Msg::Voice(Voice {
                        part: pi,
                        start: at,
                        buf,
                        done: 0,
                        gen: self.gen,
                        live: false,
                    }));
                }
            }
        }
        self.upto = until;
    }

    /// 今すぐ鳴らす。画面で音符を触ったときと、鍵を押したとき。
    fn live(&mut self, part: &str, pitch: i32, vel: u8, secs: f32) {
        let Some(song) = self.song.clone() else { return };
        let Some(pi) = self.plan.part_of(part) else { return };
        let Some(buf) = voice::render_live(&song, part, pitch, vel, secs) else { return };
        // 「今」より少しだけ後ろへ置く。今ちょうどの所へ置くと、
        // 音側が既に通り過ぎていて頭が欠ける
        let at = self.shared.clock.load(Ordering::Relaxed) + (0.005 * SR) as u64;
        self.send(Msg::Voice(Voice {
            part: pi,
            start: at,
            buf,
            done: 0,
            gen: self.gen,
            live: true,
        }));
    }

    /// 返ってきた音を捨てる。**捨てるのはこちら側の仕事。**
    fn collect(&self) {
        while self.gc.pop().is_some() {}
    }
}

/// 係を始める。画面側は返ってきた口へ頼みごとを送る。
pub(crate) fn spawn(
    shared: Arc<Shared>,
    tx: Tx<Msg>,
    gc: Rx<Voice>,
    rx: Receiver<Cmd>,
    back: Sender<Cmd>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("tonescript-sched".into())
        .spawn(move || {
            let mut s = Sched {
                song: None,
                plan: Arc::new(Plan::empty()),
                parts: Vec::new(),
                tx,
                gc,
                shared,
                pending: Vec::new(),
                gen: 1,
                upto: 0,
                last_pos: 0,
                back,
            };
            loop {
                match rx.recv_timeout(Duration::from_millis(4)) {
                    Ok(Cmd::Quit) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Cmd::Song(song, score)) => s.set_song(song, score),
                    Ok(Cmd::Plan(p)) => {
                        s.plan = p.clone();
                        s.send(Msg::Plan(p));
                    }
                    Ok(Cmd::Live { part, pitch, vel, secs }) => s.live(&part, pitch, vel, secs),
                    Ok(Cmd::Measured { makeup, trim }) => s.measured(makeup, trim),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                s.collect();
                // 頭出しされていたら数え直す
                if s.shared.gen.load(Ordering::Relaxed) != s.gen {
                    s.restart();
                }
                s.flush_pending();
                s.fill_ahead();
            }
        })
        .expect("係のスレッドが立てられません")
}

/// 譜面のパート名を並び順で返す。[`Plan`] の番号と合わせるため。
pub fn part_names(score: &Score) -> Vec<String> {
    let mut v: Vec<String> = score.keys().cloned().collect();
    v.sort();
    v
}

/// 譜面に出てくるパートごとのノート数。画面に出すため。
pub fn note_counts(score: &Score) -> HashMap<String, usize> {
    score.iter().map(|(k, v)| (k.clone(), v.len())).collect()
}
