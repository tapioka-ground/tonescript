//! 音を出す側。**ここは締め切りのある場所。**
//!
//! 守っている決まり
//! ----------------
//! - 待たない（鍵を取らない）
//! - 確保しない（`Vec` を伸ばさない）
//! - 捨てない（鳴り終わった音は逆向きの輪へ返す）
//!
//! この3つを破ると、破った回数だけプツッと鳴る。作業用の置き場は最初に
//! 広く取っておき、足りなくなったときだけ伸ばす（曲を差し替えた直後の
//! 1ブロックだけ）。
//!
//! やること
//! --------
//! ```text
//!   鳴っている音 ─→ パートごとに足す ─→ ダッキング・音量の線
//!                                    ├→ 残響へ送る
//!                                    └→ 広がり・左右 ─→ 足し合わせ
//!                                                       ＋残響の戻り
//!                                                       ─→ 音圧・天井
//! ```
//!
//! 書き出しとの違い
//! ----------------
//! 書き出しにある「トラックの尖り止め（[`tonescript_render::mix::tame_crest`]）」
//! だけは掛けていない。あれは**曲まるごとの実効値**を見てから決めるもので、
//! 先頭から順に鳴らしていく側からは見えない。音圧合わせ（LUFS）のほうは、
//! 曲を読んだあとに裏で1回測って倍率として持つので揃う。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use tonescript_dsp::osc::SR;
use tonescript_dsp::reverb::Reverb;
use tonescript_render::mix::pan_gains;

use crate::plan::{Plan, StepCursor};
use crate::ring::{Rx, Tx};
use crate::voice::{Msg, Voice};

/// 画面側と音側で分け合う値。どちらからも読み書きする。
#[derive(Debug)]
pub struct Shared {
    /// 今どこを鳴らしているか（サンプル）
    pub pos: AtomicU64,
    pub playing: AtomicBool,
    /// 代。頭出しや曲の差し替えで1つ進む
    pub gen: AtomicU64,
    pub loop_from: AtomicU64,
    /// 0 なら繰り返さない
    pub loop_to: AtomicU64,
    /// 曲の終わりまで来たか（画面が見て止める）
    pub hit_end: AtomicBool,
    /// 書き出したときと同じ音圧で聞くための倍率（f32 のビット）。
    /// 曲を読んだあとに裏で1回測って入る。[`crate::sched`] を見よ
    pub makeup: AtomicU32,
    /// 止まらない時計。鍵を押した音はこちらへ乗せる
    pub clock: AtomicU64,
}

impl Shared {
    pub fn makeup(&self) -> f32 {
        f32::from_bits(self.makeup.load(Ordering::Relaxed))
    }

    pub fn set_makeup(&self, g: f32) {
        self.makeup.store(g.to_bits(), Ordering::Relaxed);
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            pos: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            gen: AtomicU64::new(1),
            loop_from: AtomicU64::new(0),
            loop_to: AtomicU64::new(0),
            hit_end: AtomicBool::new(false),
            makeup: AtomicU32::new(1.0f32.to_bits()),
            clock: AtomicU64::new(0),
        }
    }
}

/// キックのたびに凹ませる。
///
/// 書き出し側（[`tonescript_render::mix::sidechain_env`]）と同じ形を、
/// 先頭から順に作る。重なったところは深いほうを採る。
#[derive(Default)]
struct Duck {
    hits: VecDeque<u64>,
    depth: f32,
    a: u64,
    h: u64,
    r: u64,
}

impl Duck {
    fn set(&mut self, (depth, a, h, r): (f32, f32, f32, f32)) {
        self.depth = depth;
        self.a = ((a * SR) as u64).max(1);
        self.h = ((h * SR) as u64).max(1);
        self.r = ((r * SR) as u64).max(1);
    }

    fn len(&self) -> u64 {
        self.a + self.h + self.r
    }

    fn push(&mut self, at: u64) {
        if self.hits.len() < 256 {
            self.hits.push_back(at);
        }
    }

    fn clear(&mut self) {
        self.hits.clear();
    }

    /// もう過ぎたものを落とす。ブロックごとに1回。
    fn prune(&mut self, now: u64) {
        let span = self.len();
        while let Some(&k) = self.hits.front() {
            if k + span < now {
                self.hits.pop_front();
            } else {
                break;
            }
        }
    }

    /// そのサンプルでの倍率（1.0 で凹んでいない）。
    fn at(&self, now: u64) -> f32 {
        let mut g = 1.0f32;
        for &k in &self.hits {
            if now < k {
                continue;
            }
            let i = now - k;
            let v = if i < self.a {
                1.0 - self.depth * (i as f32 / self.a as f32)
            } else if i < self.a + self.h {
                1.0 - self.depth
            } else if i < self.a + self.h + self.r {
                let j = i - self.a - self.h;
                1.0 - self.depth + self.depth * (j as f32 / self.r as f32)
            } else {
                1.0
            };
            g = g.min(v);
        }
        g
    }
}

/// 外で録った音1本。歌や、その場で録ったもの。
///
/// 譜面ではなく波形をそのまま鳴らす。伴奏側のサイドチェインも尖り止めも
/// 掛けない（外で既に整っているものへ二重に掛けると割れる）。
#[derive(Clone, Debug)]
pub struct Track {
    pub label: String,
    pub l: std::sync::Arc<Vec<f32>>,
    pub r: std::sync::Arc<Vec<f32>>,
    pub gain: f32,
}

/// パート1つぶんの、そのブロックでの値。
#[derive(Clone, Copy, Default)]
struct Lanes {
    gain: f32,
    pan: f32,
    reverb: f32,
    duck: f32,
}

const MAX_BLOCK: usize = 8192;
/// パートの上限。針の数もこれに合わせる
pub const MAX_PARTS: usize = 64;
const MAX_VOICES: usize = 1024;

/// 音を出す側。cpal の中へ丸ごと渡す。
pub struct Mixer {
    shared: Arc<Shared>,
    rx: Rx<Msg>,
    gc: Tx<Voice>,
    plan: Arc<Plan>,
    voices: Vec<Voice>,
    /// パートごとの足し場
    acc: Vec<Vec<f32>>,
    /// 残響へ送るぶん
    send: Vec<f32>,
    reverb: Reverb,
    duck: Duck,
    cursor: StepCursor,
    /// 前のブロックの終わりの値と、今のブロックの終わりの値。
    /// 急に変えるとプツッと鳴るので、このあいだを繋ぐ
    lane_a: Vec<Lanes>,
    lane_b: Vec<Lanes>,
    have_prev: bool,
    /// 残響を回し続ける残りサンプル数。送りが途切れても尾を鳴らし切る
    rv_left: u64,
    /// 止まらない時計（サンプル）。鍵を押した音の置き場
    clock: u64,
    /// パートごとの尖り止め `(始まり, 天井)`。裏で測ってから届く。
    /// [`tonescript_render::mix::tame_crest`] と同じ形を、同じ閾値で掛ける
    trim: Vec<Option<(f32, f32)>>,
    /// 外で録った音
    audio: std::sync::Arc<Vec<Track>>,
    /// いま鳴っている音の数（画面に出す）
    live: Arc<AtomicU64>,
    /// 針
    meters: Arc<crate::meter::Meters>,
    /// 音圧計
    loudness: crate::meter::Loudness,
}

impl Mixer {
    pub(crate) fn new(
        shared: Arc<Shared>,
        rx: Rx<Msg>,
        gc: Tx<Voice>,
        live: Arc<AtomicU64>,
        meters: Arc<crate::meter::Meters>,
    ) -> Self {
        let plan = Arc::new(Plan::empty());
        let mut duck = Duck::default();
        duck.set(plan.sidechain);
        let (secs, spread) = plan.reverb;
        Self {
            shared,
            rx,
            gc,
            plan,
            voices: Vec::with_capacity(MAX_VOICES),
            acc: (0..MAX_PARTS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            send: vec![0.0; MAX_BLOCK],
            reverb: Reverb::new(secs, spread),
            duck,
            cursor: StepCursor::default(),
            lane_a: vec![Lanes::default(); MAX_PARTS],
            lane_b: vec![Lanes::default(); MAX_PARTS],
            have_prev: false,
            rv_left: 0,
            clock: 0,
            trim: Vec::new(),
            audio: std::sync::Arc::new(Vec::new()),
            live,
            meters,
            loudness: crate::meter::Loudness::new(),
        }
    }

    /// 届いたものを受け取る。**待たない。**
    fn take_messages(&mut self) {
        let gen = self.shared.gen.load(Ordering::Relaxed);
        // 1ブロックで捌く上限。いくら届いても締め切りは守る
        for _ in 0..2048 {
            let Some(m) = self.rx.pop() else { break };
            match m {
                Msg::Voice(v) => {
                    if v.gen != gen || self.voices.len() >= MAX_VOICES {
                        self.dispose(v);
                    } else {
                        self.voices.push(v);
                    }
                }
                Msg::Plan(p) => {
                    self.duck.set(p.sidechain);
                    let (secs, spread) = p.reverb;
                    let (osecs, ospread) = self.plan.reverb;
                    if (secs - osecs).abs() > 1e-6 || (spread - ospread).abs() > 1e-6 {
                        self.reverb.set(secs, spread);
                    }
                    if p.parts.len() != self.plan.parts.len() {
                        self.have_prev = false;
                        // パートの並びが変わった。測り直しが届くまで掛けない
                        self.trim.clear();
                    }
                    self.duck.set(p.sidechain);
                    self.plan = p;
                }
                Msg::Flush(g) => {
                    let mut keep = Vec::with_capacity(self.voices.len());
                    for v in self.voices.drain(..) {
                        // 鍵を押している音は曲の位置に乗っていないので、
                        // 頭出ししても止めない（押しているのに消えたら驚く）
                        if v.gen >= g || v.live {
                            keep.push(v);
                        } else {
                            // 音側では捨てない。輪へ返して向こうで捨てる
                            let _ = self.gc.push(v);
                        }
                    }
                    self.voices = keep;
                    self.duck.clear();
                    self.reverb.clear();
                    self.cursor.reset();
                    self.have_prev = false;
                }
                Msg::Kick(at) => self.duck.push(at),
                Msg::Trim(t) => self.trim = t,
                Msg::Audio(a) => self.audio = a,
                Msg::Cut { part, pitch, at } => {
                    for v in self.voices.iter_mut() {
                        // 新しく足したぶんより前のものだけを終わらせる
                        if v.live && v.part == part && v.pitch == pitch && v.start < at {
                            v.cut_at(at);
                        }
                    }
                }
                Msg::Off { part, pitch } => {
                    for v in self.voices.iter_mut() {
                        if v.live && v.part == part && v.pitch == pitch {
                            v.release();
                        }
                    }
                }
            }
        }
    }

    fn dispose(&mut self, v: Voice) {
        // 輪がいっぱいなら仕方なくここで捨てる（滅多に起きない）
        let _ = self.gc.push(v);
    }

    /// そのブロックでのパートの値を引く。**置き場は使い回す。**
    fn fill_lanes(plan: &Plan, step: f32, dst: &mut [Lanes]) {
        for (i, p) in plan.parts.iter().enumerate() {
            dst[i] = Lanes {
                gain: p.gain_curve.as_ref().and_then(|c| c.at(step)).unwrap_or(1.0),
                pan: p.pan_curve.as_ref().and_then(|c| c.at(step)).unwrap_or(0.0),
                reverb: p.reverb_curve.as_ref().and_then(|c| c.at(step)).unwrap_or(p.mix.reverb),
                duck: p.duck_curve.as_ref().and_then(|c| c.at(step)).unwrap_or(p.mix.duck),
            };
        }
    }

    /// `n` サンプルぶん、`pos` から作る。
    fn block(&mut self, pos: u64, n: usize, play: bool, out_l: &mut [f32], out_r: &mut [f32]) {
        let plan = self.plan.clone();
        let np = plan.parts.len();
        if self.acc.len() < np {
            self.acc.resize_with(np, || vec![0.0; MAX_BLOCK]);
            self.lane_a.resize(np, Lanes::default());
            self.lane_b.resize(np, Lanes::default());
        }
        for a in self.acc.iter_mut().take(np) {
            if a.len() < n {
                a.resize(n, 0.0);
            }
            a[..n].fill(0.0);
        }
        if self.send.len() < n {
            self.send.resize(n, 0.0);
        }
        self.send[..n].fill(0.0);
        out_l[..n].fill(0.0);
        out_r[..n].fill(0.0);

        // 鳴っている音をパートごとに足す。
        // 譜面の音は曲の位置、鍵を押した音は止まらない時計で見る
        let clock = self.clock;
        for v in self.voices.iter_mut() {
            let base = if v.live { clock } else { pos };
            let end = base + n as u64;
            if v.part >= np || (!play && !v.live) || v.start >= end || v.end() <= base {
                continue;
            }
            let from = v.start.max(base);
            let skip = (from - v.start) as usize;
            let at = (from - base) as usize;
            let take = ((v.end().min(end) - from) as usize)
                .min(v.buf.len().saturating_sub(skip))
                .min(n - at);
            let dst = &mut self.acc[v.part][at..at + take];
            let src = &v.buf[skip..skip + take];
            // 立ち上がり・継ぎ目・離した後。何も無ければ素通し
            let shaped = v.fade_in > 0 || v.cut.is_some() || v.off.is_some();
            if !shaped {
                for (d, s) in dst.iter_mut().zip(src) {
                    *d += *s;
                }
                v.done = (skip + take).max(v.done);
            } else {
                let mut off = v.off;
                for (k, (d, s)) in dst.iter_mut().zip(src).enumerate() {
                    let at = from + k as u64;
                    let mut g = v.gain_at(at);
                    if let Some((span, left)) = &mut off {
                        if *left == 0 {
                            g = 0.0;
                        } else {
                            g *= *left as f32 / (*span).max(1) as f32;
                            *left -= 1;
                        }
                    }
                    *d += *s * g;
                }
                v.off = off;
                let gone = v.off.is_some_and(|(_, left)| left == 0)
                    || v.cut.is_some_and(|(at, span)| from + take as u64 >= at + span as u64);
                v.done = if gone { v.buf.len() } else { (skip + take).max(v.done) };
            }
        }

        // パートごとに整えて左右へ
        self.duck.prune(pos);
        let step_a = self.cursor.step_at(pos, &plan.step_times);
        let step_b = self.cursor.step_at(pos + n as u64, &plan.step_times);
        let mut from = std::mem::take(&mut self.lane_a);
        let mut now = std::mem::take(&mut self.lane_b);
        if !self.have_prev {
            Self::fill_lanes(&plan, step_a, &mut from);
        }
        Self::fill_lanes(&plan, step_b, &mut now);

        for (pi, part) in plan.parts.iter().enumerate() {
            if !part.audible || part.gain == 0.0 {
                continue;
            }
            let (a, b) = (from[pi], now[pi]);
            let width = part.mix.width;
            let s = (width * 0.5).max(0.0);
            let wide = 1.0 / (1.0 + s * s).sqrt();
            let gain = part.gain;
            let mut loudest = 0.0f32;
            for i in 0..n {
                let t = i as f32 / n as f32;
                let g = a.gain + (b.gain - a.gain) * t;
                let pan = a.pan + (b.pan - a.pan) * t;
                let rv = a.reverb + (b.reverb - a.reverb) * t;
                let dk = a.duck + (b.duck - a.duck) * t;

                let mut v = self.acc[pi][i];
                // トラックの尖り止め。書き出しと同じ膝で寄せる
                if let Some(Some((start, ceil))) = self.trim.get(pi) {
                    let a = v.abs();
                    if a > *start {
                        let over = (a - start) / (ceil - start).max(1e-9);
                        v = v.signum() * (start + (ceil - start) * over.tanh());
                    }
                }
                if dk > 0.0 {
                    v *= 1.0 - (1.0 - self.duck.at(pos + i as u64)) * dk;
                }
                v *= g;
                if rv > 0.0 {
                    self.send[i] += v * rv;
                }
                // 広がり（逆相ぶんを混ぜる）
                let (mut l, mut r) =
                    if s > 0.0 { (v * (1.0 + s) * wide, v * (1.0 - s) * wide) } else { (v, v) };
                if pan != 0.0 {
                    let (gl, gr) = pan_gains(pan);
                    l *= gl * std::f32::consts::SQRT_2;
                    r *= gr * std::f32::consts::SQRT_2;
                }
                out_l[i] += l * gain;
                out_r[i] += r * gain;
                let seen = (l * gain).abs().max((r * gain).abs());
                if seen > loudest {
                    loudest = seen;
                }
            }
            self.meters.hit_part(pi, loudest);
        }
        // 次のブロックの「前の値」は、今のブロックの終わりの値
        from[..np].copy_from_slice(&now[..np]);
        self.lane_a = from;
        self.lane_b = now;
        self.have_prev = true;

        // 残響。集めた送りを1つの部屋へ通して戻す。
        // 送りが途切れても、尾が消えるまでは回し続ける
        if self.send[..n].iter().any(|v| *v != 0.0) {
            self.rv_left = ((plan.reverb.0 * SR) as u64).max(1) * 2;
        }
        if self.rv_left > 0 {
            for i in 0..n {
                let (rl, rr) = self.reverb.run(self.send[i]);
                out_l[i] += rl;
                out_r[i] += rr;
            }
            self.rv_left = self.rv_left.saturating_sub(n as u64);
        }

        // 外で録った音。譜面のパートより後に足す。
        // 伴奏側のサイドチェインや尖り止めは掛けない
        if play && !self.audio.is_empty() {
            let audio = self.audio.clone();
            for t in audio.iter() {
                if t.gain <= 0.0 {
                    continue;
                }
                let from = pos as usize;
                let take = t.l.len().min(t.r.len()).saturating_sub(from).min(n);
                for i in 0..take {
                    out_l[i] += t.l[from + i] * t.gain;
                    out_r[i] += t.r[from + i] * t.gain;
                }
            }
        }

        // 音圧と天井
        let g = plan.master_gain * self.shared.makeup();
        let (mut pl, mut pr) = (0.0f32, 0.0f32);
        for i in 0..n {
            out_l[i] = ceiling(out_l[i] * g);
            out_r[i] = ceiling(out_r[i] * g);
            pl = pl.max(out_l[i].abs());
            pr = pr.max(out_r[i].abs());
        }
        self.meters.hit_master(pl, pr);
    }

    /// 音の出口から呼ばれる。左右そろえて `n` サンプル埋める。
    pub fn fill(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        self.take_messages();
        let n = out_l.len().min(out_r.len());
        // 止まっていても、鍵を押した音だけは鳴らす。
        // **音符を触った音が返らない**のが、直したかったことの1つ
        if !self.shared.playing.load(Ordering::Relaxed) || self.plan.total == 0 {
            let pos = self.shared.pos.load(Ordering::Relaxed);
            self.block(pos, n, false, &mut out_l[..n], &mut out_r[..n]);
            self.tick(n);
            self.measure_loudness(&out_l[..n], &out_r[..n]);
            self.retire();
            return;
        }
        let mut pos = self.shared.pos.load(Ordering::Relaxed);
        let mut at = 0usize;
        // 繰り返す範囲と曲の終わりで切りながら埋める
        while at < n {
            let lo = self.shared.loop_from.load(Ordering::Relaxed);
            let hi = self.shared.loop_to.load(Ordering::Relaxed);
            if hi > lo && pos >= hi {
                pos = lo;
                self.jump(pos);
            }
            if pos >= self.plan.total {
                out_l[at..n].fill(0.0);
                out_r[at..n].fill(0.0);
                self.shared.playing.store(false, Ordering::Relaxed);
                self.shared.hit_end.store(true, Ordering::Relaxed);
                break;
            }
            let mut take = n - at;
            if hi > lo {
                take = take.min((hi - pos) as usize);
            }
            take = take.min((self.plan.total - pos) as usize).max(1);
            let (l, r) = (&mut out_l[at..at + take], &mut out_r[at..at + take]);
            self.block(pos, take, true, l, r);
            pos += take as u64;
            at += take;
        }
        self.shared.pos.store(pos, Ordering::Relaxed);
        self.tick(n);
        self.measure_loudness(&out_l[..n], &out_r[..n]);
        self.retire();
    }

    /// 今の音圧を測る。400ミリ秒の窓で、書き出しと同じ物差し。
    fn measure_loudness(&mut self, l: &[f32], r: &[f32]) {
        let v = self.loudness.push(l, r);
        self.meters.set_lufs(v);
    }

    /// 止まらない時計を進める。
    fn tick(&mut self, n: usize) {
        self.clock += n as u64;
        self.shared.clock.store(self.clock, Ordering::Relaxed);
    }

    /// 繰り返しの頭へ飛ぶ。鳴りかけを片付けて、残響も切る。
    fn jump(&mut self, _to: u64) {
        for v in self.voices.drain(..) {
            let _ = self.gc.push(v);
        }
        self.duck.clear();
        self.reverb.clear();
        self.rv_left = 0;
        self.loudness.clear();
        self.cursor.reset();
        self.have_prev = false;
    }

    /// 鳴り終わったものを返す。
    fn retire(&mut self) {
        let pos = self.shared.pos.load(Ordering::Relaxed);
        let clock = self.clock;
        let mut i = 0;
        while i < self.voices.len() {
            let gone = self.voices[i].end() <= if self.voices[i].live { clock } else { pos };
            if gone || self.voices[i].finished() {
                let v = self.voices.swap_remove(i);
                let _ = self.gc.push(v);
            } else {
                i += 1;
            }
        }
        self.live.store(self.voices.len() as u64, Ordering::Relaxed);
    }
}

/// 天井を丸める。書き出し側の [`tonescript_render::mix::limit`] と同じ膝。
#[inline]
fn ceiling(v: f32) -> f32 {
    const CEIL: f32 = 0.99;
    const KNEE: f32 = CEIL * 0.7;
    const SPAN: f32 = CEIL * 0.3;
    let a = v.abs();
    if a <= KNEE {
        return v;
    }
    v.signum() * (KNEE + SPAN * ((a - KNEE) / SPAN).tanh())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::ring;

    fn rig() -> (Mixer, Tx<Msg>, Rx<Voice>, Arc<Shared>) {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = ring::<Msg>(256);
        let (gtx, grx) = ring::<Voice>(256);
        let live = Arc::new(AtomicU64::new(0));
        let meters = Arc::new(crate::meter::Meters::new(MAX_PARTS));
        (Mixer::new(shared.clone(), rx, gtx, live, meters), tx, grx, shared)
    }

    fn plan_with(total: u64, parts: usize) -> Arc<Plan> {
        let mut p = Plan::empty();
        p.total = total;
        for i in 0..parts {
            p.parts.push(crate::plan::PartPlan {
                name: format!("p{i}"),
                gain: 1.0,
                mix: Default::default(),
                audible: true,
                gain_curve: None,
                pan_curve: None,
                reverb_curve: None,
                duck_curve: None,
            });
            p.index.insert(format!("p{i}"), i);
        }
        Arc::new(p)
    }

    fn voice(part: usize, start: u64, v: f32, n: usize) -> Voice {
        Voice {
            part,
            start,
            buf: vec![v; n],
            done: 0,
            gen: 1,
            live: false,
            pitch: 60,
            off: None,
            fade_in: 0,
            cut: None,
        }
    }

    #[test]
    fn silence_until_told_to_play() {
        let (mut m, tx, _gc, _sh) = rig();
        tx.push(Msg::Plan(plan_with(48_000, 1))).unwrap();
        tx.push(Msg::Voice(voice(0, 0, 0.5, 100))).unwrap();
        let (mut l, mut r) = (vec![9.0; 64], vec![9.0; 64]);
        m.fill(&mut l, &mut r);
        assert!(l.iter().all(|v| *v == 0.0), "止まっているのに鳴った");
    }

    #[test]
    fn a_voice_lands_where_it_was_put() {
        let (mut m, tx, _gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(48_000, 1))).unwrap();
        // 32サンプル目から 16サンプルぶん
        tx.push(Msg::Voice(voice(0, 32, 0.5, 16))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        m.fill(&mut l, &mut r);
        assert!(l[..32].iter().all(|v| *v == 0.0), "早く鳴り出した");
        assert!(l[32..48].iter().all(|v| *v > 0.4), "鳴っていない");
        assert!(l[48..].iter().all(|v| *v == 0.0), "長く鳴りすぎ");
        assert_eq!(sh.pos.load(Ordering::Relaxed), 64, "位置が進んでいない");
    }

    #[test]
    fn a_voice_split_across_blocks_stays_whole() {
        let (mut m, tx, _gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(48_000, 1))).unwrap();
        tx.push(Msg::Voice(voice(0, 48, 0.5, 32))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        m.fill(&mut l, &mut r);
        assert!(l[48..].iter().all(|v| *v > 0.4), "1回目の後半が鳴っていない");
        let (mut l2, mut r2) = (vec![0.0; 64], vec![0.0; 64]);
        m.fill(&mut l2, &mut r2);
        // 残り16サンプルが2回目の頭に続く
        assert!(l2[..16].iter().all(|v| *v > 0.4), "続きが切れた");
        assert!(l2[16..].iter().all(|v| *v == 0.0), "長く残った");
    }

    #[test]
    fn parts_add_up() {
        let (mut m, tx, _gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(48_000, 2))).unwrap();
        tx.push(Msg::Voice(voice(0, 0, 0.2, 32))).unwrap();
        tx.push(Msg::Voice(voice(1, 0, 0.3, 32))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 32], vec![0.0; 32]);
        m.fill(&mut l, &mut r);
        assert!((l[0] - 0.5).abs() < 1e-5, "足し合わせが {}", l[0]);
    }

    #[test]
    fn a_muted_part_is_silent() {
        let (mut m, tx, _gc, sh) = rig();
        let mut p = (*plan_with(48_000, 1)).clone();
        p.parts[0].audible = false;
        tx.push(Msg::Plan(Arc::new(p))).unwrap();
        tx.push(Msg::Voice(voice(0, 0, 0.5, 32))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 32], vec![0.0; 32]);
        m.fill(&mut l, &mut r);
        assert!(l.iter().all(|v| *v == 0.0), "ミュートしたのに鳴った");
    }

    #[test]
    fn panning_moves_the_sound() {
        let (mut m, tx, _gc, sh) = rig();
        let mut p = (*plan_with(48_000, 1)).clone();
        p.parts[0].pan_curve =
            Some(tonescript_song::model::Curve::new(vec![(0, -1.0), (0, -1.0)]));
        tx.push(Msg::Plan(Arc::new(p))).unwrap();
        tx.push(Msg::Voice(voice(0, 0, 0.5, 64))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        m.fill(&mut l, &mut r);
        assert!(l[32] > 0.6, "左が鳴っていない: {}", l[32]);
        assert!(r[32].abs() < 1e-3, "左いっぱいなのに右が鳴っている: {}", r[32]);
    }

    #[test]
    fn a_flush_drops_the_old_generation() {
        let (mut m, tx, gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(48_000, 1))).unwrap();
        tx.push(Msg::Voice(voice(0, 0, 0.5, 64))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        sh.gen.store(2, Ordering::Relaxed);
        tx.push(Msg::Flush(2)).unwrap();
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        m.fill(&mut l, &mut r);
        assert!(l.iter().all(|v| *v == 0.0), "頭出ししたのに古い音が残った");
        assert!(gc.pop().is_some(), "捨てる側へ返っていない");
    }

    #[test]
    fn finished_voices_are_handed_back_not_dropped() {
        let (mut m, tx, gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(48_000, 1))).unwrap();
        tx.push(Msg::Voice(voice(0, 0, 0.5, 16))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        m.fill(&mut l, &mut r);
        let back = gc.pop().expect("鳴り終わった音が返ってきていない");
        assert_eq!(back.buf.len(), 16);
    }

    #[test]
    fn it_stops_at_the_end_of_the_song() {
        let (mut m, tx, _gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(100, 1))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        m.fill(&mut l, &mut r);
        assert!(!sh.playing.load(Ordering::Relaxed), "終わっても止まらない");
        assert!(sh.hit_end.load(Ordering::Relaxed));
    }

    #[test]
    fn a_loop_comes_back_to_the_top() {
        let (mut m, tx, _gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(48_000, 1))).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        sh.loop_from.store(0, Ordering::Relaxed);
        sh.loop_to.store(32, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
        m.fill(&mut l, &mut r);
        let pos = sh.pos.load(Ordering::Relaxed);
        // 128サンプル頼んで、32サンプルの輪を4周した。最後は輪の中に居る
        // （折り返しは次に呼ばれたときの頭で起きる）
        assert!(pos <= 32, "繰り返しの外へ出た: {pos}");
        assert!(sh.playing.load(Ordering::Relaxed), "繰り返しなのに止まった");
    }

    #[test]
    fn a_released_key_fades_instead_of_cutting() {
        let (mut m, tx, _gc, sh) = rig();
        tx.push(Msg::Plan(plan_with(480_000, 1))).unwrap();
        let mut v = voice(0, 0, 0.5, 48_000);
        v.live = true;
        v.pitch = 64;
        tx.push(Msg::Voice(v)).unwrap();
        sh.playing.store(true, Ordering::Relaxed);
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        m.fill(&mut l, &mut r);
        assert!(l[100] > 0.4, "鳴っていない");

        // 別の音程を離しても消えない
        tx.push(Msg::Off { part: 0, pitch: 60 }).unwrap();
        m.fill(&mut l, &mut r);
        assert!(l[100] > 0.4, "違う音程で消えた");

        // 離す。ぶつ切りではなく下がっていくこと
        tx.push(Msg::Off { part: 0, pitch: 64 }).unwrap();
        let n = (tonescript_engine_release() * SR) as usize;
        let (mut fl, mut fr) = (vec![0.0; n + 256], vec![0.0; n + 256]);
        m.fill(&mut fl, &mut fr);
        assert!(fl[0] > 0.4, "離した瞬間に切れた");
        assert!(fl[n / 2] < fl[0], "下がっていない");
        assert!(fl[n / 2] > 0.0, "途中で切れた");
        assert!(fl[n + 100].abs() < 1e-6, "消えていない: {}", fl[n + 100]);
    }

    fn tonescript_engine_release() -> f32 {
        crate::voice::RELEASE
    }

    #[test]
    fn the_ducker_dips_and_recovers() {
        let mut d = Duck::default();
        d.set((0.7, 0.003, 0.02, 0.2));
        d.push(1000);
        assert_eq!(d.at(999), 1.0, "鳴る前から凹んでいる");
        let bottom = d.at(1000 + (0.01 * SR) as u64);
        assert!((bottom - 0.3).abs() < 0.02, "凹みが {bottom}");
        let after = d.at(1000 + (0.5 * SR) as u64);
        assert_eq!(after, 1.0, "戻りきっていない: {after}");
    }

    #[test]
    fn overlapping_kicks_take_the_deeper_dip() {
        let mut d = Duck::default();
        d.set((0.7, 0.003, 0.02, 0.2));
        d.push(0);
        d.push(100);
        // 足し算で消えず、深いほうが残ること
        let g = d.at(100 + (0.01 * SR) as u64);
        assert!(g >= 0.29 && g <= 0.32, "{g}");
    }

    #[test]
    fn the_ceiling_is_soft_and_never_passes_one() {
        assert_eq!(ceiling(0.5), 0.5, "低い所は触らない");
        assert!(ceiling(2.0) < 1.0, "天井を超えた");
        assert!(ceiling(-2.0) > -1.0);
        assert!(ceiling(10.0) < 1.0, "どれだけ入れても超えない");
        // 折り返さず、単調であること
        assert!(ceiling(3.0) > ceiling(2.0));
    }
}
