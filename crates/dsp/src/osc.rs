//! 発振器。
//!
//! Python 版は「帯域制限したノコギリ波を 2048 点のテーブルに焼いて、
//! 最近傍で引く」形だった。1サンプルずつのループが書けないので、
//! テーブル引き＋ファンシーインデックスに逃がすしかなかったため。
//!
//! ここではテーブルを残しつつ、引き方だけ直線補間にする。
//! テーブルは最近傍で引くと 2048 点の段差がそのまま量子化雑音になる。
//! Python でこれをやらなかったのは、補間すると配列が 3 本増えて
//! かえって遅くなるからで、1サンプルずつ回せる今は理由が無い。
//!
//! テーブル自体を捨てて毎サンプル加算合成する手もあるが、
//! 64 倍音だと 1 サンプルに sin が 64 回要る。テーブルのほうが速く、
//! 補間さえすれば折り返しも段差も出ない。

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub const SR: f32 = 48_000.0;
/// 1周期ぶんのテーブルの点数。Python 版と同じ。
pub const TABLE: usize = 2048;

/// 倍音の数ごとのノコギリ波テーブル。作るのは1回だけ。
fn tables() -> &'static RwLock<HashMap<u32, Vec<f32>>> {
    static T: OnceLock<RwLock<HashMap<u32, Vec<f32>>>> = OnceLock::new();
    T.get_or_init(|| RwLock::new(HashMap::new()))
}

/// nh 次までの倍音で作った1周期ぶんのノコギリ波。折り返しを防ぐ。
///
/// 末尾に 1 点だけ余分に持たせて、先頭の値を写しておく。
/// こうすると補間のときに「次の点」が必ず存在して、
/// 1サンプルごとの折り返し判定が要らなくなる。
fn saw_table(nh: u32) -> Vec<f32> {
    if let Some(t) = tables().read().unwrap().get(&nh) {
        return t.clone();
    }
    let mut w = vec![0.0f64; TABLE + 1];
    for k in 1..=nh as usize {
        let kf = k as f64;
        for (i, v) in w.iter_mut().take(TABLE).enumerate() {
            let t = i as f64 / TABLE as f64;
            *v += (2.0 * std::f64::consts::PI * kf * t).sin() / kf;
        }
    }
    let peak = w.iter().take(TABLE).fold(0.0f64, |m, v| m.max(v.abs()));
    let scale = if peak > 0.0 { 1.0 / peak } else { 1.0 };
    let mut out: Vec<f32> = w.iter().map(|v| (v * scale) as f32).collect();
    out[TABLE] = out[0]; // 折り返しの番人
    tables().write().unwrap().insert(nh, out.clone());
    out
}

/// その周波数で折り返さずに出せる倍音の数。
pub fn harmonics(freq: f32) -> u32 {
    let f = freq.max(20.0);
    ((SR * 0.45 / f) as i32).clamp(1, 64) as u32
}

/// テーブルを位相で引く。位相は 0〜1。直線補間。
#[inline(always)]
fn lookup(tbl: &[f32], phase: f32) -> f32 {
    let x = phase * TABLE as f32;
    // **ここで必ず収める。** 呼ぶ側は 0〜1 に収めているつもりでも、
    // f64 で積んだ位相を f32 へ落とすときに 0.99999999... が 1.0 へ
    // 丸まることがある。そうなると i が TABLE になり、i+1 が範囲外を指す。
    // 長く鳴らすほど当たりやすく、短い試験では出ない
    let i = (x as usize).min(TABLE as usize - 1);
    let frac = (x - i as f32).clamp(0.0, 1.0);
    let a = tbl[i];
    let b = tbl[i + 1];
    a + (b - a) * frac
}

/// 位相を 0〜1 に収める。負の位相も受ける。
#[inline(always)]
fn wrap(p: f32) -> f32 {
    let p = p - p.floor();
    // floor の丸めで 1.0 ちょうどが出ることがある。そこだけ落とす。
    if p >= 1.0 {
        0.0
    } else {
        p
    }
}

/// ノコギリ波を n サンプル。
pub fn saw(freq: f32, n: usize, phase0: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; n];
    saw_into(&mut out, freq, phase0);
    out
}

/// 既にある配列へ書き込む版。確保を減らしたいところで使う。
pub fn saw_into(out: &mut [f32], freq: f32, phase0: f32) {
    let tbl = saw_table(harmonics(freq));
    let step = freq / SR;
    let mut ph = wrap(phase0);
    for v in out.iter_mut() {
        *v = lookup(&tbl, ph);
        ph += step;
        if ph >= 1.0 {
            ph -= 1.0;
        }
    }
}

/// 帯域制限ノコギリの差分で作る矩形波。
pub fn square(freq: f32, n: usize, phase0: f32) -> Vec<f32> {
    let tbl = saw_table(harmonics(freq));
    let step = freq / SR;
    let mut a = wrap(phase0);
    let mut b = wrap(phase0 + 0.5);
    let mut out = vec![0.0f32; n];
    for v in out.iter_mut() {
        *v = lookup(&tbl, a) - lookup(&tbl, b);
        a += step;
        if a >= 1.0 {
            a -= 1.0;
        }
        b += step;
        if b >= 1.0 {
            b -= 1.0;
        }
    }
    out
}

/// 幅を選べる矩形波（パルス波）。
///
/// 矩形波は「上と下が半々」だが、幅を変えると倍音の並びが変わって
/// 音色そのものが変わる。**ファミコンの音がまさにこれ**で、12.5% と 25% と
/// 50% を切り替えて使い分けていた。細いほど鼻に掛かった細い音になる。
///
/// `width` は**上に居る割合**。0.125 なら 12.5% の時間だけ上。
///
/// 作り方は矩形波と同じでノコギリ2本の差だが、ずらす量は `1 - width`。
/// ここを `width` にすると上下が入れ替わり、12.5% を頼んで 87.5% が出る。
pub fn pulse(freq: f32, n: usize, phase0: f32, width: f32) -> Vec<f32> {
    let w = 1.0 - width.clamp(0.02, 0.98);
    let tbl = saw_table(harmonics(freq));
    let step = freq / SR;
    let mut a = wrap(phase0);
    let mut b = wrap(phase0 + w);
    let mut out = vec![0.0f32; n];
    for v in out.iter_mut() {
        *v = lookup(&tbl, a) - lookup(&tbl, b);
        a += step;
        if a >= 1.0 {
            a -= 1.0;
        }
        b += step;
        if b >= 1.0 {
            b -= 1.0;
        }
    }
    out
}

/// 周波数が毎サンプル変わるノコギリ波。ビブラート用。
///
/// Python 版は `cumsum` で位相を作っていた。f32 の累積は
/// 長い音で誤差が溜まるので、ここは f64 で積む。
pub fn saw_var(freq: &[f32], phase0: f32) -> Vec<f32> {
    let top = freq.iter().fold(0.0f32, |m, &v| m.max(v));
    let tbl = saw_table(harmonics(top));
    let mut ph = wrap(phase0) as f64;
    let inv = 1.0 / SR as f64;
    let mut out = vec![0.0f32; freq.len()];
    for (v, &f) in out.iter_mut().zip(freq) {
        // Python は cumsum なので「先に足してから引く」。合わせる。
        ph += f as f64 * inv;
        ph -= ph.floor();
        *v = lookup(&tbl, ph as f32);
    }
    out
}

pub fn square_var(freq: &[f32], phase0: f32) -> Vec<f32> {
    pulse_var(freq, phase0, 0.5)
}

/// 幅を選べるパルス波の、音程が動く版。
pub fn pulse_var(freq: &[f32], phase0: f32, width: f32) -> Vec<f32> {
    let w = 1.0 - width.clamp(0.02, 0.98);
    let a = saw_var(freq, phase0);
    let b = saw_var(freq, phase0 + w);
    a.iter().zip(&b).map(|(x, y)| x - y).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 上に居る割合。パルス幅がそのまま出るはず。
    fn duty_of(x: &[f32]) -> f32 {
        x.iter().filter(|v| **v > 0.0).count() as f32 / x.len() as f32
    }

    #[test]
    fn a_phase_that_rounds_to_one_does_not_read_past_the_table() {
        // f64 で積んだ位相を f32 へ落とすと、1.0 ちょうどになることがある。
        // そこで範囲外を読んで落ちていた
        let tbl = saw_table(8);
        let v = lookup(&tbl, 1.0);
        assert!(v.is_finite(), "位相 1.0 で壊れた");
        assert!(lookup(&tbl, 0.9999999).is_finite());
        // 長く鳴らしても落ちないこと
        let f = vec![261.63f32; 48_000 * 2];
        let w = saw_var(&f, 0.0);
        assert!(w.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn a_pulse_has_the_width_it_was_given() {
        for w in [0.125f32, 0.25, 0.5] {
            let x = pulse(220.0, 24_000, 0.0, w);
            let got = duty_of(&x);
            assert!((got - w).abs() < 0.03, "幅 {w} を頼んで {got:.3}");
        }
    }

    #[test]
    fn a_half_pulse_is_the_square_wave() {
        let a = pulse(220.0, 4800, 0.0, 0.5);
        let b = square(220.0, 4800, 0.0);
        assert_eq!(a, b, "幅 0.5 が矩形波と違う");
    }

    #[test]
    fn a_narrow_pulse_is_thinner_but_still_sounds() {
        let wide = pulse(220.0, 24_000, 0.0, 0.5);
        let thin = pulse(220.0, 24_000, 0.0, 0.125);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        assert!(rms(&thin) > 0.05, "細いパルスが無音");
        // 細いほうが小さい（上に居る時間が短いので）
        assert!(rms(&thin) < rms(&wide), "細くしたのに小さくならない");
    }

    #[test]
    fn a_silly_width_is_pulled_back_not_broken() {
        for w in [-1.0f32, 0.0, 1.0, 5.0] {
            let x = pulse(220.0, 4800, 0.0, w);
            assert!(x.iter().all(|v| v.is_finite()), "幅 {w} で壊れた");
        }
    }

    #[test]
    fn the_moving_version_matches_the_still_one() {
        // 音程が動かないなら、動く版と同じものが出ること。
        // ただし動く版は「先に位相を進めてから読む」ので1サンプル先に居る
        //（Python 版の cumsum に合わせてある）
        //
        // 長く回すと少しずつ離れる。止め版は位相を f32 で、動く版は f64 で
        // 積んでいるため（長い音で誤差が溜まらないようにした結果）。
        // 頭のうちは同じ波であることを見る
        let f = vec![220.0f32; 600];
        let a = pulse_var(&f, 0.0, 0.25);
        let b = pulse(220.0, 600, 0.0, 0.25);
        let diff =
            a[..599].iter().zip(&b[1..]).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        assert!(diff < 2e-3, "1サンプルずらしても合わない: {diff}");
    }

    #[test]
    fn harmonics_matches_python() {
        // Python: int(np.clip(SR * 0.45 / max(freq, 20.0), 1, 64))
        assert_eq!(harmonics(20.0), 64); // 1080 -> 64 で頭打ち
        assert_eq!(harmonics(440.0), 49); // 21600/440 = 49.09 -> 49
        assert_eq!(harmonics(1000.0), 21); // 21.6 -> 21
        assert_eq!(harmonics(20000.0), 1); // 1.08 -> 1
        assert_eq!(harmonics(5.0), 64); // 20 未満は 20 として扱う
    }

    #[test]
    fn saw_is_bounded_and_periodic() {
        let y = saw(440.0, 4800, 0.0);
        assert_eq!(y.len(), 4800);
        for v in &y {
            assert!(v.abs() <= 1.001, "はみ出した: {v}");
        }
        // 440Hz なら 48000/440 サンプルで一周する
        let period = (SR / 440.0) as usize;
        let d = (y[100] - y[100 + period]).abs();
        assert!(d < 0.05, "1周期後に戻っていない: {d}");
    }

    #[test]
    fn square_is_saw_difference() {
        let sq = square(220.0, 512, 0.0);
        let a = saw(220.0, 512, 0.0);
        let b = saw(220.0, 512, 0.5);
        for i in 0..512 {
            assert!((sq[i] - (a[i] - b[i])).abs() < 1e-6);
        }
    }

    #[test]
    fn interpolation_beats_nearest() {
        // 直線補間したテーブルは、最近傍より滑らかなはず。
        // 隣り合うサンプルの差の最大値で比べる（段差があると跳ねる）。
        let tbl = saw_table(harmonics(110.0));
        let step = 110.0 / SR;
        let (mut worst_lin, mut worst_near) = (0.0f32, 0.0f32);
        let (mut prev_l, mut prev_n) = (0.0f32, 0.0f32);
        let mut ph = 0.0f32;
        for i in 0..4800 {
            let l = lookup(&tbl, ph);
            let n = tbl[(ph * TABLE as f32) as usize];
            if i > 0 {
                worst_lin = worst_lin.max((l - prev_l).abs());
                worst_near = worst_near.max((n - prev_n).abs());
            }
            prev_l = l;
            prev_n = n;
            ph += step;
            if ph >= 1.0 {
                ph -= 1.0;
            }
        }
        // 折り返し（ノコギリの落ちる所）は両方で跳ねるので、
        // そこを除いた滑らかさを見たい。補間が悪化していないことだけ確かめる。
        assert!(worst_lin <= worst_near + 1e-6);
    }
}
