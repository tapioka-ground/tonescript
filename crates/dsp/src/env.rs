//! エンベロープ。音の時間ごとの音量。
//!
//! Python 版は `np.linspace` と `np.exp` で区間ごとに配列を作って
//! 貼り合わせていた。ここでは1サンプルずつ書く。
//! 区間の境目の値は Python と同じになるように合わせてある
//! （linspace は両端を含む。step は n-1 で割る）。

use crate::osc::SR;

/// Python の `np.linspace(a, b, n)` と同じ刻み方で書き込む。
/// 両端を含むので、刻み幅は (b - a) / (n - 1)。n == 1 なら a だけ。
#[inline]
fn linspace_into(out: &mut [f32], a: f32, b: f32) {
    let n = out.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        out[0] = a;
        return;
    }
    let step = (b - a) / (n - 1) as f32;
    for (i, v) in out.iter_mut().enumerate() {
        *v = a + step * i as f32;
    }
}

/// アタックしてから指数的に減衰する。打楽器と短い音向け。
pub fn ad(n: usize, attack: f32, decay: f32, curve: f32) -> Vec<f32> {
    let mut e = vec![0.0f32; n];
    if n == 0 {
        return e;
    }
    let a = ((attack * SR) as usize).max(1).min(n);
    linspace_into(&mut e[..a], 0.0, 1.0);
    if n > a {
        let d = decay.max(1e-4);
        for (i, v) in e[a..].iter_mut().enumerate() {
            let t = i as f32 / SR;
            *v = (-curve * t / d).exp();
        }
    }
    e
}

/// アタック → 指数減衰 → サステイン保持 → リリース。
///
/// 減衰時間を固定にすると音符の長さに関係なく切れてしまい、伸ばした音が
/// 作れない。必ずサステインを持たせて音価ぶん鳴らしきる。
pub fn lead(n: usize, a: f32, d: f32, s: f32, r: f32) -> Vec<f32> {
    let mut e = vec![s; n];
    if n == 0 {
        return e;
    }
    let ai = ((a * SR) as usize).min(n);
    if ai > 0 {
        linspace_into(&mut e[..ai], 0.0, 1.0);
    }
    let di = (ai + (d * SR) as usize).min(n);
    if di > ai {
        let len = di - ai;
        for (i, v) in e[ai..di].iter_mut().enumerate() {
            // Python: k = linspace(0, 1, len); s + (1-s) * exp(-4k)
            let k = if len == 1 {
                0.0
            } else {
                i as f32 / (len - 1) as f32
            };
            *v = s + (1.0 - s) * (-4.0 * k).exp();
        }
    }
    // Python: ri = min(int(r*SR), n - ai) —— 上限が n ではなく n - ai
    let ri = ((r * SR) as usize).min(n.saturating_sub(ai));
    if ri > 0 {
        let start = n - ri;
        let mut fade = vec![0.0f32; ri];
        linspace_into(&mut fade, 1.0, 0.0);
        for (v, f) in e[start..].iter_mut().zip(&fade) {
            *v *= f;
        }
    }
    e
}

/// ふつうの ADSR。
pub fn adsr(n: usize, a: f32, d: f32, s: f32, r: f32) -> Vec<f32> {
    let mut e = vec![0.0f32; n];
    if n == 0 {
        return e;
    }
    let (ai, di, ri) = ((a * SR) as usize, (d * SR) as usize, (r * SR) as usize);
    let i = ai.min(n);
    linspace_into(&mut e[..i], 0.0, 1.0);
    if n > i {
        let j = (i + di).min(n);
        linspace_into(&mut e[i..j], 1.0, s);
        if n > j {
            for v in e[j..].iter_mut() {
                *v = s;
            }
        }
    }
    if ri > 0 && n > ri {
        let mut fade = vec![0.0f32; ri];
        linspace_into(&mut fade, 1.0, 0.0);
        for (v, f) in e[n - ri..].iter_mut().zip(&fade) {
            *v *= f;
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ad_rises_then_falls() {
        let e = ad(4800, 0.01, 0.2, 3.0);
        let a = (0.01 * SR) as usize;
        assert!((e[0] - 0.0).abs() < 1e-6);
        assert!((e[a - 1] - 1.0).abs() < 1e-6, "頂点が 1 でない: {}", e[a - 1]);
        // 減衰は単調に下がる
        for i in a..e.len() - 1 {
            assert!(e[i] >= e[i + 1] - 1e-7, "減衰が上がった @{i}");
        }
        assert!(*e.last().unwrap() < 0.6);
    }

    #[test]
    fn lead_holds_sustain() {
        let s = 0.52;
        let e = lead(24_000, 0.004, 0.13, s, 0.09);
        // リリースに入る前は s 以上を保つ
        let r = (0.09 * SR) as usize;
        let mid = e.len() - r - 10;
        assert!(e[mid] >= s - 1e-3, "サステインを割った: {}", e[mid]);
        assert!(*e.last().unwrap() < 1e-3, "最後が 0 に落ちていない");
    }

    #[test]
    fn adsr_shape() {
        let e = adsr(48_000, 0.01, 0.05, 0.4, 0.1);
        let a = (0.01 * SR) as usize;
        let d = (0.05 * SR) as usize;
        assert!((e[a - 1] - 1.0).abs() < 1e-5);
        assert!((e[a + d] - 0.4).abs() < 1e-3, "サステイン値が違う: {}", e[a + d]);
        assert!(*e.last().unwrap() < 1e-3);
    }

    #[test]
    fn short_notes_do_not_panic() {
        // 音価が極端に短いときに区間が潰れる。落ちないこと。
        for n in [0usize, 1, 2, 5, 50] {
            let _ = ad(n, 0.01, 0.2, 3.0);
            let _ = lead(n, 0.004, 0.13, 0.52, 0.09);
            let _ = adsr(n, 0.01, 0.05, 0.4, 0.1);
        }
    }
}
