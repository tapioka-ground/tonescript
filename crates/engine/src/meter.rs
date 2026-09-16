//! 針。今どれくらい鳴っているか。
//!
//! 音側が書き、画面が読む。**画面は 60 分の1秒に1回しか見に来ないので、
//! そのあいだの一番大きかったところを覚えておく。** 覚えずにその瞬間の値を
//! 渡すと、一番大きい所をほとんど取りこぼして、針が実際より低く出る。
//!
//! 読むと 0 に戻す。次に読むまでのあいだの最大がまた溜まる。

use std::sync::atomic::{AtomicU32, Ordering};

use tonescript_dsp::osc::SR;
use tonescript_render::mix::{lufs_of_mean, KFilter};

/// 音圧を測る窓の長さ。EBU R128 の「瞬時」と同じ 400ミリ秒。
pub const WINDOW: f32 = 0.400;

/// f32 を原子的に置く。`Ordering` は緩くていい（1サンプル遅れても針）。
#[derive(Debug, Default)]
struct F32Cell(AtomicU32);

impl F32Cell {
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }

    /// 大きいほうを残す。書くのは音側だけなので、読んで比べて書くだけでいい。
    fn keep_larger(&self, v: f32) {
        if v > self.get() {
            self.set(v);
        }
    }

    /// 読んで 0 に戻す。
    fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

/// 針の束。
#[derive(Debug)]
pub struct Meters {
    parts: Vec<F32Cell>,
    master_l: F32Cell,
    master_r: F32Cell,
    lufs: F32Cell,
}

impl Meters {
    pub fn new(parts: usize) -> Meters {
        Meters {
            parts: (0..parts).map(|_| F32Cell::default()).collect(),
            master_l: F32Cell::default(),
            master_r: F32Cell::default(),
            lufs: F32Cell::new_at(-70.0),
        }
    }

    pub(crate) fn hit_part(&self, i: usize, v: f32) {
        if let Some(c) = self.parts.get(i) {
            c.keep_larger(v);
        }
    }

    pub(crate) fn hit_master(&self, l: f32, r: f32) {
        self.master_l.keep_larger(l);
        self.master_r.keep_larger(r);
    }

    pub(crate) fn set_lufs(&self, v: f32) {
        self.lufs.set(v);
    }

    /// そのパートの、前に読んでからの一番大きかったところ。読むと 0 に戻る。
    pub fn take_part(&self, i: usize) -> f32 {
        self.parts.get(i).map(|c| c.take()).unwrap_or(0.0)
    }

    /// 全体の左右。読むと 0 に戻る。
    pub fn take_master(&self) -> (f32, f32) {
        (self.master_l.take(), self.master_r.take())
    }

    /// 今の音圧（LUFS、400ミリ秒の窓）。読んでも消えない。
    pub fn lufs(&self) -> f32 {
        self.lufs.get()
    }
}

impl F32Cell {
    fn new_at(v: f32) -> F32Cell {
        F32Cell(AtomicU32::new(v.to_bits()))
    }
}

/// 音圧を測る。K 特性を通した二乗和を、窓のぶんだけ持つ。
///
/// 窓ぶんの生の音を持つのではなく、**ブロックごとの合計だけ**を持つ。
/// 400ミリ秒ぶんで 19,200 個の数ではなく、数十個の合計で足りる。
pub struct Loudness {
    kl: KFilter,
    kr: KFilter,
    /// `(左の二乗和, 右の二乗和, 何サンプルぶん)`
    blocks: std::collections::VecDeque<(f64, f64, usize)>,
    sum_l: f64,
    sum_r: f64,
    total: usize,
}

impl Default for Loudness {
    fn default() -> Self {
        Self::new()
    }
}

impl Loudness {
    pub fn new() -> Loudness {
        Loudness {
            kl: KFilter::new(),
            kr: KFilter::new(),
            blocks: std::collections::VecDeque::with_capacity(512),
            sum_l: 0.0,
            sum_r: 0.0,
            total: 0,
        }
    }

    /// 1ブロック入れて、今の音圧を返す。
    pub fn push(&mut self, l: &[f32], r: &[f32]) -> f32 {
        let n = l.len().min(r.len());
        let mut sl = 0.0f64;
        let mut sr = 0.0f64;
        for i in 0..n {
            let a = self.kl.run(l[i]) as f64;
            let b = self.kr.run(r[i]) as f64;
            sl += a * a;
            sr += b * b;
        }
        self.blocks.push_back((sl, sr, n));
        self.sum_l += sl;
        self.sum_r += sr;
        self.total += n;

        let window = (WINDOW * SR) as usize;
        while self.total > window || self.blocks.len() > 500 {
            let Some((a, b, k)) = self.blocks.pop_front() else { break };
            self.sum_l -= a;
            self.sum_r -= b;
            self.total -= k;
            if self.total == 0 {
                break;
            }
        }
        if self.total == 0 {
            return -70.0;
        }
        let d = self.total as f64;
        lufs_of_mean(self.sum_l / d, self.sum_r / d).max(-70.0)
    }

    /// 忘れる。曲を変えたときや頭出しのとき。
    pub fn clear(&mut self) {
        *self = Loudness::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meter_keeps_the_loudest_until_it_is_read() {
        let m = Meters::new(2);
        m.hit_part(0, 0.3);
        m.hit_part(0, 0.9);
        m.hit_part(0, 0.5);
        assert_eq!(m.take_part(0), 0.9, "一番大きい所を取りこぼした");
        assert_eq!(m.take_part(0), 0.0, "読んだのに残っている");
        // 無いパートを読んでも落ちない
        assert_eq!(m.take_part(99), 0.0);
    }

    #[test]
    fn left_and_right_are_kept_apart() {
        let m = Meters::new(1);
        m.hit_master(0.8, 0.2);
        assert_eq!(m.take_master(), (0.8, 0.2));
        assert_eq!(m.take_master(), (0.0, 0.0));
    }

    #[test]
    fn silence_reads_as_very_quiet() {
        let mut ld = Loudness::new();
        let z = vec![0.0f32; 4800];
        let v = ld.push(&z, &z);
        assert!(v <= -70.0, "無音が {v} LUFS になった");
    }

    #[test]
    fn a_full_scale_sine_lands_near_minus_three() {
        // 1kHz の正弦を振り切るまで入れると、K 特性を通しておよそ -3 LUFS。
        // 書き出し側の lufs() と同じ数字になること
        let n = (WINDOW * SR) as usize;
        let mut sig = Vec::with_capacity(n);
        for i in 0..n {
            sig.push((i as f32 * std::f32::consts::TAU * 1000.0 / SR).sin());
        }
        let mut ld = Loudness::new();
        let mut got = -70.0;
        // 窓が埋まるまで入れる
        for chunk in sig.chunks(512) {
            got = ld.push(chunk, chunk);
        }
        let want = tonescript_render::mix::lufs(&sig, &sig);
        assert!((got - want).abs() < 0.5, "鳴らしながら {got:.2} / 書き出し {want:.2}");
    }

    #[test]
    fn louder_reads_louder() {
        let n = (WINDOW * SR) as usize;
        let mk = |amp: f32| -> Vec<f32> {
            (0..n).map(|i| amp * (i as f32 * std::f32::consts::TAU * 1000.0 / SR).sin()).collect()
        };
        let mut a = Loudness::new();
        let mut b = Loudness::new();
        let (x, y) = (mk(1.0), mk(0.5));
        let mut va = -70.0;
        let mut vb = -70.0;
        for (p, q) in x.chunks(512).zip(y.chunks(512)) {
            va = a.push(p, p);
            vb = b.push(q, q);
        }
        // 半分の大きさは 6dB 下
        assert!((va - vb - 6.02).abs() < 0.2, "{va:.2} と {vb:.2} の差が合わない");
    }

    #[test]
    fn the_window_forgets_what_fell_out_of_it() {
        // 大きい音のあと黙ったら、窓のぶんだけ経って下がること
        let n = (WINDOW * SR) as usize;
        let loud: Vec<f32> =
            (0..n).map(|i| (i as f32 * std::f32::consts::TAU * 1000.0 / SR).sin()).collect();
        let mut ld = Loudness::new();
        let mut hot = -70.0;
        for c in loud.chunks(512) {
            hot = ld.push(c, c);
        }
        assert!(hot > -10.0, "大きい音が {hot}");
        let quiet = vec![0.0f32; 512];
        let mut now = hot;
        for _ in 0..(n / 512 + 2) {
            now = ld.push(&quiet, &quiet);
        }
        assert!(now < hot - 20.0, "黙ったのに {now}（さっきは {hot}）");
    }
}
