//! 基本の楽器。
//!
//! ここにあるものは全部 [`crate::recipe`] の書式で書いてある。**Rust の
//! 関数ではない。** つまり曲ファイルへ写せばそのまま直せるし、AI に
//! 「これを元に、もっと暗いギター」と言えば書き換えられる。
//!
//! 内蔵46音色（[`crate::patch`]）は Rust の関数で、細かい細工ができる
//! 代わりに人が触れない。こちらはその逆。
//!
//! どこまで本物に近いか
//! --------------------
//! 正直に書いておく。
//!
//! - **撥いた弦**（ギター・ベース・ハープ）は物理模型なので、かなり近い。
//!   弦の往復をそのまま真似ている
//! - **鍵盤と金属**（ローズ・ビブラフォン・鉄琴）はよく似る。もともと
//!   倍音の並びが単純なので、合成が得意な領域
//! - **管**（トランペット・サックス・クラリネット）は「それらしい」止まり。
//!   本物は息と唇と管の相互作用で鳴るもので、ここでは真似ていない
//! - **弓**（バイオリン・チェロ）は一番遠い。弓と弦がくっついて離れてを
//!   繰り返す現象は、ここでは再現していない
//!
//! 生ドラム・生ピアノ・オーケストラは**そもそも作れない**。録ったものを
//! 使うしかない領域で、音源ライブラリが何十GBある理由でもある。

use crate::recipe::{Attack, Env, Filter, FilterKind, Fm, Osc, Recipe, Vibrato, Wave};
#[cfg(test)]
use crate::recipe;

/// 名前から作り方を引く。
pub fn get(name: &str) -> Option<Recipe> {
    let r = match name {
        // ---- 撥いた弦。物理模型（Karplus-Strong）
        "guitar" => Recipe {
            // スチール弦。硬めに、端寄りを弾く
            osc: vec![Osc { wave: Wave::String, decay: 3.2, bright: 0.62, pick: 0.20, ..o() }],
            env: Env { a: 0.001, d: 3.0, s: 1.0, r: 0.12 },
            // 胴。ギターの箱鳴りは 100Hz 前後と 200Hz 前後に山がある
            body: vec![(100.0, 6.0, 0.45), (205.0, 8.0, 0.25), (430.0, 9.0, 0.12)],
            attack: Attack { amount: 0.06, hp: 3000.0, a: 0.0002, d: 0.004 },
            gain: 0.85,
            ring: 1.6,
            ..d()
        },
        "nylon" => Recipe {
            // ナイロン弦。柔らかく、真ん中寄りを弾く
            osc: vec![Osc { wave: Wave::String, decay: 2.2, bright: 0.34, pick: 0.35, ..o() }],
            env: Env { a: 0.002, d: 2.2, s: 1.0, r: 0.12 },
            body: vec![(96.0, 5.0, 0.5), (190.0, 7.0, 0.22)],
            gain: 0.9,
            ring: 1.2,
            ..d()
        },
        "eguitar" => Recipe {
            // エレキ（クリーン）。胴が無いぶん痩せるので、少し歪ませる
            osc: vec![Osc { wave: Wave::String, decay: 4.0, bright: 0.70, pick: 0.14, ..o() }],
            env: Env { a: 0.001, d: 3.5, s: 1.0, r: 0.10 },
            filter: Filter { kind: FilterKind::Ladder, base: 2600.0, sweep: 0.0, res: 0.12, ..f() },
            // アンプの箱。本物のキャビネットは高い所が早く落ちる
            body: vec![(420.0, 3.0, 0.18), (1400.0, 2.5, 0.10)],
            drive: 1.25,
            gain: 0.8,
            ring: 1.4,
            ..d()
        },
        "distguitar" => Recipe {
            // 歪ませたエレキ。潰れると伸びるので decay も長い
            osc: vec![Osc { wave: Wave::String, decay: 5.0, bright: 0.72, pick: 0.12, ..o() }],
            env: Env { a: 0.001, d: 4.0, s: 1.0, r: 0.10 },
            filter: Filter { kind: FilterKind::Ladder, base: 2200.0, sweep: 0.0, res: 0.2, ..f() },
            body: vec![(400.0, 2.5, 0.22), (1100.0, 2.0, 0.14)],
            drive: 7.0,
            gain: 0.42,
            ring: 1.6,
            ..d()
        },
        "ebass" => Recipe {
            // ピック弾きのエレキベース
            osc: vec![Osc { wave: Wave::String, decay: 2.4, bright: 0.52, pick: 0.12, ..o() }],
            env: Env { a: 0.001, d: 2.2, s: 1.0, r: 0.10 },
            filter: Filter { kind: FilterKind::Ladder, base: 1400.0, sweep: 0.0, res: 0.18, ..f() },
            body: vec![(70.0, 4.0, 0.5), (240.0, 6.0, 0.15)],
            attack: Attack { amount: 0.08, hp: 1800.0, a: 0.0002, d: 0.008 },
            drive: 1.4,
            gain: 0.95,
            ring: 0.9,
            ..d()
        },
        "ukulele" => Recipe {
            osc: vec![Osc { wave: Wave::String, decay: 1.2, bright: 0.40, pick: 0.30, ..o() }],
            env: Env { a: 0.001, d: 1.2, s: 1.0, r: 0.08 },
            // 小さい箱なので山が高い所にある
            body: vec![(240.0, 6.0, 0.45), (520.0, 8.0, 0.2)],
            gain: 0.85,
            ring: 0.7,
            ..d()
        },
        "banjo" => Recipe {
            // 皮を張った胴。硬くて短い
            osc: vec![Osc { wave: Wave::String, decay: 0.9, bright: 0.85, pick: 0.10, ..o() }],
            env: Env { a: 0.0005, d: 0.9, s: 1.0, r: 0.06 },
            body: vec![(300.0, 4.0, 0.35), (900.0, 5.0, 0.2)],
            attack: Attack { amount: 0.12, hp: 3500.0, a: 0.0002, d: 0.003 },
            gain: 0.8,
            ring: 0.5,
            ..d()
        },
        "mandolin" => Recipe {
            // 2本1組の弦。少しずらして重ねると本物の唸りになる
            osc: vec![
                Osc { wave: Wave::String, decay: 1.6, bright: 0.72, pick: 0.16, detune: -4.0, ..o() },
                Osc { wave: Wave::String, decay: 1.6, bright: 0.72, pick: 0.16, detune: 4.0, ..o() },
            ],
            env: Env { a: 0.0008, d: 1.5, s: 1.0, r: 0.08 },
            body: vec![(280.0, 6.0, 0.35), (620.0, 7.0, 0.18)],
            gain: 0.8,
            ring: 0.8,
            ..d()
        },
        "harp" => Recipe {
            osc: vec![Osc { wave: Wave::String, decay: 4.5, bright: 0.45, pick: 0.28, ..o() }],
            env: Env { a: 0.001, d: 4.0, s: 1.0, r: 0.2 },
            body: vec![(160.0, 5.0, 0.3), (380.0, 6.0, 0.15)],
            gain: 0.9,
            ring: 2.2,
            ..d()
        },
        "pizzicato" => Recipe {
            // 弓を使わず指で弾いた弦。短く切る
            osc: vec![Osc { wave: Wave::String, decay: 0.55, bright: 0.45, pick: 0.30, ..o() }],
            env: Env { a: 0.001, d: 0.5, s: 0.0, r: 0.08 },
            body: vec![(300.0, 5.0, 0.3), (700.0, 6.0, 0.15)],
            gain: 1.0,
            ring: 0.35,
            ..d()
        },

        // ---- 鍵盤と金属
        "rhodes" => Recipe {
            // エレピ。金属の棒を叩く音なので、頭だけ金属質にする
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            fm: Fm { ratio: 3.0, index: 5.5, decay: 0.13 },
            env: Env { a: 0.002, d: 1.6, s: 0.22, r: 0.25 },
            gain: 0.85,
            ring: 0.5,
            ..d()
        },
        "clav" => Recipe {
            // クラビネット。弦を叩いて、すぐ止める
            osc: vec![Osc { wave: Wave::String, decay: 0.45, bright: 0.88, pick: 0.08, ..o() }],
            env: Env { a: 0.0005, d: 0.4, s: 0.0, r: 0.04 },
            filter: Filter { kind: FilterKind::Ladder, base: 900.0, sweep: 4500.0, res: 0.3,
                             env: (0.0005, 0.06, 3.0), ..f() },
            gain: 0.9,
            ring: 0.15,
            ..d()
        },
        "vibraphone" => Recipe {
            // 金属の板。ゆっくり揺れるのは下の管が回っているから。
            // 少しずらした2本の唸りで、その揺れを作る
            osc: vec![
                Osc { wave: Wave::Sine, mix: 0.5, detune: -3.0, ..o() },
                Osc { wave: Wave::Sine, mix: 0.5, detune: 3.0, ..o() },
            ],
            partials: vec![(4.0, 0.18, 1.6), (9.2, 0.08, 0.9)],
            env: Env { a: 0.002, d: 2.6, s: 0.0, r: 0.5 },
            gain: 1.0,
            ring: 1.5,
            ..d()
        },
        "glocken" => Recipe {
            // 鉄琴。高くて硬い
            osc: vec![],
            partials: vec![(1.0, 0.5, 1.2), (2.7, 0.3, 0.7), (5.4, 0.18, 0.4), (8.9, 0.08, 0.25)],
            env: Env { a: 0.0008, d: 1.2, s: 0.0, r: 0.3 },
            gain: 1.1,
            ring: 0.9,
            ..d()
        },

        // ---- 管。息と唇の相互作用は真似ていないので「それらしい」止まり
        "trumpet" => Recipe {
            osc: vec![Osc { wave: Wave::Saw, ..o() }],
            env: Env { a: 0.035, d: 0.12, s: 0.85, r: 0.09 },
            // ベルが開くように、強く吹くほど上が伸びる
            filter: Filter { kind: FilterKind::Ladder, base: 900.0, sweep: 4200.0, res: 0.22,
                             vel: 2600.0, env: (0.03, 0.2, 1.6), ..f() },
            body: vec![(1200.0, 3.0, 0.2)],
            vibrato: Vibrato { rate: 5.5, depth: 0.004, delay: 0.35 },
            drive: 1.7,
            gain: 0.55,
            ..d()
        },
        "sax" => Recipe {
            osc: vec![Osc { wave: Wave::Saw, mix: 0.8, ..o() },
                      Osc { wave: Wave::Square, mix: 0.2, ..o() }],
            env: Env { a: 0.028, d: 0.14, s: 0.82, r: 0.10 },
            filter: Filter { kind: FilterKind::Ladder, base: 700.0, sweep: 2800.0, res: 0.25,
                             vel: 1800.0, env: (0.025, 0.25, 1.4), ..f() },
            // サックスらしさは胴の山（フォルマント）に出る
            body: vec![(560.0, 5.0, 0.3), (1300.0, 4.0, 0.2), (2400.0, 3.0, 0.1)],
            attack: Attack { amount: 0.05, hp: 2500.0, a: 0.004, d: 0.03 },
            vibrato: Vibrato { rate: 5.0, depth: 0.005, delay: 0.3 },
            drive: 1.4,
            gain: 0.6,
            ..d()
        },
        "clarinet" => Recipe {
            // 片側が閉じた管は、奇数倍音しか出ない。四角波がまさにそれ
            osc: vec![Osc { wave: Wave::Square, ..o() }],
            env: Env { a: 0.03, d: 0.1, s: 0.88, r: 0.09 },
            filter: Filter { kind: FilterKind::Ladder, base: 1500.0, sweep: 900.0, res: 0.15,
                             env: (0.03, 0.2, 1.2), ..f() },
            body: vec![(1500.0, 4.0, 0.15)],
            vibrato: Vibrato { rate: 4.6, depth: 0.003, delay: 0.4 },
            gain: 0.5,
            ..d()
        },
        "oboe" => Recipe {
            // 細い管。倍音が多く、鼻に掛かる
            osc: vec![Osc { wave: Wave::Saw, ..o() }],
            env: Env { a: 0.025, d: 0.1, s: 0.86, r: 0.08 },
            filter: Filter { kind: FilterKind::Ladder, base: 1100.0, sweep: 1500.0, res: 0.3,
                             env: (0.02, 0.18, 1.3), ..f() },
            body: vec![(1400.0, 8.0, 0.4), (3000.0, 6.0, 0.2)],
            vibrato: Vibrato { rate: 5.4, depth: 0.005, delay: 0.25 },
            gain: 0.45,
            ..d()
        },
        "horn" => Recipe {
            // 丸い管。上が少ないので柔らかい
            osc: vec![Osc { wave: Wave::Saw, ..o() }],
            env: Env { a: 0.07, d: 0.18, s: 0.85, r: 0.18 },
            filter: Filter { kind: FilterKind::Ladder, base: 500.0, sweep: 1600.0, res: 0.18,
                             vel: 900.0, env: (0.06, 0.3, 1.3), ..f() },
            body: vec![(480.0, 4.0, 0.25)],
            gain: 0.6,
            ..d()
        },

        // ---- 弓。ここが一番遠い
        "violin" => Recipe {
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -3.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 3.0, ..o() }],
            env: Env { a: 0.06, d: 0.15, s: 0.88, r: 0.18 },
            filter: Filter { kind: FilterKind::Ladder, base: 1100.0, sweep: 2600.0, res: 0.2,
                             track: 0.5, env: (0.05, 0.3, 1.2), ..f() },
            // 胴の山。バイオリンらしさはほぼここ
            body: vec![(280.0, 6.0, 0.35), (460.0, 7.0, 0.25), (700.0, 6.0, 0.18)],
            vibrato: Vibrato { rate: 5.8, depth: 0.007, delay: 0.25 },
            gain: 0.5,
            ..d()
        },
        "cello" => Recipe {
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -2.5, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 2.5, ..o() }],
            env: Env { a: 0.07, d: 0.18, s: 0.88, r: 0.22 },
            filter: Filter { kind: FilterKind::Ladder, base: 600.0, sweep: 1800.0, res: 0.2,
                             track: 0.5, env: (0.06, 0.35, 1.2), ..f() },
            body: vec![(100.0, 5.0, 0.4), (200.0, 6.0, 0.25), (400.0, 6.0, 0.15)],
            vibrato: Vibrato { rate: 5.0, depth: 0.006, delay: 0.3 },
            gain: 0.55,
            ..d()
        },

        // ---- その他
        "accordion" => Recipe {
            // 金属の舌が震える。2枚を少しずらすのが本物の唸り
            osc: vec![
                Osc { wave: Wave::Square, mix: 0.6, detune: -7.0, ..o() },
                Osc { wave: Wave::Saw, mix: 0.4, detune: 7.0, ..o() },
                Osc { wave: Wave::Square, mix: 0.3, octave: 1, ..o() },
            ],
            env: Env { a: 0.02, d: 0.06, s: 0.92, r: 0.07 },
            filter: Filter { kind: FilterKind::Ladder, base: 2200.0, sweep: 0.0, res: 0.1, ..f() },
            body: vec![(700.0, 4.0, 0.2)],
            gain: 0.42,
            ..d()
        },
        "harmonica" => Recipe {
            osc: vec![
                Osc { wave: Wave::Square, mix: 0.7, ..o() },
                Osc { wave: Wave::Saw, mix: 0.3, detune: 6.0, ..o() },
            ],
            env: Env { a: 0.012, d: 0.08, s: 0.85, r: 0.08 },
            filter: Filter { kind: FilterKind::Ladder, base: 1200.0, sweep: 1800.0, res: 0.28,
                             vel: 1200.0, env: (0.01, 0.15, 1.5), ..f() },
            body: vec![(900.0, 5.0, 0.25), (2100.0, 4.0, 0.15)],
            attack: Attack { amount: 0.07, hp: 2500.0, a: 0.003, d: 0.02 },
            vibrato: Vibrato { rate: 6.0, depth: 0.006, delay: 0.2 },
            drive: 1.5,
            gain: 0.5,
            ..d()
        },
        _ => return None,
    };
    Some(r)
}

/// ここで足した楽器の名前。
pub const NAMES: &[&str] = &[
    "guitar",
    "nylon",
    "eguitar",
    "distguitar",
    "ebass",
    "ukulele",
    "banjo",
    "mandolin",
    "harp",
    "pizzicato",
    "rhodes",
    "clav",
    "vibraphone",
    "glocken",
    "trumpet",
    "sax",
    "clarinet",
    "oboe",
    "horn",
    "violin",
    "cello",
    "accordion",
    "harmonica",
];

// 書き並べるときの省略。`..o()` で発振器の既定、`..d()` で全体の既定
fn o() -> Osc {
    Osc::default()
}
fn d() -> Recipe {
    Recipe::default()
}
fn f() -> Filter {
    Filter::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn every_name_has_a_recipe() {
        for n in NAMES {
            assert!(get(n).is_some(), "{n} が引けない");
        }
        assert!(get("nosuch").is_none());
    }

    #[test]
    fn every_instrument_passes_its_own_checks() {
        // 自分で配った音色が、自分の検算に落ちるようでは話にならない
        for n in NAMES {
            let r = get(n).unwrap();
            assert!(recipe::check(&r).is_ok(), "{n}: {:?}", recipe::check(&r));
        }
    }

    #[test]
    fn every_instrument_actually_sounds() {
        for n in NAMES {
            let r = get(n).unwrap();
            // 低い音・真ん中・高い音で見る。どれかで黙るのが一番多い失敗
            for hz in [82.41f32, 261.63, 880.0] {
                let w = recipe::render(&r, hz, 48_000, 1.0, 7);
                let v = rms(&w);
                assert!(v > 0.002, "{n} が {hz}Hz で無音（実効 {v:.5}）");
                assert!(w.iter().all(|x| x.is_finite()), "{n} に数でない値");
                let peak = w.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                assert!(peak < 8.0, "{n} が {hz}Hz で大きすぎる（ピーク {peak:.2}）");
            }
        }
    }

    #[test]
    fn the_levels_are_in_the_same_ballpark() {
        // 持ち替えたら音量が10倍違う、では使えない
        let mut loud: Vec<(f32, &str)> = NAMES
            .iter()
            .map(|n| {
                let w = recipe::render(&get(n).unwrap(), 261.63, 48_000, 1.0, 7);
                (rms(&w), *n)
            })
            .collect();
        loud.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let (min, max) = (loud[0], loud[loud.len() - 1]);
        assert!(
            max.0 / min.0 < 14.0,
            "音量の差が大きすぎる: {} {:.4} 〜 {} {:.4}",
            min.1,
            min.0,
            max.1,
            max.0
        );
    }

    #[test]
    fn the_plucked_ones_die_away() {
        // 撥いた弦が伸びっぱなしなら、物理模型が効いていない
        for n in ["guitar", "nylon", "ukulele", "banjo", "harp", "pizzicato"] {
            let w = recipe::render(&get(n).unwrap(), 196.0, 48_000 * 3, 1.0, 7);
            let head = rms(&w[..4800]);
            let late = rms(&w[48_000 * 2..48_000 * 2 + 4800]);
            assert!(late < head * 0.8, "{n} が減衰していない（{head:.4} → {late:.4}）");
        }
    }

    #[test]
    fn the_blown_ones_hold() {
        // 管と弓は、伸ばしているあいだ鳴り続けること
        for n in ["trumpet", "sax", "clarinet", "violin", "cello", "accordion"] {
            let w = recipe::render(&get(n).unwrap(), 261.63, 48_000, 1.0, 7);
            let head = rms(&w[4800..9600]);
            let late = rms(&w[38_400..43_200]);
            assert!(late > head * 0.5, "{n} が途中で消える（{head:.4} → {late:.4}）");
        }
    }

    #[test]
    fn the_names_do_not_collide_with_the_built_ins() {
        for n in NAMES {
            assert!(
                !crate::patch::NAMES.contains(n),
                "{n} が内蔵と同じ名前（どちらが鳴るか分からなくなる）"
            );
        }
    }
}
