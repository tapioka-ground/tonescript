//! Tonescript の音を作る所。
//!
//! Python 版（cli/synth.py）からの移植。移植の方針は1つだけ。
//!
//! **Python の速度を回避するための細工は、移し替えずに捨てる。**
//!
//! 元のコードには「1サンプルずつ回せないから」生まれた構造が多くある。
//! 波形テーブルの最近傍引き、1極フィルタの FFT 畳み込み、numba への依存。
//! どれも Python では正しい判断だが、ここでは素直なループのほうが速く、
//! しかも音が良くなる（段差も打ち切り誤差も出ない）。
//!
//! 逆に、音そのものを決めている数字と理屈はそのまま持ってくる。
//! なぜその値なのかは Python 側のコメントに残っているので、
//! 変えるときは必ず測ってから変える。

pub mod drum;
pub mod env;
pub mod filter;
pub mod osc;
pub mod patch;
pub mod recipe;
pub mod reverb;
pub mod rng;
pub mod shape;

pub use osc::{SR, TABLE};
pub use rng::{noise, Pcg64};

/// 音量の倍率をデシベルへ。
pub fn to_db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// デシベルを音量の倍率へ。
pub fn from_db(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// 実効値（RMS）。
pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// いちばん大きいところ（絶対値）。
pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trip() {
        for v in [1.0f32, 0.5, 0.1, 0.001] {
            assert!((from_db(to_db(v)) - v).abs() < 1e-5, "{v}");
        }
        assert!((to_db(1.0) - 0.0).abs() < 1e-6);
        assert!((to_db(0.5) + 6.0206).abs() < 1e-3);
    }

    #[test]
    fn rms_and_peak() {
        let x = [1.0f32, -1.0, 1.0, -1.0];
        assert!((rms(&x) - 1.0).abs() < 1e-6);
        assert!((peak(&x) - 1.0).abs() < 1e-6);
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(peak(&[]), 0.0);
    }
}
