//! 音色。46 種。
//!
//! ここは Rust の関数として書いてあるもの。数で書いた基本の楽器は
//! [`crate::kit`]、曲ファイルで作るものは [`crate::recipe`]。
//!
//! Python 版（synth.py）からの移植。音を決めている数字も、層の重ね方も
//! そのまま持ってきてある。なぜその値なのかは Python 側のコメントに残る。
//!
//! 乱数は numpy 互換（`rng` を見ること）なので、同じ種なら Python と
//! 同じ波形になる。移植が合っているかを数字で確かめられる。

use crate::env;
use crate::filter::{self, bandpass, formants, highpass, LadderMode};
use crate::osc::{self, SR};
use crate::rng::{noise, Pcg64};
use crate::shape::{
    add_scaled, breath, driven, mul_in_place, partials, partials_break, scale, sine, sine_sweep,
    vib as vib_curve,
};

const TAU: f32 = std::f32::consts::TAU;

/// ラダーの方式と歪み。Python の `LADDER_MODE` / `LADDER_DRIVE` に当たる。
#[derive(Clone, Copy, Debug)]
pub struct Cfg {
    pub ladder_mode: LadderMode,
    pub ladder_drive: f32,
    /// スーパーソウのフィルタが閉じきったときのカットオフ
    pub saw_floor: f32,
    pub saw_drive: f32,
    /// ベースの「ざらつき」。中域の倍音をどれだけ足すか
    pub bass_grit: f32,
    pub bass_grit_drive: f32,
    pub wobble: Wobble,
}

/// ぐおんぐおん鳴るベースの設定。hz は編曲側がテンポから計算して入れる。
#[derive(Clone, Copy, Debug)]
pub struct Wobble {
    /// LFO の速さ（1秒あたり何回開閉するか）
    pub hz: f32,
    /// フィルタが閉じたとき／開いたときのカットオフ
    pub lo: f32,
    pub hi: f32,
    /// レゾナンス。上げるほど「ぐお」の芯が出る
    pub res: f32,
    pub drive: f32,
    /// LFO の形。1.0 で正弦、大きいほど閉じている時間が長くなる
    pub shape: f32,
    pub sub: f32,
}

impl Default for Wobble {
    fn default() -> Self {
        Self { hz: 5.8, lo: 105.0, hi: 2700.0, res: 0.80, drive: 3.4, shape: 1.7, sub: 0.60 }
    }
}

impl Default for Cfg {
    fn default() -> Self {
        Self {
            ladder_mode: LadderMode::Classic,
            ladder_drive: 0.0,
            saw_floor: 3000.0,
            saw_drive: 2.6,
            bass_grit: 0.50,
            bass_grit_drive: 8.0,
            wobble: Wobble::default(),
        }
    }
}

#[inline]
fn lad(cfg: &Cfg, x: &[f32], cut: &[f32], res: f32) -> Vec<f32> {
    filter::ladder(x, cut, res, cfg.ladder_mode, cfg.ladder_drive)
}

/// 時間の並び t[i] = i / SR。
#[inline]
fn times(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32 / SR).collect()
}

/// セント差を周波数の倍率へ。
#[inline]
fn cents(c: f32) -> f32 {
    2.0f32.powf(c / 1200.0)
}

/// 遅れて効くビブラートの倍率。`clip((t-delay)/grow, 0, 1) * depth` の形。
fn late_vib(n: usize, start: f32, grow: f32, depth: f32, rate: f32) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let d = ((t - start) / grow).clamp(0.0, 1.0) * depth;
            1.0 + d * (TAU * rate * t).sin()
        })
        .collect()
}

/// 掛け算した新しい配列。
fn mul(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x * y).collect()
}

/// 定数のカットオフを配列として渡すための入れ物。
#[inline]
fn flat(v: f32) -> [f32; 1] {
    [v]
}

// ================================================================ リード

/// 7声デチューンのスーパーソウ。リフとリードに使う。
pub fn supersaw(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let mut mix = vec![0.0f32; n];
    for c in [-16.0f32, -10.0, -5.0, 0.0, 5.0, 10.0, 16.0] {
        let ph = rng.next_f64() as f32;
        add_scaled(&mut mix, &osc::saw(freq * cents(c), n, ph), 1.0);
    }
    // デチューンした波を足すと打ち消し合って実効レベルが下がるので
    // 単純平均ではなく sqrt で割って音量を揃える
    scale(&mut mix, 1.0 / 7.0f32.sqrt());
    let amp = env::adsr(n, 0.003, 0.05, 0.72, (n.max(1) as f32 * 0.25).max(1.0) / SR);
    let fenv = env::ad(n, 0.002, 0.10, 2.5);
    let cut: Vec<f32> = fenv
        .iter()
        .map(|f| cfg.saw_floor + 7000.0 * f * (0.5 + 0.5 * vel))
        .collect();
    mul_in_place(&mut mix, &amp);
    let mut out = driven(&lad(cfg, &mix, &cut, 0.30), cfg.saw_drive);
    scale(&mut out, 0.42 * vel);
    out
}

/// 太くて強いリード。9声スーパーソウ + オクターブ下 + 強めの歪み。
pub fn hardlead(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let v = late_vib(n, 0.18, 0.30, 0.008, 5.2);
    let mut mix = vec![0.0f32; n];
    for c in [-24.0f32, -16.0, -9.0, -4.0, 0.0, 4.0, 9.0, 16.0, 24.0] {
        let f: Vec<f32> = v.iter().map(|x| freq * x * cents(c)).collect();
        let ph = rng.next_f64() as f32;
        add_scaled(&mut mix, &osc::saw_var(&f, ph), 1.0);
    }
    scale(&mut mix, 1.0 / 9.0f32.sqrt());
    // 1オクターブ下を重ねて太さを出す
    let f: Vec<f32> = v.iter().map(|x| freq * 0.5 * x).collect();
    let ph = rng.next_f64() as f32;
    add_scaled(&mut mix, &osc::saw_var(&f, ph), 0.48);

    let amp = env::lead(n, 0.003, 0.10, 0.62, 0.07);
    let fenv = env::ad(n, 0.002, 0.11, 2.4);
    let cut: Vec<f32> = fenv
        .iter()
        .map(|f| 1100.0 + 7500.0 * f * (0.5 + 0.5 * vel) + 1500.0 * vel)
        .collect();
    mul_in_place(&mut mix, &amp);
    let mut out = driven(&lad(cfg, &mix, &cut, 0.34), 3.4);
    scale(&mut out, 0.40 * vel);
    out
}

/// 歪ませずに明るく張るスーパーソウ。トランス寄りのリード。
pub fn brightsaw(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let v = late_vib(n, 0.20, 0.30, 0.008, 5.5);
    let mut mix = vec![0.0f32; n];
    for c in [-14.0f32, -8.0, -3.0, 0.0, 3.0, 8.0, 14.0] {
        let f: Vec<f32> = v.iter().map(|x| freq * x * cents(c)).collect();
        let ph = rng.next_f64() as f32;
        add_scaled(&mut mix, &osc::saw_var(&f, ph), 1.0);
    }
    scale(&mut mix, 1.0 / 7.0f32.sqrt());
    let amp = env::lead(n, 0.004, 0.09, 0.70, 0.08);
    let fenv = env::ad(n, 0.002, 0.13, 2.0);
    let cut: Vec<f32> = fenv
        .iter()
        .map(|f| 2200.0 + 9000.0 * f * (0.5 + 0.5 * vel) + 2000.0 * vel)
        .collect();
    mul_in_place(&mut mix, &amp);
    let mut out = driven(&lad(cfg, &mix, &cut, 0.22), 1.15);
    scale(&mut out, 0.52 * vel);
    out
}

/// 中空な矩形リード。音数が多くても濁らず、旋律の輪郭がはっきり出る。
pub fn squarelead(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let v = late_vib(n, 0.18, 0.28, 0.010, 5.6);
    let f: Vec<f32> = v.iter().map(|x| freq * x).collect();
    let f2: Vec<f32> = f.iter().map(|x| x * cents(9.0)).collect();
    let f3: Vec<f32> = f.iter().map(|x| x * 0.5).collect();
    let a = osc::square_var(&f, rng.next_f64() as f32);
    let b = osc::square_var(&f2, rng.next_f64() as f32);
    let c = osc::square_var(&f3, rng.next_f64() as f32); // 1oct下で太さ
    let mut mix: Vec<f32> = (0..n).map(|i| 0.75 * a[i] + 0.25 * b[i] + 0.30 * c[i]).collect();
    let amp = env::lead(n, 0.003, 0.10, 0.66, 0.07);
    let fenv = env::ad(n, 0.001, 0.10, 2.6);
    let cut: Vec<f32> = fenv
        .iter()
        .map(|f| 1600.0 + 7000.0 * f * (0.5 + 0.5 * vel) + 1600.0 * vel)
        .collect();
    mul_in_place(&mut mix, &amp);
    let mut out = driven(&lad(cfg, &mix, &cut, 0.30), 1.7);
    scale(&mut out, 0.46 * vel);
    out
}

/// 立ち上がりは粒が立ち、伸ばすとちゃんと鳴り続けるリード。
pub fn pluck(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    // 0.20秒あたりから効き始めるビブラート。伸ばした音だけが歌う
    let v = late_vib(n, 0.20, 0.30, 0.009, 5.4);
    let f1: Vec<f32> = v.iter().map(|x| freq * x).collect();
    let f2: Vec<f32> = f1.iter().map(|x| x * cents(6.0)).collect();
    let a = osc::saw_var(&f1, rng.next_f64() as f32);
    let b = osc::square_var(&f2, rng.next_f64() as f32);
    let mut o: Vec<f32> = (0..n).map(|i| 0.62 * a[i] + 0.38 * b[i]).collect();
    let amp = env::lead(n, 0.004, 0.13, 0.52, 0.09);
    let fenv = env::ad(n, 0.001, 0.09, 2.8);
    // 持続部にもカットオフを残さないと伸ばしたときに消えてしまう
    let cut: Vec<f32> = fenv
        .iter()
        .map(|f| 750.0 + 6200.0 * f * (0.5 + 0.5 * vel) + 1100.0 * vel)
        .collect();
    mul_in_place(&mut o, &amp);
    let mut body = lad(cfg, &o, &cut, 0.44);
    let f3: Vec<f32> = f1.iter().map(|x| x * 2.0).collect();
    let mut shine = osc::saw_var(&f3, rng.next_f64() as f32);
    mul_in_place(&mut shine, &amp);
    add_scaled(&mut body, &shine, 0.13);
    let mut out = driven(&body, 1.35);
    scale(&mut out, 0.50 * vel);
    out
}

/// 短いコードスタブ。裏拍の刻み用。
pub fn stab(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let a = osc::saw(freq, n, rng.next_f64() as f32);
    let b = osc::saw(freq * cents(7.0), n, rng.next_f64() as f32);
    let mut mix: Vec<f32> = a.iter().zip(&b).map(|(x, y)| (x + y) * 0.7).collect();
    let amp = env::ad(n, 0.002, 0.055, 4.0);
    let fenv = env::ad(n, 0.001, 0.04, 3.0);
    let cut: Vec<f32> = fenv.iter().map(|f| 800.0 + 3200.0 * f).collect();
    mul_in_place(&mut mix, &amp);
    let mut out = driven(&lad(cfg, &mix, &cut, 0.38), 1.3);
    scale(&mut out, 0.30 * vel);
    out
}

/// シンセブラス。刺すための音。
///
/// ブラスらしさは倍音の量ではなく「立ち上がりで倍音が遅れて開く」ところ。
pub fn brass(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    // 立ち上がりだけ音程が上から降りてくる。ごく浅く、短く
    let bend: Vec<f32> = t.iter().map(|x| 1.0 + 0.010 * (-x / 0.020).exp()).collect();
    let mut o = vec![0.0f32; n];
    for (det, amp) in [(0.0f32, 1.0f32), (7.0, 0.7), (-8.0, 0.7), (14.0, 0.4)] {
        let f = freq * cents(det);
        let ph0 = rng.next_f64() as f32 * 6.283;
        // ノコギリを位相から直接作る（帯域は下のフィルタで抑える）
        let mut ph = 0.0f64;
        for i in 0..n {
            ph += (f * bend[i]) as f64 / SR as f64;
            let p = (std::f64::consts::TAU * ph) as f32 + ph0;
            let frac = (p / TAU).rem_euclid(1.0);
            o[i] += (2.0 * frac - 1.0) * amp;
        }
    }
    scale(&mut o, 1.0 / 2.8);
    let amp = env::lead(n, 0.012, 0.10, 0.72, 0.05);
    // 息が入ってから鳴りが立つ。開いて、少し戻る
    let open = env::ad(n, 0.030, 0.16, 1.6);
    let cut: Vec<f32> = open.iter().map(|x| 380.0 + 3600.0 * x * (0.5 + 0.5 * vel)).collect();
    mul_in_place(&mut o, &amp);
    let mut out = driven(&lad(cfg, &o, &cut, 0.30), 2.2);
    scale(&mut out, 0.58 * vel);
    out
}

/// 透き通るベル系。部分音を数本だけ選んで鳴らすので、隙間があって澄む。
pub fn crystal(_cfg: &Cfg, freq: f32, n: usize, vel: f32, _seed: u64) -> Vec<f32> {
    let v = late_vib(n, 0.25, 0.35, 0.006, 4.8);
    // 位相を積む（Python の cumsum(freq*vib)/SR）
    let mut ph = vec![0.0f64; n];
    let mut det = vec![0.0f64; n];
    let (mut a, mut b) = (0.0f64, 0.0f64);
    let dc = cents(7.0);
    for i in 0..n {
        a += (freq * v[i]) as f64 / SR as f64;
        b += (freq * v[i] * dc) as f64 / SR as f64;
        ph[i] = a;
        det[i] = b;
    }
    let mut out = vec![0.0f32; n];
    // (倍率, 音量, 減衰) 上の部分音ほど速く消えるのがベルらしさ
    for (mult, amp_, dec) in [
        (1.0f64, 1.00f32, 1.10f32),
        (2.0, 0.40, 0.55),
        (3.0, 0.18, 0.34),
        (4.0, 0.10, 0.22),
        (6.0, 0.05, 0.15),
    ] {
        let e = env::ad(n, 0.004, dec, 2.0);
        for i in 0..n {
            out[i] += (std::f64::consts::TAU * ph[i] * mult).sin() as f32 * amp_ * e[i];
        }
    }
    let e1 = env::ad(n, 0.006, 0.9, 2.0);
    let e2 = env::lead(n, 0.02, 0.30, 0.40, 0.14);
    let mut res = vec![0.0f32; n];
    for i in 0..n {
        let shimmer = (std::f64::consts::TAU * det[i]).sin() as f32 * e1[i] * 0.35;
        // 伸ばした音が消えないようにサステイン層を足す
        let sus = (std::f64::consts::TAU * ph[i]).sin() as f32 * e2[i] * 0.55;
        res[i] = (out[i] * 0.5 + shimmer + sus) * (0.52 * vel);
    }
    res
}

/// やわらかい弦。立ち上がりが遅く、伸ばしで歌う。
pub fn strings(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let v = late_vib(n, 0.15, 0.30, 0.010, 5.0);
    let mut mix = vec![0.0f32; n];
    for c in [-9.0f32, -4.0, 0.0, 4.0, 9.0] {
        let f: Vec<f32> = v.iter().map(|x| freq * x * cents(c)).collect();
        add_scaled(&mut mix, &osc::saw_var(&f, rng.next_f64() as f32), 1.0);
    }
    scale(&mut mix, 1.0 / 5.0f32.sqrt());
    let amp = env::lead(n, 0.035, 0.12, 0.78, 0.13); // ゆっくり立ち上がる
    let e = env::ad(n, 0.03, 0.4, 1.5);
    let cut: Vec<f32> = e.iter().map(|x| 1800.0 + 3200.0 * vel + 1500.0 * x).collect();
    mul_in_place(&mut mix, &amp);
    let mut out = lad(cfg, &mix, &cut, 0.16);
    scale(&mut out, 0.50 * vel);
    out
}

/// 琴／古筝寄りの撥弦音。硬い立ち上がりで抜ける。
pub fn koto(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let v = late_vib(n, 0.22, 0.30, 0.011, 5.8);
    let f: Vec<f32> = v.iter().map(|x| freq * x).collect();
    let a = osc::saw_var(&f, rng.next_f64() as f32);
    let b = osc::square_var(&f, rng.next_f64() as f32);
    let mut o: Vec<f32> = (0..n).map(|i| 0.55 * a[i] + 0.45 * b[i]).collect();
    mul_in_place(&mut o, &env::lead(n, 0.0012, 0.11, 0.40, 0.08));
    let fenv = env::ad(n, 0.0008, 0.06, 3.4);
    let cut: Vec<f32> = fenv
        .iter()
        .map(|x| 1400.0 + 8000.0 * x * (0.5 + 0.5 * vel) + 1200.0 * vel)
        .collect();
    let mut out = lad(cfg, &o, &cut, 0.30);
    // 撥弦のアタック音
    let mut pick = noise(n, seed);
    mul_in_place(&mut pick, &env::ad(n, 0.0003, 0.006, 6.0));
    scale(&mut pick, 0.22);
    add_scaled(&mut out, &highpass(&pick, 2500.0), 1.0);
    let mut out = driven(&out, 1.25);
    scale(&mut out, 0.62 * vel);
    out
}

/// チェンバロ。爪で弦を弾く鍵盤楽器。
pub fn harpsi(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let mut out = vec![0.0f32; n];
    for (mul_, lvl) in [(1.0f32, 1.0f32), (2.0, 0.55)] {
        // 8フィート + 4フィート
        for (k, amp, dec) in [
            (1.0f32, 1.00f32, 0.85f32),
            (2.0, 0.70, 0.55),
            (3.0, 0.55, 0.40),
            (4.0, 0.40, 0.30),
            (5.0, 0.30, 0.24),
            (6.0, 0.22, 0.20),
            (7.0, 0.16, 0.17),
            (8.0, 0.12, 0.15),
        ] {
            let f = freq * mul_ * k;
            if f > SR * 0.45 {
                break;
            }
            let ph = rng.next_f64() as f32 * 6.283;
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / SR;
                *o += (TAU * f * t + ph).sin() * amp * lvl * (-t / dec).exp();
            }
        }
    }
    scale(&mut out, 1.0 / 4.2);
    // 爪が弦を離す瞬間の音。これが無いとただのオルガンになる
    let mut q = highpass(&noise(n, seed + 3), 3000.0);
    mul_in_place(&mut q, &env::ad(n, 0.0002, 0.008, 6.0));
    add_scaled(&mut out, &q, 0.45);
    let mut out = driven(&out, 1.4);
    scale(&mut out, 0.52 * vel);
    out
}

/// ベル。倍音が整数倍でないので、澄んでいるのに不思議に響く。
pub fn bell(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    // 実際の鐘の部分音の比。整数倍ではない
    let tab = [
        (0.50f32, 0.45f32, 1.6f32),
        (1.00, 1.00, 1.3),
        (1.19, 0.55, 0.9),
        (1.83, 0.40, 0.7),
        (2.44, 0.28, 0.5),
        (3.26, 0.18, 0.35),
        (4.07, 0.12, 0.25),
    ];
    let mut out = partials_break(freq, n, &tab, &mut rng, SR * 0.45);
    scale(&mut out, 1.0 / 2.6);
    let mut strike = highpass(&noise(n, seed + 5), 4000.0);
    mul_in_place(&mut strike, &env::ad(n, 0.0002, 0.006, 7.0));
    add_scaled(&mut out, &strike, 0.30);
    scale(&mut out, 0.50 * vel);
    out
}

/// オルガン。減衰しない。押している間ずっと同じ音量で鳴る。
pub fn organ(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut out = vec![0.0f32; n];
    for (mul_, amp) in [
        (0.5f32, 0.55f32),
        (1.0, 1.00),
        (1.5, 0.35),
        (2.0, 0.70),
        (3.0, 0.30),
        (4.0, 0.40),
        (8.0, 0.18),
    ] {
        let f = freq * mul_;
        if f > SR * 0.45 {
            continue;
        }
        add_scaled(&mut out, &sine(f, n, 0.0), amp);
    }
    scale(&mut out, 1.0 / 3.5);
    let amp = env::adsr(n, 0.006, 0.02, 0.95, 0.04f32.min(n as f32 / SR * 0.2));
    mul_in_place(&mut out, &amp);
    // 鍵盤を押した瞬間のカチッ（キークリック）
    let mut click = highpass(&noise(n, seed), 2500.0);
    mul_in_place(&mut click, &env::ad(n, 0.0002, 0.005, 8.0));
    add_scaled(&mut out, &click, 0.25);
    let mut out = driven(&out, 1.8);
    scale(&mut out, 0.46 * vel);
    out
}

/// スチールパン。金属の面を叩く音。
pub fn steelpan(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let tab = [
        (1.00f32, 1.00f32, 0.55f32),
        (2.00, 0.72, 0.38),
        (3.01, 0.34, 0.26),
        (4.02, 0.20, 0.18),
        (5.98, 0.12, 0.13),
    ];
    let mut out = partials_break(freq, n, &tab, &mut rng, SR * 0.45);
    scale(&mut out, 1.0 / 2.4);
    let mut hit = highpass(&noise(n, seed + 2), 1800.0);
    mul_in_place(&mut hit, &env::ad(n, 0.0004, 0.020, 4.0));
    add_scaled(&mut out, &hit, 0.34);
    let mut out = driven(&out, 1.6);
    scale(&mut out, 0.54 * vel);
    out
}

/// マリンバ。木を叩く音。木琴の板は4倍音に合わせて削ってある。
pub fn marimba(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let tab = [(1.00f32, 1.00f32, 0.42f32), (4.00, 0.42, 0.16), (9.20, 0.14, 0.08)];
    let mut out = partials_break(freq, n, &tab, &mut rng, SR * 0.45);
    scale(&mut out, 1.0 / 1.6);
    let raw = noise(n, seed + 1);
    let hp = highpass(&raw, 3000.0);
    let mut mallet: Vec<f32> = raw.iter().zip(&hp).map(|(a, b)| a - b).collect();
    mul_in_place(&mut mallet, &env::ad(n, 0.0005, 0.012, 5.0));
    add_scaled(&mut out, &mallet, 0.28);
    scale(&mut out, 0.56 * vel);
    out
}

/// 声のようなパッド。ゆっくり立ち上がる。
pub fn choir(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let mut o = vec![0.0f32; n];
    for det in [0.0f32, 9.0, -11.0, 5.0, -6.0] {
        add_scaled(&mut o, &osc::saw(freq * cents(det), n, rng.next_f64() as f32), 1.0);
    }
    scale(&mut o, 1.0 / 5.0);
    let amp = env::lead(n, 0.09, 0.20, 0.85, 0.12);
    let x = mul(&o, &amp);
    // フォルマント。620Hz と 1150Hz あたりを持ち上げる（「あ」に近い）
    let mut out = vec![0.0f32; n];
    for (f0, lvl) in [(620.0f32, 1.0f32), (1150.0, 0.7)] {
        let hi = lad(cfg, &x, &flat(f0 * 1.6), 0.55);
        let lo = lad(cfg, &x, &flat(f0 * 0.6), 0.0);
        for i in 0..n {
            out[i] += (hi[i] - lo[i]) * lvl;
        }
    }
    let mut br = highpass(&noise(n, seed + 4), 4000.0);
    mul_in_place(&mut br, &amp);
    scale(&mut br, 0.04);
    let g = 0.42 * vel;
    (0..n).map(|i| (out[i] * 0.55 + br[i]) * g).collect()
}

/// 三味線。撥で叩く音と、サワリ（棹に触れて細かくビビる）でできている。
pub fn shamisen(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    // 弦。上の倍音ほど速く減衰する
    let tab = [
        (1.0f32, 1.00f32, 0.62f32),
        (2.0, 0.62, 0.40),
        (3.0, 0.44, 0.28),
        (4.0, 0.26, 0.20),
        (5.0, 0.18, 0.15),
        (6.0, 0.11, 0.12),
    ];
    let mut body = partials_break(freq, n, &tab, &mut rng, SR * 0.45);
    scale(&mut body, 1.0 / 2.6);

    // サワリ。整数倍からわずかにずらした高い倍音を、長めに残す
    let mut buzz = vec![0.0f32; n];
    for (k, amp) in [(7.0f32, 0.34f32), (9.0, 0.26), (11.0, 0.20), (13.0, 0.14)] {
        let sign = if (k as i32) % 2 != 0 { 1.0 } else { -1.0 };
        let f = freq * (k + 0.13 * sign);
        if f > SR * 0.45 {
            break;
        }
        let ph = rng.next_f64() as f32 * 6.283;
        for i in 0..n {
            buzz[i] += (TAU * f * t[i] + ph).sin() * amp * (-t[i] / 0.34).exp();
        }
    }
    // ビビりは鳴りはじめが強く、すぐ落ち着く
    for i in 0..n {
        buzz[i] *= 0.45 + 0.55 * (-t[i] / 0.05).exp();
    }
    // 撥が当たる瞬間。皮を叩く成分
    let mut hit = highpass(&noise(n, seed + 7), 2200.0);
    mul_in_place(&mut hit, &env::ad(n, 0.0003, 0.014, 5.0));

    add_scaled(&mut body, &buzz, 0.42);
    add_scaled(&mut body, &hit, 0.55);
    let mut out = driven(&body, 1.9);
    scale(&mut out, 0.62 * vel);
    out
}

/// 打弦のピアノ。倍音ごとの減衰差・非調和性・強弱で鍵盤の音になる。
pub fn piano(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let t = times(n);
    let inharm = 0.00042f32;
    let nmax = ((SR * 0.45 / freq.max(20.0)) as i32).clamp(1, 16);
    // 減衰が速すぎると伸ばした音が音価の途中で消える
    let base_dec = 0.75 + 2.8 * (freq / 900.0); // 低音ほど長く鳴る
    let bright = 0.34 + 0.66 * vel;

    let mut out = vec![0.0f32; n];
    for k in 1..=nmax {
        let kf = k as f32;
        let fk = freq * kf * (1.0 + inharm * kf * kf).sqrt();
        if fk > SR * 0.45 {
            break;
        }
        let amp = (1.0 / kf.powf(1.28)) * bright.powi((k - 1).min(6));
        let rate = base_dec * (1.0 + 0.5 * (kf - 1.0));
        for i in 0..n {
            let e = (-rate * t[i]).exp();
            out[i] += (TAU * fk * t[i]).sin() * amp * e;
            if k <= 3 {
                // 3本弦のわずかなうねり
                out[i] += (TAU * fk * 1.0004 * t[i] + 0.7).sin() * amp * 0.45 * e;
            }
        }
    }
    scale(&mut out, 1.0 / 1.9);

    let a = ((0.0018 * SR) as usize).min(n); // ハンマーの立ち上がり
    for i in 0..a {
        let f = if a <= 1 { 0.0 } else { i as f32 / (a - 1) as f32 };
        out[i] *= f;
    }
    let r = ((0.090 * SR) as usize).min(n); // 鍵盤を離す
    if r > 0 {
        let start = n - r;
        for (i, v) in out[start..].iter_mut().enumerate() {
            let f = if r <= 1 { 1.0 } else { 1.0 - i as f32 / (r - 1) as f32 };
            *v *= f;
        }
    }
    let mut hammer = noise(n, seed);
    mul_in_place(&mut hammer, &env::ad(n, 0.0004, 0.007, 6.0));
    scale(&mut hammer, 0.18 * vel);
    add_scaled(&mut out, &highpass(&hammer, 1800.0), 1.0);
    scale(&mut out, 0.60 * vel);
    out
}

// ================================================================ ベース

/// レゾナンス強めのアシッドベース。サブと中域のざらつきを足す。
pub fn acid(cfg: &Cfg, freq: f32, n: usize, vel: f32, _seed: u64) -> Vec<f32> {
    let sa = osc::saw(freq, n, 0.0);
    let sq = osc::square(freq, n, 0.0);
    let o: Vec<f32> = (0..n).map(|i| 0.7 * sa[i] + 0.3 * sq[i]).collect();
    let amp = env::adsr(n, 0.002, 0.03, 0.85, (n as f32 * 0.2).max(1.0) / SR);
    let fenv = env::ad(n, 0.001, 0.055, 3.5);
    let cut: Vec<f32> = fenv.iter().map(|f| 190.0 + 2800.0 * f * (0.45 + 0.55 * vel)).collect();
    let raw = mul(&o, &amp);
    let mut body = driven(&lad(cfg, &raw, &cut, 0.62), 2.4);
    // 1オクターブ下のサイン。フィルタを通さず土台として鳴らす
    let mut sub = sine(freq * 0.5, n, 0.0);
    mul_in_place(&mut sub, &amp);
    scale(&mut sub, 0.55);
    add_scaled(&mut body, &sub, 1.0);

    if cfg.bass_grit > 0.0 {
        // 歪ませてから 180Hz〜4kHz だけ取り出す。下を切らないと土台の
        // サインと喧嘩し、上を切らないとベースなのに耳に刺さる
        let sat: Vec<f32> = raw.iter().map(|v| (v * cfg.bass_grit_drive).tanh()).collect();
        let mut g = lad(cfg, &highpass(&sat, 180.0), &flat(4000.0), 0.0);
        let r_raw = crate::rms(&raw);
        let r_g = crate::rms(&g);
        scale(&mut g, r_raw / (r_g + 1e-12));
        add_scaled(&mut body, &g, cfg.bass_grit);
    }
    scale(&mut body, 0.40 * vel);
    body
}

/// ぐおんぐおん鳴るベース。フィルタの開き具合を LFO で往復させている。
pub fn wobble(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let w = cfg.wobble;
    let t = times(n);
    let mut rng = Pcg64::new(seed);
    // 少しずつ音程をずらしたノコギリを3本。うなりで太くする
    let a = osc::saw(freq, n, rng.next_f64() as f32);
    let b = osc::saw(freq * cents(12.0), n, rng.next_f64() as f32);
    let c = osc::saw(freq * cents(-13.0), n, rng.next_f64() as f32);
    let sq = osc::square(freq * 0.5, n, 0.0);
    // 1オクターブ下の矩形波を混ぜて、閉じたときの芯を作る
    let o: Vec<f32> =
        (0..n).map(|i| 0.72 * ((a[i] + b[i] + c[i]) / 3.0) + 0.28 * sq[i]).collect();
    let amp = env::adsr(n, 0.004, 0.02, 0.94, 0.06f32.min(n as f32 / SR * 0.25));
    // LFO。音符の頭から始まるので、譜面どおりの位置で「ぐお」が来る
    let cut: Vec<f32> = t
        .iter()
        .map(|x| {
            let l = (0.5 - 0.5 * (TAU * w.hz * x).cos()).powf(w.shape);
            w.lo + (w.hi - w.lo) * l
        })
        .collect();
    let mut body = driven(&lad(cfg, &mul(&o, &amp), &cut, w.res), w.drive);
    // 土台のサイン。フィルタを通さないので、閉じている間も低音が残る
    let mut sub = sine(freq * 0.5, n, 0.0);
    mul_in_place(&mut sub, &amp);
    add_scaled(&mut body, &sub, w.sub);
    scale(&mut body, 0.34 * vel);
    body
}

/// リース。わずかにずらしたノコギリ波どうしの打ち消し合いが「ウネウネ」。
pub fn reese(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let d = 0.008 + 0.004 * vel; // ずらし幅（うねりの速さ）
    let a = osc::saw(freq * (1.0 - d), n, rng.next_f64() as f32);
    let b = osc::saw(freq * (1.0 + d), n, rng.next_f64() as f32);
    let c = osc::saw(freq * (1.0 + d * 2.3), n, rng.next_f64() as f32);
    let o: Vec<f32> = (0..n).map(|i| a[i] + b[i] * 0.95 + c[i] * 0.45).collect();
    let amp = env::adsr(n, 0.006, 0.05, 0.90, 0.06);
    let fenv = env::ad(n, 0.004, 0.20, 2.0);
    let cut: Vec<f32> = fenv.iter().map(|f| 260.0 + 1500.0 * f * (0.4 + 0.6 * vel)).collect();
    let mut x = mul(&o, &amp);
    scale(&mut x, 0.42);
    let mut out = driven(&lad(cfg, &x, &cut, 0.34), 1.8);
    let mut sub = sine(freq * 0.5, n, 0.0);
    mul_in_place(&mut sub, &amp);
    add_scaled(&mut out, &sub, 0.42);
    scale(&mut out, 0.50 * vel);
    out
}

/// FMベース。DX7 のあれ。頭だけ金属質で、伸ばすと正弦に戻る。
pub fn fmbass(_cfg: &Cfg, freq: f32, n: usize, vel: f32, _seed: u64) -> Vec<f32> {
    let t = times(n);
    let amp = env::adsr(n, 0.002, 0.07, 0.80, 0.05);
    let g = 0.42 * vel;
    (0..n)
        .map(|i| {
            let x = t[i];
            let idx = (2.6 + 3.4 * vel) * (-x / 0.055).exp() + 0.35 * (-x / 0.5).exp();
            let m = (TAU * freq * 2.0 * x).sin() * idx
                + (TAU * freq * 3.0 * x).sin() * idx * 0.35;
            let car = (TAU * freq * x + m).sin();
            let sub = (TAU * freq * 0.5 * x).sin() * amp[i] * 0.30;
            (car * amp[i] * 0.85 + sub) * g
        })
        .collect()
}

/// ドンク。裏拍で跳ねる、極端に短いベース。
pub fn donk(cfg: &Cfg, freq: f32, n: usize, vel: f32, _seed: u64) -> Vec<f32> {
    let sq = osc::square(freq, n, 0.0);
    let sa = osc::saw(freq * 2.0, n, 0.0);
    let o: Vec<f32> = (0..n).map(|i| 0.55 * sq[i] + 0.45 * sa[i]).collect();
    let amp = env::ad(n, 0.0012, 0.075, 3.6);
    let fenv = env::ad(n, 0.0008, 0.028, 4.5);
    let cut: Vec<f32> = fenv.iter().map(|f| 240.0 + 4200.0 * f * (0.4 + 0.6 * vel)).collect();
    let mut out = driven(&lad(cfg, &mul(&o, &amp), &cut, 0.72), 3.0);
    scale(&mut out, 0.72 * vel);
    out
}

/// 808。頭で音程が落ちてくる長い正弦。トラップの土台。
pub fn tr808(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let f: Vec<f32> = (0..n)
        .map(|i| freq * (1.0 + 1.6 * (-(i as f32 / SR) / 0.028).exp()))
        .collect();
    let x = sine_sweep(&f, 0.0);
    let amp = env::ad(n, 0.0015, 0.85, 1.5);
    let mut click = noise(n, seed);
    mul_in_place(&mut click, &env::ad(n, 0.0003, 0.004, 6.0));
    let click = highpass(&click, 1600.0);
    let mut out = driven(&mul(&x, &amp), 1.6 + 1.4 * vel);
    add_scaled(&mut out, &click, 0.12);
    scale(&mut out, 0.50 * vel);
    out
}

/// グロウル。リースにフィルタの往復を重ねる。開閉が速いと言葉に聞こえる。
pub fn growl(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    let d = 0.010f32;
    let a = osc::saw(freq * (1.0 - d), n, rng.next_f64() as f32);
    let b = osc::saw(freq * (1.0 + d), n, rng.next_f64() as f32);
    let c = osc::square(freq, n, rng.next_f64() as f32);
    let o: Vec<f32> = (0..n).map(|i| a[i] + b[i] * 0.9 + c[i] * 0.35).collect();
    let amp = env::adsr(n, 0.004, 0.05, 0.88, 0.05);
    // 開閉。速さを音符の中で上げていくと「うなり」から「言葉」になる
    let mut ph = 0.0f64;
    let mut cut = vec![0.0f32; n];
    for i in 0..n {
        let rate = 7.0 + 9.0 * (t[i] / 0.35).clamp(0.0, 1.0);
        ph += rate as f64 / SR as f64;
        let lfo = 0.5 + 0.5 * (std::f64::consts::TAU * ph).sin() as f32;
        cut[i] = 180.0 + 3200.0 * lfo.powf(1.5) * (0.45 + 0.55 * vel);
    }
    let mut x = mul(&o, &amp);
    scale(&mut x, 0.4);
    let mut body = driven(&lad(cfg, &x, &cut, 0.78), 3.2);
    let voice = bandpass(&body, 900.0, 3.0); // 声っぽさを足す
    add_scaled(&mut body, &voice, 0.30);
    let mut sub = sine(freq * 0.5, n, 0.0);
    mul_in_place(&mut sub, &amp);
    add_scaled(&mut body, &sub, 0.45);
    scale(&mut body, 0.46 * vel);
    body
}

/// ハードスタイルのベース。低い所は空けて中域だけで押す。
pub fn hardbass(cfg: &Cfg, freq: f32, n: usize, vel: f32, _seed: u64) -> Vec<f32> {
    let sa = osc::saw(freq, n, 0.0);
    let sq = osc::square(freq * 0.5, n, 0.0);
    let o: Vec<f32> = (0..n).map(|i| 0.6 * sa[i] + 0.4 * sq[i]).collect();
    let amp = env::ad(n, 0.0015, 0.095, 3.0);
    let body = driven(&mul(&o, &amp), 4.5);
    let body = highpass(&body, 110.0); // 低域はキックに譲る
    let fenv = env::ad(n, 0.001, 0.05, 3.0);
    let cut: Vec<f32> = fenv.iter().map(|f| 400.0 + 3000.0 * f * (0.4 + 0.6 * vel)).collect();
    let mut out = lad(cfg, &body, &cut, 0.55);
    scale(&mut out, 0.68 * vel);
    out
}

/// 指弾きのエレキベース。生っぽい低音。
pub fn fingerbass(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let sa = osc::saw(freq, n, rng.next_f64() as f32);
    let si = sine(freq, n, 0.0);
    let o: Vec<f32> = (0..n).map(|i| sa[i] * 0.55 + si[i] * 0.45).collect();
    let amp = env::lead(n, 0.006, 0.22, 0.42, 0.14);
    let fenv = env::ad(n, 0.004, 0.10, 2.6);
    let cut: Vec<f32> = fenv.iter().map(|f| 180.0 + 1500.0 * f * (0.4 + 0.6 * vel)).collect();
    let mut body = lad(cfg, &mul(&o, &amp), &cut, 0.26);
    let res = formants(&body, &[(110.0, 3.5, 0.6), (700.0, 5.0, 0.20)]);
    add_scaled(&mut body, &res, 1.0);
    let mut fing = noise(n, seed);
    mul_in_place(&mut fing, &env::ad(n, 0.0006, 0.012, 4.0));
    let fing = highpass(&fing, 1200.0);
    let mut out = driven(&body, 1.35);
    add_scaled(&mut out, &fing, 0.10);
    scale(&mut out, vel);
    out
}

/// ウッドベース。弦より胴の共鳴のほうが音量を持っている。
pub fn upright(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let a = osc::saw(freq, n, rng.next_f64() as f32);
    let b = osc::saw(freq * 1.002, n, rng.next_f64() as f32);
    let o: Vec<f32> = (0..n).map(|i| a[i] * 0.5 + b[i] * 0.5).collect();
    let amp = env::ad(n, 0.008, 0.55, 2.0);
    let fenv = env::ad(n, 0.006, 0.09, 2.4);
    let cut: Vec<f32> = fenv.iter().map(|f| 130.0 + 900.0 * f).collect();
    let body = lad(cfg, &mul(&o, &amp), &cut, 0.20);
    let res = formants(&body, &[(72.0, 6.0, 1.0), (160.0, 5.0, 0.6), (420.0, 4.0, 0.22)]);
    let mut thud = noise(n, seed);
    mul_in_place(&mut thud, &env::ad(n, 0.001, 0.018, 3.5));
    let thud = highpass(&thud, 300.0);
    let g = 1.88 * vel;
    (0..n).map(|i| (body[i] * 0.5 + res[i] * 1.5 + thud[i] * 0.10) * g).collect()
}

/// ランブル。音程を出さず、床を揺らすためだけの低音。
pub fn rumble(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    let d = 0.006f32;
    let a = osc::saw(freq * (1.0 - d), n, rng.next_f64() as f32);
    let b = osc::saw(freq * (1.0 + d), n, rng.next_f64() as f32);
    let o: Vec<f32> = (0..n).map(|i| a[i] + b[i]).collect();
    let amp = env::adsr(n, 0.030, 0.12, 0.95, 0.22);
    let ph = rng.next_f64() as f32 * 6.283;
    let drift: Vec<f32> = t.iter().map(|x| 150.0 + 60.0 * (TAU * 0.7 * x + ph).sin()).collect();
    let mut x = mul(&o, &amp);
    scale(&mut x, 0.45);
    let body = lad(cfg, &x, &drift, 0.30);
    let mut sub = sine(freq * 0.5, n, 0.0);
    mul_in_place(&mut sub, &amp);
    let g = 0.67 * vel;
    (0..n).map(|i| (body[i] * 0.8 + sub[i] * 0.55) * g).collect()
}

/// サブベース。40〜90Hz だけを受け持つ持続音。ほとんど正弦波。
pub fn sub(_cfg: &Cfg, freq: f32, n: usize, vel: f32, _seed: u64) -> Vec<f32> {
    let amp = env::lead(n, 0.010, 0.06, 0.94, 0.035);
    let mut x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            (TAU * freq * t).sin() + 0.10 * (2.0 * TAU * freq * t).sin()
        })
        .collect();
    mul_in_place(&mut x, &amp);
    // 軽く歪ませると、小さいスピーカーでも輪郭が残る
    let mut out = driven(&x, 1.25);
    scale(&mut out, 0.55 * vel);
    out
}

// ================================================================ 息もの

/// 笛の共通部分。倍音・息・チフ（吹き始めのカスレ）でできている。
#[allow(clippy::too_many_arguments)]
pub struct WindCfg {
    /// 倍音の量。奇数倍音が強いと「閉じた管」（尺八）
    pub harm: &'static [f32],
    /// 息の雑音の量。ここが笛らしさのほぼ全部
    pub air: f32,
    /// 吹き始めの「フッ」。舌で切る音
    pub chiff: f32,
    /// (速さ, 深さ, 掛かり始め)
    pub vib: (f32, f32, f32),
    /// 吹き始めの音程のしゃくり（半音単位）
    pub scoop: f32,
    pub atk: f32,
    pub dec: f32,
    pub sus: f32,
    pub rel: f32,
    pub cut: f32,
    pub tone: f32,
}

impl Default for WindCfg {
    fn default() -> Self {
        Self {
            harm: &[1.0, 0.20, 0.05],
            air: 0.18,
            chiff: 0.5,
            vib: (5.0, 0.007, 0.30),
            scoop: 0.0,
            atk: 0.045,
            dec: 0.10,
            sus: 0.86,
            rel: 0.10,
            cut: 6.0,
            tone: 1.0,
        }
    }
}

pub fn wind(cfg: &Cfg, w: &WindCfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let t = times(n);
    let v = vib_curve(n, w.vib.0, w.vib.1 * (0.6 + 0.6 * vel), w.vib.2, 0.35, seed);
    let mut f: Vec<f32> = v.iter().map(|x| freq * x).collect();
    if w.scoop != 0.0 {
        for i in 0..n {
            f[i] *= 2.0f32.powf(w.scoop * (-t[i] / 0.055).exp() / 12.0);
        }
    }
    // 位相を f64 で積む
    let mut ph = vec![0.0f64; n];
    let mut acc = 0.0f64;
    for i in 0..n {
        acc += f[i] as f64 / SR as f64;
        ph[i] = acc;
    }
    let mut o = vec![0.0f32; n];
    for (k0, &a) in w.harm.iter().enumerate() {
        let k = (k0 + 1) as f64;
        if a <= 0.0 || freq * k as f32 >= SR * 0.47 {
            continue;
        }
        for i in 0..n {
            o[i] += (std::f64::consts::TAU * k * ph[i]).sin() as f32 * a;
        }
    }
    let e = env::adsr(n, w.atk * (1.3 - 0.5 * vel), w.dec, w.sus, w.rel);
    let mut out = mul(&o, &e);

    if w.air > 0.0 {
        // 息は音より少し早く立ち上がり、消えるのも少し遅い
        let ae = env::adsr(n, w.atk * 0.45, w.dec * 1.4, w.sus * 1.05, w.rel * 1.6);
        let br = breath(n, freq, seed + 11, 1200.0, 5000.0, 0.9);
        let g = w.air * (0.7 + 0.5 * vel);
        for i in 0..n {
            out[i] += br[i] * g * ae[i];
        }
    }
    if w.chiff > 0.0 {
        let mut ck = noise(n, seed + 23);
        mul_in_place(&mut ck, &env::ad(n, 0.0015, 0.030, 3.0));
        let ck = highpass(&ck, 1500.0f32.max(freq * 2.2));
        add_scaled(&mut out, &ck, w.chiff * 0.35 * vel);
    }
    if w.cut != 0.0 {
        let br = 1.0 + 0.5 * vel;
        let c = (freq * w.cut * br * w.tone).min(SR * 0.44);
        out = lad(cfg, &out, &flat(c), 0.10);
    }
    scale(&mut out, 0.40 * (0.5 + 0.5 * vel));
    out
}

macro_rules! wind_patch {
    ($name:ident, $doc:expr, $cfgexpr:expr) => {
        #[doc = $doc]
        pub fn $name(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
            wind(cfg, &$cfgexpr, freq, n, vel, seed)
        }
    };
}

wind_patch!(flute, "コンサートフルート。素直で、息は控えめ。", WindCfg {
    harm: &[1.0, 0.20, 0.05, 0.015], air: 0.16, chiff: 0.45,
    vib: (5.0, 0.006, 0.35), cut: 7.0, ..Default::default() });

wind_patch!(shakuhachi,
    "尺八。息が主役。閉じた管なので奇数倍音が強い。吹き始めにしゃくり上がる。",
    WindCfg { harm: &[1.0, 0.08, 0.26, 0.04, 0.10], air: 0.46, chiff: 1.1,
    vib: (4.2, 0.013, 0.45), scoop: -0.55, atk: 0.070, cut: 5.0,
    ..Default::default() });

wind_patch!(sinobue, "篠笛。祭囃子の横笛。高く、鋭く、まっすぐ抜ける。", WindCfg {
    harm: &[1.0, 0.34, 0.15, 0.07, 0.03], air: 0.24, chiff: 0.85,
    vib: (6.4, 0.008, 0.26), atk: 0.030, cut: 9.0, ..Default::default() });

wind_patch!(quena, "ケーナ。アンデスの縦笛。息の粗さがそのまま味になる。", WindCfg {
    harm: &[1.0, 0.26, 0.10, 0.03], air: 0.38, chiff: 0.9,
    vib: (5.6, 0.011, 0.30), scoop: -0.25, cut: 6.5, ..Default::default() });

wind_patch!(bansuri, "バーンスリー。インドの横笛。音から音へ滑って移る。", WindCfg {
    harm: &[1.0, 0.16, 0.04], air: 0.26, chiff: 0.35, vib: (4.6, 0.014, 0.40),
    scoop: -1.1, atk: 0.085, cut: 5.5, ..Default::default() });

wind_patch!(ocarina, "オカリナ。倍音がほとんど無い。正弦波に近い数少ない生楽器。",
    WindCfg { harm: &[1.0, 0.05, 0.01], air: 0.12, chiff: 0.30,
    vib: (5.4, 0.006, 0.32), cut: 8.0, ..Default::default() });

wind_patch!(whistle, "ティンホイッスル。アイルランドの笛。頭の「タッ」が命。",
    WindCfg { harm: &[1.0, 0.42, 0.24, 0.11, 0.05], air: 0.20, chiff: 1.4,
    vib: (6.0, 0.006, 0.24), atk: 0.018, cut: 10.0, ..Default::default() });

wind_patch!(panflute, "パンフルート。息が強く、頭で「ホッ」と鳴る。", WindCfg {
    harm: &[1.0, 0.20, 0.06], air: 0.52, chiff: 1.5, vib: (5.2, 0.009, 0.28),
    atk: 0.022, dec: 0.16, sus: 0.72, cut: 6.0, ..Default::default() });

/// ドゥドゥク。二枚リード。決まった周波数に山があることで「しゃべる」。
pub fn duduk(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    let v = vib_curve(n, 4.8, 0.010, 0.30, 0.35, seed);
    let f: Vec<f32> = v
        .iter()
        .enumerate()
        .map(|(i, x)| freq * x * 2.0f32.powf(-0.7 * (-t[i] / 0.05).exp() / 12.0))
        .collect();
    let a = osc::saw_var(&f, rng.next_f64() as f32);
    let b = osc::square_var(&f, rng.next_f64() as f32);
    let src: Vec<f32> = (0..n).map(|i| a[i] * 0.7 + b[i] * 0.3).collect();
    let e = env::adsr(n, 0.055, 0.12, 0.88, 0.12);
    let se = mul(&src, &e);
    let mut body = formants(&se, &[(620.0, 6.0, 1.0), (1180.0, 8.0, 0.55), (2650.0, 10.0, 0.22)]);
    add_scaled(&mut body, &se, 0.22); // 素の音も少し混ぜて芯を残す
    let br = breath(n, freq, seed + 5, 1200.0, 5000.0, 0.9);
    for i in 0..n {
        body[i] += br[i] * 0.10 * e[i];
    }
    let c = (freq * 7.0).min(9000.0);
    let out = lad(cfg, &body, &flat(c), 0.14);
    let mut out = driven(&out, 1.25);
    scale(&mut out, 1.25 * (0.5 + 0.5 * vel));
    out
}

// ================================================================ 異国の弦・打

/// シタール。「ビィーン」は弦ではなく駒（ジャワリ）が作っている。
pub fn sitar(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    let v = vib_curve(n, 5.0, 0.006, 0.30, 0.35, seed);
    let f: Vec<f32> = v.iter().map(|x| freq * x).collect();
    let o = osc::saw_var(&f, rng.next_f64() as f32);
    let body = mul(&o, &env::lead(n, 0.0015, 0.28, 0.30, 0.20));
    let fenv = env::ad(n, 0.001, 0.10, 3.0);
    let cut: Vec<f32> = fenv.iter().map(|x| 900.0 + 7000.0 * x * (0.5 + 0.5 * vel)).collect();
    let body = lad(cfg, &body, &cut, 0.36);
    // ジャワリ。波の頭を折り返して倍音を増やす
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let buzz = (body[i] * (2.6 + 2.0 * vel)).sin() * (-t[i] / 0.9).exp();
            body[i] * 0.55 + buzz * 0.45
        })
        .collect();
    // 共鳴弦。5度と8度とその上を、うんと薄く長く
    let symp = partials(
        freq,
        n,
        &[(1.5, 0.10, 1.8), (2.0, 0.08, 1.6), (3.0, 0.05, 1.4), (1.0, 0.09, 2.2)],
        seed + 7,
        0.004,
    );
    add_scaled(&mut out, &symp, 0.35);
    let mut pick = noise(n, seed);
    mul_in_place(&mut pick, &env::ad(n, 0.0003, 0.005, 6.0));
    add_scaled(&mut out, &highpass(&pick, 2800.0), 0.20);
    scale(&mut out, 0.50 * vel);
    out
}

/// 二胡。弓で擦る2弦。音から音へ滑り、胴（蛇皮）がよく鳴る。
pub fn erhu(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    let v = vib_curve(n, 5.4, 0.016, 0.18, 0.30, seed);
    let f: Vec<f32> = v
        .iter()
        .enumerate()
        .map(|(i, x)| freq * x * 2.0f32.powf(-1.4 * (-t[i] / 0.045).exp() / 12.0))
        .collect();
    let o = osc::saw_var(&f, rng.next_f64() as f32);
    let e = env::adsr(n, 0.075, 0.14, 0.90, 0.13);
    let body = mul(&o, &e);
    let c = (freq * 6.0).min(7200.0);
    let body = lad(cfg, &body, &flat(c), 0.20);
    // 胴の共鳴。ここが無いと「ただのノコギリ波」になる
    let res = formants(&body, &[(340.0, 5.0, 0.9), (1100.0, 7.0, 0.5), (2400.0, 9.0, 0.25)]);
    let mut hair = highpass(&noise(n, seed + 3), 3000.0); // 弓の毛
    mul_in_place(&mut hair, &e);
    scale(&mut hair, 0.045);
    let out: Vec<f32> =
        (0..n).map(|i| body[i] * 0.55 + res[i] * 0.75 + hair[i]).collect();
    let mut out = driven(&out, 1.2);
    scale(&mut out, 0.62 * (0.4 + 0.6 * vel));
    out
}

/// カリンバ（親指ピアノ）。金属の細い板。倍音が整数にならない。
pub fn kalimba(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut out = partials(
        freq,
        n,
        &[(1.0, 1.0, 0.55), (5.4, 0.22, 0.16), (13.1, 0.08, 0.07), (2.0, 0.10, 0.30)],
        seed,
        0.001,
    );
    mul_in_place(&mut out, &env::ad(n, 0.0008, 0.50, 2.2));
    let mut click = noise(n, seed);
    mul_in_place(&mut click, &env::ad(n, 0.0004, 0.008, 5.0));
    add_scaled(&mut out, &highpass(&click, 1800.0), 0.16);
    scale(&mut out, 0.30 * vel);
    out
}

/// ガムラン。2台をわざとずらして調律してある（ombak＝波）。そのうなりが命。
pub fn gamelan(_cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let tab = [
        (1.0f32, 1.0f32, 1.6f32),
        (2.76, 0.42, 0.9),
        (5.40, 0.20, 0.5),
        (8.93, 0.09, 0.28),
        (1.99, 0.16, 1.1),
    ];
    let a = partials(freq, n, &tab, seed, 0.0);
    let b = partials(freq * 1.006, n, &tab, seed, 0.0); // うなり用にずらす
    let e = env::ad(n, 0.0015, 1.5, 1.6);
    let mut out: Vec<f32> = (0..n).map(|i| (a[i] + b[i]) * 0.5 * e[i]).collect();
    let mut hit = noise(n, seed);
    mul_in_place(&mut hit, &env::ad(n, 0.0004, 0.012, 5.0));
    add_scaled(&mut out, &highpass(&hit, 2200.0), 0.18);
    scale(&mut out, 0.21 * vel);
    out
}

/// サントゥール。細い撥で叩く金属弦。1音に3本張ってある。
pub fn santur(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let mut out = vec![0.0f32; n];
    for d in [-0.0035f32, 0.0, 0.0035] {
        // 3本のずれ
        let f = freq * (1.0 + d);
        add_scaled(&mut out, &osc::saw(f, n, rng.next_f64() as f32), 0.42);
    }
    mul_in_place(&mut out, &env::ad(n, 0.0012, 0.42, 2.6));
    let fenv = env::ad(n, 0.0008, 0.09, 3.2);
    let cut: Vec<f32> = fenv.iter().map(|x| 1400.0 + 9000.0 * x * (0.5 + 0.5 * vel)).collect();
    let mut out = lad(cfg, &out, &cut, 0.22);
    let mut hit = noise(n, seed);
    mul_in_place(&mut hit, &env::ad(n, 0.0003, 0.006, 6.0));
    add_scaled(&mut out, &highpass(&hit, 3200.0), 0.22);
    scale(&mut out, 1.05 * vel);
    out
}

/// ウード。中東の撥弦。フレットが無く、ガット弦と大きな胴で低くて丸い。
pub fn oud(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let a = osc::saw(freq, n, rng.next_f64() as f32);
    let b = osc::saw(freq * 1.003, n, rng.next_f64() as f32);
    let o: Vec<f32> = (0..n).map(|i| a[i] * 0.6 + b[i] * 0.4).collect();
    let body = mul(&o, &env::lead(n, 0.0018, 0.22, 0.28, 0.18));
    let fenv = env::ad(n, 0.0012, 0.07, 3.0);
    let cut: Vec<f32> = fenv.iter().map(|x| 500.0 + 3600.0 * x * (0.5 + 0.5 * vel)).collect();
    let body = lad(cfg, &body, &cut, 0.26);
    let res = formants(&body, &[(240.0, 4.0, 0.7), (760.0, 6.0, 0.35)]);
    let mut pick = noise(n, seed);
    mul_in_place(&mut pick, &env::ad(n, 0.0004, 0.007, 5.0));
    let pick = highpass(&pick, 2000.0);
    let g = 1.50 * vel;
    (0..n).map(|i| (body[i] * 0.7 + res[i] * 0.5 + pick[i] * 0.18) * g).collect()
}

/// ディジュリドゥ。音程はほぼ変わらず、共鳴の山だけが動いて「ワウ」になる。
pub fn didge(cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    let t = times(n);
    let f = freq.clamp(40.0, 110.0);
    let sa = osc::saw(f, n, rng.next_f64() as f32);
    let nz = noise(n, seed);
    let e = env::adsr(n, 0.045, 0.15, 0.92, 0.16);
    let src: Vec<f32> = (0..n).map(|i| (sa[i] * 0.8 + nz[i] * 0.10) * e[i]).collect();
    let src = driven(&src, 2.2);
    // 山を動かす。ここが「ワウ」の正体
    let ph = rng.next_f64() as f32 * 6.283;
    let lfo: Vec<f32> = t.iter().map(|x| 700.0 + 450.0 * (TAU * 2.4 * x + ph).sin()).collect();
    let mut out = lad(cfg, &src, &lfo, 0.55);
    let bp = bandpass(&src, 1500.0, 6.0);
    add_scaled(&mut out, &bp, 0.25);
    scale(&mut out, 0.62 * (0.5 + 0.5 * vel));
    out
}

// ================================================================ 効果音

/// ドロップ頭に置く低いインパクト。「来た」を作る。
pub fn fx_impact(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let n = n.min((2.2 * SR) as usize);
    let pitch: Vec<f32> =
        (0..n).map(|i| 38.0 + 95.0 * (-(i as f32 / SR) / 0.045).exp()).collect();
    let mut body = sine_sweep(&pitch, 0.0);
    mul_in_place(&mut body, &env::ad(n, 0.001, 0.45, 2.0));
    let mut air = highpass(&noise(n, seed), 250.0);
    mul_in_place(&mut air, &env::ad(n, 0.001, 0.30, 2.6));
    scale(&mut air, 0.28);
    add_scaled(&mut body, &air, 1.0);
    let mut out = driven(&body, 1.7);
    scale(&mut out, 0.85 * vel);
    out
}

/// ノイズのハイパスを上げていくライザー。
pub fn fx_riser(n: usize, vel: f32, seed: u64) -> Vec<f32> {
    let x = highpass(&noise(n, seed), 400.0);
    let tone = osc::saw(220.0, n, 0.0);
    (0..n)
        .map(|i| {
            let t = if n <= 1 { 0.0 } else { i as f32 / (n - 1) as f32 };
            let a = x[i] * (0.15 + 0.85 * t.powf(2.2));
            (a * 0.30 + tone[i] * 0.15 * t.powi(3)) * vel
        })
        .collect()
}

// ================================================================ 登録

/// 名前から音色を引く。`None` なら知らない名前。
///
/// ここに無ければ [`crate::kit`]（作り方を数で書いた楽器）も見る。
pub fn render(name: &str, cfg: &Cfg, freq: f32, n: usize, vel: f32, seed: u64) -> Option<Vec<f32>> {
    if let Some(r) = crate::kit::get(name) {
        return Some(crate::recipe::render(&r, freq, n, vel, seed));
    }
    render_builtin(name, cfg, freq, n, vel, seed)
}

/// Rust の関数として書いてある46音色。
fn render_builtin(
    name: &str,
    cfg: &Cfg,
    freq: f32,
    n: usize,
    vel: f32,
    seed: u64,
) -> Option<Vec<f32>> {
    let f = match name {
        "supersaw" => supersaw,
        "acid" => acid,
        "stab" => stab,
        "pluck" => pluck,
        "hardlead" => hardlead,
        "wobble" => wobble,
        "brass" => brass,
        "shamisen" => shamisen,
        "harpsi" => harpsi,
        "bell" => bell,
        "organ" => organ,
        "steelpan" => steelpan,
        "marimba" => marimba,
        "choir" => choir,
        "piano" => piano,
        "koto" => koto,
        "brightsaw" => brightsaw,
        "squarelead" => squarelead,
        "crystal" => crystal,
        "strings" => strings,
        "sub" => sub,
        "flute" => flute,
        "shakuhachi" => shakuhachi,
        "sinobue" => sinobue,
        "quena" => quena,
        "bansuri" => bansuri,
        "ocarina" => ocarina,
        "whistle" => whistle,
        "panflute" => panflute,
        "duduk" => duduk,
        "sitar" => sitar,
        "erhu" => erhu,
        "kalimba" => kalimba,
        "gamelan" => gamelan,
        "santur" => santur,
        "oud" => oud,
        "didge" => didge,
        "reese" => reese,
        "fmbass" => fmbass,
        "donk" => donk,
        "808" => tr808,
        "growl" => growl,
        "hardbass" => hardbass,
        "fingerbass" => fingerbass,
        "upright" => upright,
        "rumble" => rumble,
        _ => return None,
    };
    Some(f(cfg, freq, n, vel, seed))
}

/// 使える音色の名前。Python 版の `PATCHES` と同じ 46 本。
/// 使える音色の名前を全部。Rust で書いた46種＋数で書いた楽器。
pub fn all_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = NAMES.to_vec();
    v.extend_from_slice(crate::kit::NAMES);
    v
}

/// Rust の関数として書いてある音色の名前。
pub const NAMES: &[&str] = &[
    "supersaw", "acid", "stab", "pluck", "hardlead", "wobble", "brass", "shamisen", "harpsi",
    "bell", "organ", "steelpan", "marimba", "choir", "piano", "koto", "brightsaw", "squarelead",
    "crystal", "strings", "sub", "flute", "shakuhachi", "sinobue", "quena", "bansuri", "ocarina",
    "whistle", "panflute", "duduk", "sitar", "erhu", "kalimba", "gamelan", "santur", "oud",
    "didge", "reese", "fmbass", "donk", "808", "growl", "hardbass", "fingerbass", "upright",
    "rumble",
];

/// 音符が終わったあとも鳴り続ける長さ（秒）。
///
/// 打弦・撥弦系は鍵盤や指を離しても余韻が残るので、音価ぴったりで切ると
/// 音と音のあいだに穴が空いて「分離して」聞こえる。
pub fn ring(name: &str) -> f32 {
    if let Some(r) = crate::kit::get(name) {
        return r.ring;
    }
    match name {
        "piano" => 0.42,
        "crystal" => 0.30,
        "koto" => 0.18,
        // 息ものは息を止めれば止まる。管の中の空気が落ち着くぶんだけ残す
        "shakuhachi" | "duduk" | "erhu" => 0.10,
        "bansuri" => 0.08,
        "flute" | "panflute" => 0.06,
        // 金属と共鳴弦はよく残る。ガムランはとくに長い
        "gamelan" => 1.60,
        "kalimba" => 0.55,
        "santur" => 0.45,
        "sitar" => 0.60,
        "oud" => 0.25,
        // 808 と指弾きは音符を切っても残る
        "808" => 0.70,
        "fingerbass" => 0.20,
        "upright" => 0.28,
        "rumble" => 0.25,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{peak, rms};

    #[test]
    fn all_46_patches_are_reachable() {
        assert_eq!(NAMES.len(), 46, "音色の数が合わない");
        // 数で書いた楽器も合わせて、全部が名前で引けること
        let all = all_names();
        assert_eq!(all.len(), 46 + crate::kit::NAMES.len());
        for n in &all {
            assert!(
                render(n, &Cfg::default(), 220.0, 4800, 1.0, 1).is_some(),
                "{n} が名前で引けない"
            );
        }
        let cfg = Cfg::default();
        for name in NAMES {
            assert!(render(name, &cfg, 220.0, 100, 1.0, 0).is_some(), "引けない: {name}");
        }
        assert!(render("そんな音色はない", &cfg, 220.0, 100, 1.0, 0).is_none());
    }

    #[test]
    fn every_patch_makes_finite_sound() {
        let cfg = Cfg::default();
        let n = (0.5 * SR) as usize;
        for name in NAMES {
            let y = render(name, &cfg, 220.0, n, 1.0, 0).unwrap();
            assert_eq!(y.len(), n, "{name}: 長さが違う");
            assert!(y.iter().all(|v| v.is_finite()), "{name}: 値が飛んだ");
            assert!(rms(&y) > 1e-5, "{name}: 無音 rms={}", rms(&y));
            assert!(peak(&y) < 12.0, "{name}: 大きすぎる peak={}", peak(&y));
        }
    }

    #[test]
    fn velocity_changes_level_for_every_patch() {
        let cfg = Cfg::default();
        let n = (0.3 * SR) as usize;
        for name in NAMES {
            let soft = rms(&render(name, &cfg, 220.0, n, 0.3, 0).unwrap());
            let loud = rms(&render(name, &cfg, 220.0, n, 1.0, 0).unwrap());
            assert!(loud > soft, "{name}: 強弱が効かない {soft} -> {loud}");
        }
    }

    #[test]
    fn patches_survive_extreme_pitches() {
        let cfg = Cfg::default();
        let n = 2400;
        for name in NAMES {
            for f in [27.5f32, 110.0, 1760.0, 4186.0] {
                let y = render(name, &cfg, f, n, 0.8, 3).unwrap();
                assert!(y.iter().all(|v| v.is_finite()), "{name} @{f}Hz: 値が飛んだ");
            }
        }
    }

    #[test]
    fn same_seed_gives_same_sound() {
        let cfg = Cfg::default();
        let a = render("supersaw", &cfg, 440.0, 4800, 1.0, 5).unwrap();
        let b = render("supersaw", &cfg, 440.0, 4800, 1.0, 5).unwrap();
        assert_eq!(a, b, "同じ種で結果が変わった");
        let c = render("supersaw", &cfg, 440.0, 4800, 1.0, 6).unwrap();
        assert_ne!(a, c, "種を変えても同じ");
    }

    #[test]
    fn short_notes_do_not_panic() {
        let cfg = Cfg::default();
        for name in NAMES {
            for n in [1usize, 2, 16, 480] {
                let y = render(name, &cfg, 220.0, n, 1.0, 0).unwrap();
                assert_eq!(y.len(), n, "{name} n={n}");
            }
        }
    }

    #[test]
    fn ring_is_zero_for_short_patches() {
        assert_eq!(ring("supersaw"), 0.0);
        assert_eq!(ring("gamelan"), 1.60);
        assert_eq!(ring("piano"), 0.42);
    }
}
