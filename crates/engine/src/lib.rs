//! 鳴らしながら計算する。
//!
//! なぜ作り直したか
//! ----------------
//! 前の作りは「曲まるごとを先に作って、その帯を鳴らす」だった。速いから
//! 成り立っていたのだが、DAW として見ると3つ困る。
//!
//! - 音符を置いても**その場で鳴らない**（作り直すまで聞こえない）
//! - 鳴らしている最中に**フェーダーを動かしても変わらない**
//! - 鍵盤を挿しても**弾いた音が返らない**
//!
//! どれも「先に作る」という形そのものから出ている。そこで、先に作るのは
//! **音符1本ぶん**だけにして、並べて鳴らすのは音側でやる形に変えた。
//!
//! 作りの全体
//! ----------
//! ```text
//!   画面 ──頼みごと──→ 先回り係 ──作った音──→ 音の出口
//!    │                    ↑                      │
//!    └──位置・再生中・代──┴──────────────────────┘
//!                        （共有の数字）
//!                     鳴り終わった音 ────→ 捨てるのは係の側
//! ```
//!
//! - **画面**は待たない。頼みごとを置くだけ
//! - **係**は再生の頭の 0.3 秒先までを作って渡す
//! - **音の出口**は足して整えるだけ。待たない・確保しない・捨てない
//!
//! 音を作る所は書き出しと同じ [`tonescript_render::render_note`] を呼ぶ。
//! **聞いた音と書き出した音が違う、を起こさないため。**

pub mod mixer;
pub mod plan;
pub mod ring;
pub mod sched;
pub mod voice;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

use tonescript_dsp::osc::SR;
use tonescript_render::Score;
use tonescript_song::Song;

pub use mixer::{Mixer, Shared};
pub use plan::Plan;

use sched::Cmd;

/// 画面側が持つ取っ手。
///
/// [`Engine::new`] が返す [`Mixer`] を音の出口へ渡すと鳴り始める。
pub struct Engine {
    shared: Arc<Shared>,
    cmd: Sender<Cmd>,
    live: Arc<AtomicU64>,
    /// 画面側が持つ設定の写し。触ったら丸ごと送り直す
    plan: Plan,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    /// 係を立てて、音側の半分を返す。
    pub fn new() -> (Engine, Mixer) {
        let shared = Arc::new(Shared::default());
        let live = Arc::new(AtomicU64::new(0));
        // 音へ渡す輪と、鳴り終わったものを返す輪
        let (tx, rx) = ring::ring::<voice::Msg>(4096);
        let (gtx, grx) = ring::ring::<voice::Voice>(4096);
        let (cmd, cmd_rx) = channel::<Cmd>();
        let thread = sched::spawn(shared.clone(), tx, grx, cmd_rx, cmd.clone());
        let mixer = Mixer::new(shared.clone(), rx, gtx, live.clone());
        let me = Engine { shared, cmd, live, plan: Plan::empty(), thread: Some(thread) };
        (me, mixer)
    }

    /// 曲を差し替える。位置は頭へ戻す。
    pub fn set_song(&mut self, song: Arc<Song>, score: Arc<Score>) {
        let names = sched::part_names(&score);
        self.plan = Plan::from_song(&song, &names);
        self.shared.pos.store(0, Ordering::Relaxed);
        self.bump();
        let _ = self.cmd.send(Cmd::Song(song, score));
    }

    /// 譜面だけ差し替える（音符を足した・消した）。位置はそのまま。
    ///
    /// 既に鳴り出している音はそのまま鳴り切る。**鳴っている音符を消したら
    /// 途中で切れる**ほうが、鳴り続けるより驚かない。
    pub fn set_score(&mut self, song: Arc<Song>, score: Arc<Score>) {
        let names = sched::part_names(&score);
        self.plan = Plan::from_song(&song, &names);
        self.bump();
        let _ = self.cmd.send(Cmd::Song(song, score));
    }

    /// 代を1つ進める。音側は古い代の音を捨て、係は数え直す。
    fn bump(&self) {
        let g = self.shared.gen.fetch_add(1, Ordering::Relaxed) + 1;
        // 音側へも直接伝える（係の手が空くのを待たない）
        let _ = self.cmd.send(Cmd::Plan(Arc::new(self.plan.clone())));
        let _ = g;
    }

    pub fn play(&self) {
        self.shared.hit_end.store(false, Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// 曲の終わりまで来たか。見たら下ろす。
    pub fn took_end(&self) -> bool {
        self.shared.hit_end.swap(false, Ordering::Relaxed)
    }

    /// 今どこ（サンプル）。
    pub fn position(&self) -> u64 {
        self.shared.pos.load(Ordering::Relaxed)
    }

    pub fn seconds(&self) -> f64 {
        self.position() as f64 / SR as f64
    }

    /// 頭出し。鳴りかけの音は捨てて、そこから作り直す。
    pub fn seek(&self, sample: u64) {
        self.shared.pos.store(sample, Ordering::Relaxed);
        let g = self.shared.gen.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.cmd.send(Cmd::Plan(Arc::new(self.plan.clone())));
        let _ = g;
    }

    /// 目盛りで頭出し。
    pub fn seek_step(&self, step: u32) {
        self.seek(self.plan.sample_of(step));
    }

    /// 繰り返す範囲（サンプル）。`to` が `from` 以下なら繰り返さない。
    pub fn set_loop(&self, from: u64, to: u64) {
        self.shared.loop_from.store(from, Ordering::Relaxed);
        self.shared.loop_to.store(if to > from { to } else { 0 }, Ordering::Relaxed);
    }

    pub fn loop_range(&self) -> Option<(u64, u64)> {
        let to = self.shared.loop_to.load(Ordering::Relaxed);
        (to > 0).then(|| (self.shared.loop_from.load(Ordering::Relaxed), to))
    }

    /// 曲の長さ（サンプル）。
    pub fn total(&self) -> u64 {
        self.plan.total
    }

    /// 今いくつ鳴っているか。
    pub fn voices(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    /// 今すぐ1音鳴らす。音符を置いた・鍵を押した。
    pub fn note_on(&self, part: &str, pitch: i32, vel: u8, secs: f32) {
        let _ = self.cmd.send(Cmd::Live {
            part: part.to_string(),
            pitch,
            vel,
            secs: secs.clamp(0.01, 8.0),
        });
    }

    /// 目盛りぶんの長さで1音鳴らす。今のテンポで秒に直す。
    pub fn note_on_steps(&self, part: &str, pitch: i32, vel: u8, steps: u32) {
        let a = self.plan.time_of(0);
        let b = self.plan.time_of(steps.max(1));
        self.note_on(part, pitch, vel, (b - a).max(0.05) as f32);
    }

    /// 設定を1つ触る。触ったらすぐ音へ届く（**鳴らしたままでいい**）。
    fn tweak(&mut self, f: impl FnOnce(&mut Plan)) {
        f(&mut self.plan);
        let _ = self.cmd.send(Cmd::Plan(Arc::new(self.plan.clone())));
    }

    /// そのパートを鳴らすか（ミュート・ソロの結果）。
    pub fn set_audible(&mut self, part: &str, on: bool) {
        if let Some(i) = self.plan.part_of(part) {
            self.tweak(|p| p.parts[i].audible = on);
        }
    }

    /// ミュートとソロをまとめて反映する。
    pub fn set_mutes(&mut self, muted: &[String], soloed: &[String]) {
        let states: Vec<bool> = self
            .plan
            .parts
            .iter()
            .map(|p| {
                if !soloed.is_empty() {
                    soloed.iter().any(|s| *s == p.name)
                } else {
                    !muted.iter().any(|m| *m == p.name)
                }
            })
            .collect();
        self.tweak(|p| {
            for (i, on) in states.into_iter().enumerate() {
                p.parts[i].audible = on;
            }
        });
    }

    /// パートの音量。
    pub fn set_gain(&mut self, part: &str, gain: f32) {
        if let Some(i) = self.plan.part_of(part) {
            self.tweak(|p| p.parts[i].gain = gain.clamp(0.0, 8.0));
        }
    }

    /// パートの左右の広がり・残響の送り・ダッキング。
    pub fn set_mix(&mut self, part: &str, width: f32, reverb: f32, duck: f32) {
        if let Some(i) = self.plan.part_of(part) {
            self.tweak(|p| {
                p.parts[i].mix.width = width.clamp(0.0, 4.0);
                p.parts[i].mix.reverb = reverb.clamp(0.0, 2.0);
                p.parts[i].mix.duck = duck.clamp(0.0, 2.0);
            });
        }
    }

    /// 全体の音量。
    pub fn set_master_gain(&mut self, g: f32) {
        self.tweak(|p| p.master_gain = g.clamp(0.0, 4.0));
    }

    /// 書き出しと同じ音圧で聞くための倍率。裏で測り終えるまでは 1.0。
    pub fn makeup(&self) -> f32 {
        self.shared.makeup()
    }

    /// 今の設定の写し。画面に出すため。
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
