//! 残響。
//!
//! なぜ畳み込みではないのか
//! ------------------------
//! Python 版は「部屋の鳴りを録ったもの（インパルス応答）を FFT で畳み込む」
//! 作りだった。あれは numpy が FFT を持っていたから成り立っていた方法で、
//! こちらで同じことをすると FFT のライブラリを1つ抱えることになる。
//!
//! ここでは櫛型フィルタとオールパスを並べて作る。1サンプルずつ回すだけで、
//! 外の道具が要らない。ブロック単位の遅れも出ないので、鳴らしながら
//! 掛けることもできる（今は使っていないが、あとで効く）。
//!
//! 仕組み
//! ------
//! ```text
//!   入力 ─┬→ 櫛型 x8（並列）─→ 足す ─→ オールパス x4（直列）─→ 出力
//!         └ 左右で長さを少しずらして、広がりを作る
//! ```
//!
//! **櫛型**は「少し遅らせて戻す」を繰り返して、反射の連なりを作る。
//! 1本だと「ピーン」と音程が付くので、長さの違うものを8本重ねて散らす。
//! 長さは互いに割り切れない数にする。割り切れると反射が重なって、
//! 特定の音程だけが強く響く。
//!
//! **オールパス**は音の大きさを変えずに位相だけずらす。櫛型だけだと
//! 反射の粒が粗くて「バラバラ」と聞こえるので、これで密度を上げる。

use crate::osc::SR;

/// 櫛型の遅れ（サンプル）。44.1kHz を前提にした古典的な値を、
/// 48kHz へ直したもの。互いに割り切れない数にしてある。
const COMB: [usize; 8] = [1323, 1383, 1461, 1523, 1601, 1663, 1741, 1801];

/// オールパスの遅れ。こちらも互いに割り切れない。
const ALLPASS: [usize; 4] = [612, 478, 371, 243];

/// 左右でずらす量。これが無いと左右が同じ音になって広がらない。
const SPREAD: usize = 23;

/// 遅らせて戻す線。反射の連なりを作る。
struct Comb {
    buf: Vec<f32>,
    i: usize,
    /// どれだけ戻すか。大きいほど長く残る
    feedback: f32,
    /// 戻す前に高い音を落とす量。実際の部屋は高い音から先に消える
    damp: f32,
    /// 落とした結果を覚えておく
    store: f32,
}

impl Comb {
    fn new(n: usize) -> Self {
        Self { buf: vec![0.0; n.max(1)], i: 0, feedback: 0.5, damp: 0.2, store: 0.0 }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.buf[self.i];
        // 高い音を落としてから戻す。1極のローパス
        self.store = y * (1.0 - self.damp) + self.store * self.damp;
        self.buf[self.i] = x + self.store * self.feedback;
        self.i += 1;
        if self.i >= self.buf.len() {
            self.i = 0;
        }
        y
    }
    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.store = 0.0;
    }
}

/// 音の大きさを変えずに位相だけずらす。反射の密度を上げる。
struct AllPass {
    buf: Vec<f32>,
    i: usize,
}

impl AllPass {
    fn new(n: usize) -> Self {
        Self { buf: vec![0.0; n.max(1)], i: 0 }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.buf[self.i];
        // 係数 0.5 は古典的な値。ここを動かすと「金属的」になる
        self.buf[self.i] = x + y * 0.5;
        self.i += 1;
        if self.i >= self.buf.len() {
            self.i = 0;
        }
        y - x
    }
    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// 片側ぶん。
struct Side {
    combs: Vec<Comb>,
    allpass: Vec<AllPass>,
}

impl Side {
    fn new(offset: usize) -> Self {
        Self {
            combs: COMB.iter().map(|n| Comb::new(n + offset)).collect(),
            allpass: ALLPASS.iter().map(|n| AllPass::new(n + offset)).collect(),
        }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        // 櫛型は並列。足してから
        let mut y = 0.0;
        for c in &mut self.combs {
            y += c.run(x);
        }
        y /= self.combs.len() as f32;
        // オールパスは直列
        for a in &mut self.allpass {
            y = a.run(y);
        }
        y
    }
    fn clear(&mut self) {
        for c in &mut self.combs {
            c.clear();
        }
        for a in &mut self.allpass {
            a.clear();
        }
    }
}

/// 残響。
pub struct Reverb {
    l: Side,
    r: Side,
    /// 左右をどれだけ混ぜるか。0 で完全に分かれる
    cross: f32,
}

impl Reverb {
    /// 作る。
    ///
    /// - `seconds` … 残響が消えるまでのおよその秒数。0.2〜10 くらい
    /// - `spread` … 広がり。0 で中央、大きいほど左右へ開く。だいたい 0〜8
    pub fn new(seconds: f32, spread: f32) -> Self {
        let mut me = Self { l: Side::new(0), r: Side::new(SPREAD), cross: 0.0 };
        me.set(seconds, spread);
        me
    }

    /// 長さと広がりを決め直す。
    pub fn set(&mut self, seconds: f32, spread: f32) {
        let secs = seconds.clamp(0.05, 20.0);
        // 60dB 落ちるまでの時間から、戻す量を逆算する。
        //   feedback^(遅れの回数) = 1/1000
        // 遅れの長さは櫛型ごとに違うので、真ん中あたりを使う
        let avg = COMB.iter().sum::<usize>() as f32 / COMB.len() as f32;
        let laps = (secs * SR / avg).max(1.0);
        let fb = 10f32.powf(-3.0 / laps).clamp(0.0, 0.98);
        // 長い残響ほど高い音を強く落とす。実際の部屋もそうなっている
        let damp = (0.15 + secs * 0.05).clamp(0.1, 0.6);
        for s in [&mut self.l, &mut self.r] {
            for c in &mut s.combs {
                c.feedback = fb;
                c.damp = damp;
            }
        }
        // 広がりが小さいほど左右を混ぜる（＝中央に寄る）
        self.cross = (1.0 - (spread / 8.0).clamp(0.0, 1.0)) * 0.5;
    }

    /// 溜まっているものを捨てる。曲を作り直すときに呼ぶ。
    pub fn clear(&mut self) {
        self.l.clear();
        self.r.clear();
    }

    /// 1サンプル。入力はモノラル、出力は左右。
    #[inline]
    pub fn run(&mut self, x: f32) -> (f32, f32) {
        let a = self.l.run(x);
        let b = self.r.run(x);
        // 左右を少し混ぜる。0 なら完全に分かれて「ヘッドホンで不自然」になる
        let c = self.cross;
        (a * (1.0 - c) + b * c, b * (1.0 - c) + a * c)
    }

    /// まとめて掛ける。返すのは残響だけ（元の音は混ざっていない）。
    ///
    /// 混ぜる量は呼ぶ側が決める。パートごとに送る量が違うため。
    pub fn process(&mut self, x: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut l = Vec::with_capacity(x.len());
        let mut r = Vec::with_capacity(x.len());
        for &v in x {
            let (a, b) = self.run(v);
            l.push(a);
            r.push(b);
        }
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rms;

    /// 叩いたときの反応を見る。残響の性質はここに全部出る。
    fn impulse(secs: f32, spread: f32, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut rv = Reverb::new(secs, spread);
        let mut x = vec![0.0f32; n];
        x[0] = 1.0;
        rv.process(&x)
    }

    #[test]
    fn a_tail_actually_comes_out() {
        let (l, _) = impulse(2.0, 4.0, (3.0 * SR) as usize);
        // 頭の直後は、まだ最初の反射が来ていない
        assert!(rms(&l[..500]) < 0.2, "遅れずに出ている");
        // 少し後には鳴っている
        let mid = rms(&l[(0.3 * SR) as usize..(0.5 * SR) as usize]);
        assert!(mid > 1e-5, "残響が出ていない: {mid}");
    }

    #[test]
    fn the_tail_decays() {
        let n = (4.0 * SR) as usize;
        let (l, _) = impulse(2.0, 4.0, n);
        let a = rms(&l[(0.2 * SR) as usize..(0.4 * SR) as usize]);
        let b = rms(&l[(1.5 * SR) as usize..(1.7 * SR) as usize]);
        let c = rms(&l[(3.0 * SR) as usize..(3.2 * SR) as usize]);
        assert!(b < a, "減っていない {a} -> {b}");
        assert!(c < b, "減り続けていない {b} -> {c}");
        assert!(c < a * 0.1, "10分の1まで落ちていない");
    }

    #[test]
    fn longer_setting_gives_a_longer_tail() {
        let n = (5.0 * SR) as usize;
        let short = impulse(0.5, 4.0, n).0;
        let long = impulse(4.0, 4.0, n).0;
        let at = (2.0 * SR) as usize;
        let a = rms(&short[at..at + (0.2 * SR) as usize]);
        let b = rms(&long[at..at + (0.2 * SR) as usize]);
        assert!(b > a * 3.0, "長さの指定が効いていない {a} vs {b}");
    }

    #[test]
    fn the_tail_does_not_run_away() {
        // 戻す量が 1 を超えると、鳴りっぱなしになって音が壊れる
        for secs in [0.05f32, 1.0, 5.0, 20.0, 100.0] {
            let (l, r) = impulse(secs, 4.0, (6.0 * SR) as usize);
            assert!(l.iter().all(|v| v.is_finite()), "{secs}秒: 値が飛んだ");
            let peak = crate::peak(&l).max(crate::peak(&r));
            assert!(peak < 4.0, "{secs}秒: 大きくなりすぎ peak={peak}");
            // 終わりのほうでは必ず小さくなっていること
            let tail = rms(&l[l.len() - 4800..]);
            assert!(tail < 0.05, "{secs}秒: 鳴りっぱなし tail={tail}");
        }
    }

    #[test]
    fn spread_opens_the_stereo_image() {
        let n = (2.0 * SR) as usize;
        let narrow = impulse(2.0, 0.0, n);
        let wide = impulse(2.0, 8.0, n);
        let diff = |(l, r): &(Vec<f32>, Vec<f32>)| -> f32 {
            let d: Vec<f32> = l.iter().zip(r).map(|(a, b)| a - b).collect();
            rms(&d)
        };
        assert!(diff(&wide) > 0.0, "広がりが出ていない");
        // 広がり 0 は「完全に中央」。左右が同じになるのが正しい
        //（MIX の width: 0.0 と同じ意味にしてある）
        assert!(diff(&narrow) < 1e-6, "広がり 0 なのに左右が違う: {}", diff(&narrow));
    }

    #[test]
    fn high_notes_fade_before_low_ones() {
        // 実際の部屋は高い音から先に消える
        let n = (3.0 * SR) as usize;
        let (l, _) = impulse(3.0, 4.0, n);
        let early = &l[(0.2 * SR) as usize..(0.4 * SR) as usize];
        let late = &l[(2.0 * SR) as usize..(2.2 * SR) as usize];
        let hi = |x: &[f32]| rms(&crate::filter::highpass(x, 4000.0)) / rms(x).max(1e-12);
        assert!(hi(late) < hi(early), "高い音が先に消えていない");
    }

    #[test]
    fn silence_in_silence_out() {
        let mut rv = Reverb::new(2.0, 4.0);
        let (l, r) = rv.process(&vec![0.0f32; 4800]);
        assert!(l.iter().all(|v| *v == 0.0), "無音から音が出た");
        assert!(r.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn clearing_forgets_the_tail() {
        let mut rv = Reverb::new(3.0, 4.0);
        let mut x = vec![0.0f32; 4800];
        x[0] = 1.0;
        rv.process(&x);
        rv.clear();
        let (l, _) = rv.process(&vec![0.0f32; 4800]);
        assert!(l.iter().all(|v| v.abs() < 1e-9), "溜まったものが残っている");
    }

    #[test]
    fn empty_input_is_fine() {
        let mut rv = Reverb::new(2.0, 4.0);
        let (l, r) = rv.process(&[]);
        assert!(l.is_empty() && r.is_empty());
    }

    #[test]
    fn it_does_not_ring_on_one_pitch() {
        // 櫛型が1本だと「ピーン」と音程が付く。散っていること。
        // 帯ごとの偏りを見て、極端に尖っていないか確かめる
        let (l, _) = impulse(2.0, 4.0, (2.0 * SR) as usize);
        let seg = &l[(0.3 * SR) as usize..(1.3 * SR) as usize];
        let bands: Vec<f32> = [200.0f32, 500.0, 1000.0, 2000.0, 4000.0]
            .iter()
            .map(|f| rms(&crate::filter::bandpass(seg, *f, 2.0)))
            .collect();
        let max = bands.iter().cloned().fold(0.0f32, f32::max);
        let min = bands.iter().cloned().fold(f32::MAX, f32::min);
        assert!(max / min.max(1e-12) < 60.0, "どこかの帯だけ突出: {bands:?}");
    }
}
