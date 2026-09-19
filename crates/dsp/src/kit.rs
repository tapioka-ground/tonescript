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

use crate::recipe::{
    Attack, Env, Filter, FilterKind, Fm, Osc, Recipe, Tremolo, Vibrato, Wave,
};
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
            gain: 0.648,
            ring: 1.6,
            ..d()
        },
        "nylon" => Recipe {
            // ナイロン弦。柔らかく、真ん中寄りを弾く
            osc: vec![Osc { wave: Wave::String, decay: 2.2, bright: 0.34, pick: 0.35, ..o() }],
            env: Env { a: 0.002, d: 2.2, s: 1.0, r: 0.12 },
            body: vec![(96.0, 5.0, 0.5), (190.0, 7.0, 0.22)],
            gain: 0.811,
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
            gain: 1.122,
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
            gain: 0.334,
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
            gain: 1.536,
            ring: 0.9,
            ..d()
        },
        "ukulele" => Recipe {
            osc: vec![Osc { wave: Wave::String, decay: 1.2, bright: 0.40, pick: 0.30, ..o() }],
            env: Env { a: 0.001, d: 1.2, s: 1.0, r: 0.08 },
            // 小さい箱なので山が高い所にある
            body: vec![(240.0, 6.0, 0.45), (520.0, 8.0, 0.2)],
            gain: 0.732,
            ring: 0.7,
            ..d()
        },
        "banjo" => Recipe {
            // 皮を張った胴。硬くて短い
            osc: vec![Osc { wave: Wave::String, decay: 0.9, bright: 0.85, pick: 0.10, ..o() }],
            env: Env { a: 0.0005, d: 0.9, s: 1.0, r: 0.06 },
            body: vec![(300.0, 4.0, 0.35), (900.0, 5.0, 0.2)],
            attack: Attack { amount: 0.12, hp: 3500.0, a: 0.0002, d: 0.003 },
            gain: 0.536,
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
            gain: 0.687,
            ring: 0.8,
            ..d()
        },
        "harp" => Recipe {
            osc: vec![Osc { wave: Wave::String, decay: 4.5, bright: 0.45, pick: 0.28, ..o() }],
            env: Env { a: 0.001, d: 4.0, s: 1.0, r: 0.2 },
            body: vec![(160.0, 5.0, 0.3), (380.0, 6.0, 0.15)],
            gain: 0.708,
            ring: 2.2,
            ..d()
        },
        "pizzicato" => Recipe {
            // 弓を使わず指で弾いた弦。短く切る
            osc: vec![Osc { wave: Wave::String, decay: 0.55, bright: 0.45, pick: 0.30, ..o() }],
            env: Env { a: 0.001, d: 0.5, s: 0.0, r: 0.08 },
            body: vec![(300.0, 5.0, 0.3), (700.0, 6.0, 0.15)],
            gain: 0.732,
            ring: 0.35,
            ..d()
        },


        // ---- 管の残り
        "trombone" => Recipe {
            // トロンボーン。管が太く長いので、ホルンより暗くて重い
            osc: vec![Osc { wave: Wave::Saw, ..o() }],
            env: Env { a: 0.05, d: 0.15, s: 0.86, r: 0.12 },
            filter: Filter { kind: FilterKind::Ladder, base: 420.0, sweep: 2600.0, res: 0.2,
                             vel: 1600.0, env: (0.045, 0.25, 1.5), ..f() },
            body: vec![(520.0, 3.5, 0.22)],
            vibrato: Vibrato { rate: 5.0, depth: 0.003, delay: 0.4 },
            drive: 1.6,
            gain: 0.348,
            ..d()
        },
        "tuba" => Recipe {
            // チューバ。いちばん下を支える
            osc: vec![Osc { wave: Wave::Saw, ..o() }],
            env: Env { a: 0.06, d: 0.2, s: 0.88, r: 0.16 },
            filter: Filter { kind: FilterKind::Ladder, base: 260.0, sweep: 900.0, res: 0.16,
                             vel: 600.0, env: (0.055, 0.3, 1.4), ..f() },
            body: vec![(160.0, 4.0, 0.35)],
            drive: 1.4,
            gain: 0.367,
            ..d()
        },
        "mutetrumpet" => Recipe {
            // ミュートを差したトランペット。鼻に掛かって細くなる
            osc: vec![Osc { wave: Wave::Saw, ..o() }],
            env: Env { a: 0.025, d: 0.1, s: 0.82, r: 0.08 },
            filter: Filter { kind: FilterKind::Bandpass, base: 1600.0, res: 0.5, ..f() },
            body: vec![(1200.0, 9.0, 0.5), (2600.0, 7.0, 0.3)],
            vibrato: Vibrato { rate: 5.6, depth: 0.004, delay: 0.3 },
            drive: 2.0,
            gain: 0.866,
            ..d()
        },
        "bassoon" => Recipe {
            // ファゴット。低い木管。倍音が独特で、低いのに埋もれない
            osc: vec![Osc { wave: Wave::Saw, mix: 0.7, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.3, width: 0.3, ..o() }],
            env: Env { a: 0.03, d: 0.12, s: 0.85, r: 0.1 },
            filter: Filter { kind: FilterKind::Ladder, base: 500.0, sweep: 1200.0, res: 0.28,
                             env: (0.025, 0.2, 1.4), ..f() },
            body: vec![(440.0, 7.0, 0.35), (1200.0, 5.0, 0.2)],
            vibrato: Vibrato { rate: 4.8, depth: 0.004, delay: 0.35 },
            gain: 1.311,
            ..d()
        },
        "piccolo" => Recipe {
            // ピッコロ。フルートの1オクターブ上。息の音が目立つ
            osc: vec![Osc { wave: Wave::Sine, mix: 0.7, ..o() },
                      Osc { wave: Wave::Noise, mix: 0.3, ..o() }],
            env: Env { a: 0.02, d: 0.08, s: 0.88, r: 0.07 },
            filter: Filter { kind: FilterKind::Bandpass, base: 2600.0, res: 0.4, track: 0.8, ..f() },
            vibrato: Vibrato { rate: 5.8, depth: 0.006, delay: 0.2 },
            gain: 4.158,
            ..d()
        },
        "recorder" => Recipe {
            // リコーダー。息が素直に管へ入るので、倍音が少なくて澄む
            osc: vec![Osc { wave: Wave::Sine, mix: 0.85, ..o() },
                      Osc { wave: Wave::Noise, mix: 0.15, ..o() }],
            env: Env { a: 0.025, d: 0.06, s: 0.9, r: 0.06 },
            filter: Filter { kind: FilterKind::Bandpass, base: 1400.0, res: 0.3, track: 0.7, ..f() },
            gain: 3.217,
            ..d()
        },
        "melodica" => Recipe {
            // 鍵盤ハーモニカ。金属の舌なので、アコーディオンに近い
            osc: vec![Osc { wave: Wave::Pulse, mix: 0.7, width: 0.35, ..o() },
                      Osc { wave: Wave::Saw, mix: 0.3, detune: 5.0, ..o() }],
            env: Env { a: 0.015, d: 0.07, s: 0.88, r: 0.07 },
            filter: Filter { kind: FilterKind::Ladder, base: 1600.0, sweep: 900.0, res: 0.2,
                             env: (0.012, 0.12, 1.4), ..f() },
            body: vec![(1000.0, 4.0, 0.2)],
            gain: 0.459,
            ..d()
        },
        "bagpipe" => Recipe {
            // バグパイプ。鳴りっぱなしで、倍音がぎっしり
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -8.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 8.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.5, width: 0.25, octave: -1, ..o() }],
            env: Env { a: 0.04, d: 0.05, s: 0.95, r: 0.06 },
            filter: Filter { kind: FilterKind::Ladder, base: 2400.0, sweep: 0.0, res: 0.25, ..f() },
            body: vec![(900.0, 6.0, 0.3), (2000.0, 5.0, 0.2)],
            drive: 1.6,
            gain: 0.527,
            ..d()
        },

        // ---- 弦の残り
        "viola" => Recipe {
            // ビオラ。バイオリンより5度低く、胴が相対的に小さいので鼻に掛かる
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -3.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 3.0, ..o() }],
            env: Env { a: 0.065, d: 0.16, s: 0.88, r: 0.2 },
            filter: Filter { kind: FilterKind::Ladder, base: 850.0, sweep: 2200.0, res: 0.2,
                             track: 0.5, env: (0.055, 0.32, 1.2), ..f() },
            body: vec![(220.0, 6.0, 0.35), (350.0, 7.0, 0.25), (600.0, 6.0, 0.15)],
            vibrato: Vibrato { rate: 5.4, depth: 0.007, delay: 0.28 },
            gain: 0.793,
            ..d()
        },
        "contrabass" => Recipe {
            // コントラバス（弓）。オーケストラのいちばん下
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -2.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 2.0, ..o() }],
            env: Env { a: 0.08, d: 0.2, s: 0.88, r: 0.25 },
            filter: Filter { kind: FilterKind::Ladder, base: 380.0, sweep: 1100.0, res: 0.18,
                             track: 0.4, env: (0.07, 0.4, 1.2), ..f() },
            body: vec![(60.0, 5.0, 0.45), (130.0, 6.0, 0.25)],
            vibrato: Vibrato { rate: 4.4, depth: 0.005, delay: 0.35 },
            gain: 1.209,
            ..d()
        },
        "tremolostrings" => Recipe {
            // 弓を細かく往復させる奏法。緊張した場面の定番
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -4.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 4.0, ..o() }],
            env: Env { a: 0.04, d: 0.12, s: 0.9, r: 0.2 },
            filter: Filter { kind: FilterKind::Ladder, base: 900.0, sweep: 2200.0, res: 0.2,
                             track: 0.5, env: (0.03, 0.3, 1.2), ..f() },
            body: vec![(280.0, 6.0, 0.3), (460.0, 7.0, 0.2)],
            // 弓の往復そのもの。毎秒14回くらい
            tremolo: Tremolo { rate: 14.0, depth: 0.55 },
            gain: 0.923,
            ..d()
        },
        "slapbass" => Recipe {
            // 弦を叩いて指板に当てる。頭がバチッと鳴る
            osc: vec![Osc { wave: Wave::String, decay: 1.4, bright: 0.85, pick: 0.06, ..o() }],
            env: Env { a: 0.0005, d: 1.2, s: 1.0, r: 0.08 },
            filter: Filter { kind: FilterKind::Ladder, base: 500.0, sweep: 6000.0, res: 0.45,
                             vel: 2500.0, env: (0.0005, 0.05, 3.5), ..f() },
            body: vec![(80.0, 4.0, 0.45), (900.0, 5.0, 0.2)],
            attack: Attack { amount: 0.25, hp: 2500.0, a: 0.0002, d: 0.005 },
            drive: 2.2,
            gain: 0.776,
            ring: 0.6,
            ..d()
        },
        "fretless" => Recipe {
            // フレットレスベース。フレットが無いぶん丸く、唸りが出る
            osc: vec![Osc { wave: Wave::String, decay: 3.0, bright: 0.38, pick: 0.30, ..o() }],
            env: Env { a: 0.004, d: 2.6, s: 1.0, r: 0.14 },
            filter: Filter { kind: FilterKind::Ladder, base: 900.0, sweep: 0.0, res: 0.2, ..f() },
            body: vec![(75.0, 4.0, 0.5), (260.0, 5.0, 0.18)],
            vibrato: Vibrato { rate: 4.5, depth: 0.004, delay: 0.25 },
            gain: 1.796,
            ring: 1.0,
            ..d()
        },
        "twelvestring" => Recipe {
            // 12弦ギター。6組が1オクターブずれて張ってある
            osc: vec![
                Osc { wave: Wave::String, decay: 3.0, bright: 0.60, pick: 0.20, detune: -5.0, ..o() },
                Osc { wave: Wave::String, decay: 3.0, bright: 0.60, pick: 0.20, detune: 5.0, ..o() },
                Osc { wave: Wave::String, mix: 0.55, decay: 2.2, bright: 0.72, pick: 0.18,
                      octave: 1, ..o() },
            ],
            env: Env { a: 0.001, d: 3.0, s: 1.0, r: 0.12 },
            body: vec![(100.0, 6.0, 0.45), (205.0, 8.0, 0.25)],
            attack: Attack { amount: 0.07, hp: 3000.0, a: 0.0002, d: 0.004 },
            gain: 0.871,
            ring: 1.6,
            ..d()
        },
        "balalaika" => Recipe {
            // 三角の胴。短くて硬い
            osc: vec![Osc { wave: Wave::String, decay: 0.8, bright: 0.78, pick: 0.14, ..o() }],
            env: Env { a: 0.0008, d: 0.8, s: 1.0, r: 0.06 },
            body: vec![(330.0, 7.0, 0.4), (780.0, 8.0, 0.2)],
            gain: 0.593,
            ring: 0.45,
            ..d()
        },
        "bouzouki" => Recipe {
            // ブズーキ。2本1組で、金属的に伸びる
            osc: vec![
                Osc { wave: Wave::String, decay: 2.2, bright: 0.80, pick: 0.15, detune: -5.0, ..o() },
                Osc { wave: Wave::String, decay: 2.2, bright: 0.80, pick: 0.15, detune: 5.0, ..o() },
            ],
            env: Env { a: 0.0008, d: 2.0, s: 1.0, r: 0.08 },
            body: vec![(200.0, 6.0, 0.35), (520.0, 7.0, 0.2)],
            gain: 0.66,
            ring: 1.0,
            ..d()
        },
        "guzheng" => Recipe {
            // 古筝。琴より大きく、伸びる
            osc: vec![Osc { wave: Wave::String, decay: 3.8, bright: 0.55, pick: 0.22, ..o() }],
            env: Env { a: 0.001, d: 3.4, s: 1.0, r: 0.2 },
            body: vec![(180.0, 5.0, 0.35), (420.0, 6.0, 0.18)],
            vibrato: Vibrato { rate: 5.5, depth: 0.010, delay: 0.35 },
            gain: 0.713,
            ring: 1.8,
            ..d()
        },
        "pipa" => Recipe {
            // 琵琶。爪で弾くので頭が鋭い
            osc: vec![Osc { wave: Wave::String, decay: 1.6, bright: 0.80, pick: 0.10, ..o() }],
            env: Env { a: 0.0006, d: 1.5, s: 1.0, r: 0.07 },
            body: vec![(260.0, 7.0, 0.35), (700.0, 8.0, 0.2)],
            attack: Attack { amount: 0.14, hp: 3000.0, a: 0.0002, d: 0.004 },
            gain: 0.536,
            ring: 0.8,
            ..d()
        },
        "dulcimer" => Recipe {
            // ダルシマー。ハンマーで叩く弦
            osc: vec![
                Osc { wave: Wave::String, decay: 2.6, bright: 0.68, pick: 0.12, detune: -4.0, ..o() },
                Osc { wave: Wave::String, decay: 2.6, bright: 0.68, pick: 0.12, detune: 4.0, ..o() },
            ],
            env: Env { a: 0.0006, d: 2.4, s: 1.0, r: 0.12 },
            body: vec![(220.0, 6.0, 0.35), (560.0, 7.0, 0.2)],
            attack: Attack { amount: 0.10, hp: 2800.0, a: 0.0002, d: 0.005 },
            gain: 0.754,
            ring: 1.3,
            ..d()
        },

        // ---- 鍵盤と金属
        "rhodes" => Recipe {
            // エレピ。金属の棒を叩く音なので、頭だけ金属質にする
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            fm: Fm { ratio: 3.0, index: 5.5, decay: 0.13 },
            env: Env { a: 0.002, d: 1.6, s: 0.22, r: 0.25 },
            gain: 0.383,
            ring: 0.5,
            ..d()
        },
        "clav" => Recipe {
            // クラビネット。弦を叩いて、すぐ止める
            osc: vec![Osc { wave: Wave::String, decay: 0.45, bright: 0.88, pick: 0.08, ..o() }],
            env: Env { a: 0.0005, d: 0.4, s: 0.0, r: 0.04 },
            filter: Filter { kind: FilterKind::Ladder, base: 900.0, sweep: 4500.0, res: 0.3,
                             env: (0.0005, 0.06, 3.0), ..f() },
            gain: 1.443,
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
            gain: 0.355,
            ring: 1.5,
            ..d()
        },
        "glocken" => Recipe {
            // 鉄琴。高くて硬い
            osc: vec![],
            partials: vec![(1.0, 0.5, 1.2), (2.7, 0.3, 0.7), (5.4, 0.18, 0.4), (8.9, 0.08, 0.25)],
            env: Env { a: 0.0008, d: 1.2, s: 0.0, r: 0.3 },
            gain: 0.629,
            ring: 0.9,
            ..d()
        },




        // ---- 和楽器。撥弦は物理模型が効く
        "kotostring" => Recipe {
            // 琴。柱で分けた絹弦。撥で弾く
            osc: vec![Osc { wave: Wave::String, decay: 3.0, bright: 0.52, pick: 0.18, ..o() }],
            env: Env { a: 0.001, d: 2.8, s: 1.0, r: 0.18 },
            body: vec![(190.0, 5.0, 0.4), (450.0, 6.0, 0.18)],
            attack: Attack { amount: 0.10, hp: 2800.0, a: 0.0002, d: 0.005 },
            vibrato: Vibrato { rate: 5.0, depth: 0.008, delay: 0.4 },
            gain: 0.681,
            ring: 1.5,
            ..d()
        },
        "shamisenstring" => Recipe {
            // 三味線。皮を張った胴。撥が当たる音が要る
            osc: vec![Osc { wave: Wave::String, decay: 1.1, bright: 0.82, pick: 0.09, ..o() }],
            env: Env { a: 0.0006, d: 1.0, s: 1.0, r: 0.08 },
            body: vec![(320.0, 4.0, 0.42), (900.0, 5.0, 0.22)],
            attack: Attack { amount: 0.22, hp: 2600.0, a: 0.0002, d: 0.006 },
            drive: 1.3,
            gain: 0.767,
            ring: 0.6,
            ..d()
        },
        "biwa" => Recipe {
            // 琵琶。太い弦を撥で叩く。サワリが鳴る
            osc: vec![Osc { wave: Wave::String, decay: 1.8, bright: 0.86, pick: 0.07, ..o() }],
            env: Env { a: 0.0005, d: 1.6, s: 1.0, r: 0.08 },
            body: vec![(240.0, 5.0, 0.4), (820.0, 6.0, 0.25)],
            attack: Attack { amount: 0.26, hp: 2400.0, a: 0.0002, d: 0.007 },
            drive: 1.5,
            gain: 0.703,
            ring: 0.9,
            ..d()
        },
        "sanshin" => Recipe {
            // 三線。三味線より丸く、沖縄の音
            osc: vec![Osc { wave: Wave::String, decay: 1.4, bright: 0.66, pick: 0.16, ..o() }],
            env: Env { a: 0.0008, d: 1.3, s: 1.0, r: 0.08 },
            body: vec![(280.0, 5.0, 0.4), (720.0, 6.0, 0.18)],
            attack: Attack { amount: 0.14, hp: 2600.0, a: 0.0002, d: 0.006 },
            gain: 0.576,
            ring: 0.7,
            ..d()
        },
        "kokyu" => Recipe {
            // 胡弓。和の擦弦。細くて鼻に掛かる
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -4.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 4.0, ..o() }],
            env: Env { a: 0.06, d: 0.15, s: 0.86, r: 0.16 },
            filter: Filter { kind: FilterKind::Bandpass, base: 1500.0, res: 0.45, track: 0.6, ..f() },
            body: vec![(700.0, 9.0, 0.4), (1800.0, 7.0, 0.22)],
            vibrato: Vibrato { rate: 6.0, depth: 0.009, delay: 0.2 },
            gain: 1.574,
            ..d()
        },
        "sho" => Recipe {
            // 笙。17本の竹が同時に鳴る。雅楽のあの和音
            osc: vec![Osc { wave: Wave::Pulse, mix: 1.0, width: 0.3, detune: -5.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.8, width: 0.22, detune: 5.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 0.5, octave: 1, ..o() },
                      Osc { wave: Wave::Saw, mix: 0.35, detune: 703.0, ..o() }],
            env: Env { a: 0.12, d: 0.1, s: 0.92, r: 0.2 },
            filter: Filter { kind: FilterKind::Ladder, base: 2600.0, sweep: 0.0, res: 0.2, ..f() },
            body: vec![(1500.0, 6.0, 0.25)],
            gain: 0.52,
            ..d()
        },
        "hichiriki" => Recipe {
            // 篳篥。雅楽の主旋律。強く、鼻に掛かって、揺れる
            osc: vec![Osc { wave: Wave::Saw, mix: 0.7, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.3, width: 0.25, ..o() }],
            env: Env { a: 0.03, d: 0.12, s: 0.88, r: 0.1 },
            filter: Filter { kind: FilterKind::Ladder, base: 900.0, sweep: 1800.0, res: 0.35,
                             env: (0.025, 0.2, 1.4), ..f() },
            body: vec![(1100.0, 9.0, 0.45), (2600.0, 7.0, 0.25)],
            vibrato: Vibrato { rate: 5.0, depth: 0.010, delay: 0.25 },
            drive: 1.6,
            gain: 0.691,
            ..d()
        },

        // ---- その他の民族楽器
        "koto12" => Recipe {
            // 大正琴。金属弦を鍵盤で押さえる
            osc: vec![Osc { wave: Wave::String, decay: 1.8, bright: 0.80, pick: 0.12, ..o() }],
            env: Env { a: 0.0008, d: 1.6, s: 1.0, r: 0.1 },
            body: vec![(400.0, 7.0, 0.35), (1100.0, 8.0, 0.18)],
            vibrato: Vibrato { rate: 6.5, depth: 0.008, delay: 0.2 },
            gain: 0.558,
            ring: 0.9,
            ..d()
        },
        "charango" => Recipe {
            // チャランゴ。小さくて高い。アンデスの弦
            osc: vec![
                Osc { wave: Wave::String, decay: 1.1, bright: 0.72, pick: 0.20, detune: -5.0, ..o() },
                Osc { wave: Wave::String, decay: 1.1, bright: 0.72, pick: 0.20, detune: 5.0, ..o() },
            ],
            env: Env { a: 0.0008, d: 1.0, s: 1.0, r: 0.07 },
            body: vec![(380.0, 7.0, 0.4), (860.0, 8.0, 0.2)],
            gain: 0.629,
            ring: 0.6,
            ..d()
        },
        "kora" => Recipe {
            // コラ。西アフリカの21弦。ハープに近いが胴が瓢箪
            osc: vec![Osc { wave: Wave::String, decay: 2.6, bright: 0.58, pick: 0.25, ..o() }],
            env: Env { a: 0.001, d: 2.4, s: 1.0, r: 0.15 },
            body: vec![(210.0, 4.0, 0.45), (520.0, 5.0, 0.2)],
            gain: 0.579,
            ring: 1.3,
            ..d()
        },
        "steeldrum" => Recipe {
            // スチールドラム。ドラム缶を叩き出した音板
            osc: vec![Osc { wave: Wave::Sine, mix: 0.7, ..o() }],
            partials: vec![(2.0, 0.25, 1.2), (3.0, 0.15, 0.8), (4.9, 0.08, 0.5)],
            env: Env { a: 0.002, d: 1.4, s: 0.0, r: 0.3 },
            body: vec![(700.0, 6.0, 0.2)],
            gain: 0.53,
            ring: 1.0,
            ..d()
        },
        "sitarstring" => Recipe {
            // シタール。共鳴弦が一緒に鳴るので、賑やかに伸びる
            osc: vec![
                Osc { wave: Wave::String, decay: 3.5, bright: 0.85, pick: 0.10, ..o() },
                Osc { wave: Wave::String, mix: 0.3, decay: 4.0, bright: 0.9, pick: 0.3,
                      octave: 1, detune: 8.0, ..o() },
            ],
            env: Env { a: 0.0008, d: 3.2, s: 1.0, r: 0.2 },
            body: vec![(240.0, 5.0, 0.4), (1400.0, 7.0, 0.25)],
            attack: Attack { amount: 0.18, hp: 2600.0, a: 0.0002, d: 0.006 },
            drive: 1.4,
            gain: 0.792,
            ring: 1.8,
            ..d()
        },

        // ---- 敷くもの（パッド）。今まで1つも無かった
        "warmpad" => Recipe {
            // 温かいパッド。ゆっくり開いて、ゆっくり閉じる
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -9.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 9.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 0.6, octave: -1, ..o() }],
            env: Env { a: 0.55, d: 0.6, s: 0.85, r: 0.9 },
            filter: Filter { kind: FilterKind::Ladder, base: 500.0, sweep: 2200.0, res: 0.2,
                             track: 0.4, env: (0.5, 1.2, 1.0), ..f() },
            gain: 1.201,
            ring: 0.6,
            ..d()
        },
        "glasspad" => Recipe {
            // 冷たいパッド。倍音が高い所に散る
            osc: vec![Osc { wave: Wave::Sine, mix: 0.6, ..o() },
                      Osc { wave: Wave::Sine, mix: 0.4, detune: 1204.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.25, width: 0.2, octave: 1, ..o() }],
            partials: vec![(3.0, 0.08, 2.5), (5.1, 0.05, 1.8)],
            env: Env { a: 0.8, d: 0.8, s: 0.8, r: 1.2 },
            filter: Filter { kind: FilterKind::Ladder, base: 1200.0, sweep: 3000.0, res: 0.25,
                             env: (0.7, 1.5, 1.0), ..f() },
            tremolo: Tremolo { rate: 0.7, depth: 0.2 },
            gain: 1.031,
            ring: 1.0,
            ..d()
        },
        "choirpad" => Recipe {
            // 人の声を敷いたようなパッド。母音の山を持たせる
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -7.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 7.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 0.5, detune: 1195.0, ..o() }],
            env: Env { a: 0.6, d: 0.5, s: 0.88, r: 1.0 },
            filter: Filter { kind: FilterKind::Ladder, base: 800.0, sweep: 1400.0, res: 0.18,
                             env: (0.55, 1.2, 1.0), ..f() },
            body: vec![(600.0, 8.0, 0.35), (1100.0, 8.0, 0.25)],
            vibrato: Vibrato { rate: 4.6, depth: 0.004, delay: 0.8 },
            gain: 1.101,
            ring: 0.8,
            ..d()
        },
        "sweeppad" => Recipe {
            // ゆっくり開き切るパッド。場面の変わり目に敷く
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -14.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 14.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.4, width: 0.35, octave: -1, ..o() }],
            env: Env { a: 0.9, d: 1.0, s: 0.8, r: 1.4 },
            filter: Filter { kind: FilterKind::Ladder, base: 260.0, sweep: 7000.0, res: 0.35,
                             env: (1.6, 2.0, 0.8), ..f() },
            gain: 1.355,
            ring: 1.2,
            ..d()
        },
        "stringmachine" => Recipe {
            // 70年代の弦の機械。本物の弦より薄くて、ゆらゆらしている
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -11.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 0.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 11.0, ..o() }],
            env: Env { a: 0.25, d: 0.3, s: 0.9, r: 0.5 },
            filter: Filter { kind: FilterKind::Ladder, base: 1500.0, sweep: 900.0, res: 0.15,
                             env: (0.2, 0.6, 1.0), ..f() },
            tremolo: Tremolo { rate: 5.4, depth: 0.15 },
            gain: 1.062,
            ring: 0.4,
            ..d()
        },
        "bellpad" => Recipe {
            // 鐘を敷いたもの。頭が鐘で、後ろが伸びる
            osc: vec![Osc { wave: Wave::Sine, mix: 0.7, ..o() }],
            partials: vec![(2.76, 0.18, 2.5), (5.4, 0.1, 1.5)],
            env: Env { a: 0.3, d: 1.5, s: 0.55, r: 1.2 },
            filter: Filter { kind: FilterKind::Ladder, base: 1400.0, sweep: 2000.0, res: 0.2,
                             env: (0.25, 1.0, 1.0), ..f() },
            gain: 0.568,
            ring: 1.4,
            ..d()
        },

        // ---- 刻むもの
        "plucksynth" => Recipe {
            // 短く弾けるシンセ。刻みに使う定番
            osc: vec![Osc { wave: Wave::Saw, mix: 0.7, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.3, width: 0.3, ..o() }],
            env: Env { a: 0.001, d: 0.22, s: 0.0, r: 0.08 },
            filter: Filter { kind: FilterKind::Ladder, base: 400.0, sweep: 6000.0, res: 0.4,
                             vel: 2000.0, env: (0.001, 0.12, 3.0), ..f() },
            gain: 1.558,
            ring: 0.12,
            ..d()
        },
        "bellsynth" => Recipe {
            // 鐘のように鳴る刻み。FM で金属質にする
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            fm: Fm { ratio: 5.1, index: 7.0, decay: 0.22 },
            env: Env { a: 0.001, d: 0.9, s: 0.0, r: 0.2 },
            gain: 0.4,
            ring: 0.5,
            ..d()
        },
        "pwmlead" => Recipe {
            // 幅がゆっくり動くパルス。あの「うねる」音
            //（幅そのものは動かせないので、細い2本の唸りで作る）
            osc: vec![Osc { wave: Wave::Pulse, mix: 1.0, width: 0.3, detune: -6.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 1.0, width: 0.22, detune: 6.0, ..o() }],
            env: Env { a: 0.01, d: 0.1, s: 0.85, r: 0.08 },
            filter: Filter { kind: FilterKind::Ladder, base: 1200.0, sweep: 3500.0, res: 0.3,
                             vel: 1500.0, env: (0.008, 0.2, 1.6), ..f() },
            gain: 0.594,
            ..d()
        },
        "hoover" => Recipe {
            // 90年代レイヴの、あの下がってくる音
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -22.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 0.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 22.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.6, width: 0.15, octave: -1, ..o() }],
            env: Env { a: 0.006, d: 0.3, s: 0.8, r: 0.12 },
            filter: Filter { kind: FilterKind::Ladder, base: 700.0, sweep: 5000.0, res: 0.45,
                             env: (0.004, 0.5, 1.6), ..f() },
            vibrato: Vibrato { rate: 5.0, depth: 0.012, delay: 0.06 },
            drive: 2.4,
            gain: 0.647,
            ..d()
        },
        "organbass" => Recipe {
            // オルガンの足鍵盤。丸くて太い
            osc: vec![Osc { wave: Wave::Sine, mix: 1.0, ..o() },
                      Osc { wave: Wave::Sine, mix: 0.4, octave: 1, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.2, width: 0.5, ..o() }],
            env: Env { a: 0.006, d: 0.06, s: 0.92, r: 0.06 },
            filter: Filter { kind: FilterKind::Ladder, base: 600.0, sweep: 0.0, res: 0.15, ..f() },
            gain: 0.505,
            ..d()
        },
        "clickbass" => Recipe {
            // 頭だけ硬いベース。低い所は丸いまま
            osc: vec![Osc { wave: Wave::Sine, mix: 1.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.25, width: 0.2, octave: 1, ..o() }],
            env: Env { a: 0.001, d: 0.35, s: 0.55, r: 0.06 },
            filter: Filter { kind: FilterKind::Ladder, base: 200.0, sweep: 3000.0, res: 0.3,
                             env: (0.0008, 0.03, 4.0), ..f() },
            attack: Attack { amount: 0.1, hp: 2500.0, a: 0.0002, d: 0.004 },
            drive: 1.5,
            gain: 1.071,
            ..d()
        },

        // ---- 鍵盤の残り
        "wurli" => Recipe {
            // ウーリッツァー。ローズより硬くて、歪むと鳴く
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            fm: Fm { ratio: 2.0, index: 7.0, decay: 0.09 },
            env: Env { a: 0.002, d: 1.1, s: 0.18, r: 0.2 },
            drive: 2.2,
            gain: 0.257,
            ring: 0.4,
            ..d()
        },
        "celesta" => Recipe {
            // チェレスタ。金属板を叩く鍵盤。澄んだ高音
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            partials: vec![(4.0, 0.14, 0.9), (8.1, 0.06, 0.5)],
            env: Env { a: 0.001, d: 1.6, s: 0.0, r: 0.35 },
            gain: 0.381,
            ring: 1.0,
            ..d()
        },
        "musicbox" => Recipe {
            // オルゴール。短い金属の櫛を弾く
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            partials: vec![(3.0, 0.2, 0.6), (6.3, 0.1, 0.35), (10.7, 0.05, 0.2)],
            env: Env { a: 0.0008, d: 1.0, s: 0.0, r: 0.25 },
            gain: 0.373,
            ring: 0.7,
            ..d()
        },
        "toypiano" => Recipe {
            // トイピアノ。金属棒を叩くので、ピアノより鐘に近い
            osc: vec![Osc { wave: Wave::Sine, mix: 0.6, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.4, width: 0.2, ..o() }],
            partials: vec![(5.4, 0.12, 0.35)],
            env: Env { a: 0.001, d: 0.7, s: 0.0, r: 0.12 },
            attack: Attack { amount: 0.12, hp: 3500.0, a: 0.0002, d: 0.004 },
            gain: 0.432,
            ring: 0.35,
            ..d()
        },
        "harmonium" => Recipe {
            // 足踏みオルガン。舌が震えるので、少し濁る
            osc: vec![Osc { wave: Wave::Saw, mix: 0.6, detune: -6.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.4, width: 0.4, detune: 6.0, ..o() },
                      Osc { wave: Wave::Sine, mix: 0.4, octave: -1, ..o() }],
            env: Env { a: 0.05, d: 0.08, s: 0.92, r: 0.12 },
            filter: Filter { kind: FilterKind::Ladder, base: 1800.0, sweep: 0.0, res: 0.12, ..f() },
            gain: 0.607,
            ..d()
        },
        "leslie" => Recipe {
            // 回るスピーカーを通したオルガン。揺れがその正体
            osc: vec![Osc { wave: Wave::Sine, mix: 1.0, ..o() },
                      Osc { wave: Wave::Sine, mix: 0.5, octave: 1, ..o() },
                      Osc { wave: Wave::Sine, mix: 0.3, octave: -1, ..o() },
                      Osc { wave: Wave::Sine, mix: 0.25, detune: 1902.0, ..o() }],
            env: Env { a: 0.008, d: 0.05, s: 0.95, r: 0.08 },
            tremolo: Tremolo { rate: 6.6, depth: 0.35 },
            vibrato: Vibrato { rate: 6.6, depth: 0.004, delay: 0.0 },
            drive: 1.5,
            gain: 0.295,
            ..d()
        },

        // ---- 金属と木の打
        "xylophone" => Recipe {
            // 木琴。マリンバより短くて硬い
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            partials: vec![(3.0, 0.3, 0.12), (6.8, 0.12, 0.07)],
            env: Env { a: 0.0006, d: 0.28, s: 0.0, r: 0.06 },
            gain: 0.381,
            ring: 0.15,
            ..d()
        },
        "tubularbell" => Recipe {
            // チューブラーベル。教会の鐘を管で作ったもの
            osc: vec![],
            partials: vec![(1.0, 0.4, 4.0), (1.95, 0.3, 3.0), (2.99, 0.2, 2.2),
                           (4.18, 0.12, 1.5), (5.43, 0.07, 1.0)],
            env: Env { a: 0.002, d: 4.0, s: 0.0, r: 1.0 },
            gain: 0.691,
            ring: 3.0,
            ..d()
        },
        "handpan" => Recipe {
            // ハンドパン。鋼の椀。柔らかくて伸びる
            osc: vec![Osc { wave: Wave::Sine, mix: 0.7, ..o() }],
            partials: vec![(2.0, 0.22, 2.0), (3.0, 0.12, 1.4), (5.1, 0.05, 0.8)],
            env: Env { a: 0.004, d: 2.2, s: 0.0, r: 0.5 },
            body: vec![(300.0, 5.0, 0.2)],
            gain: 0.339,
            ring: 1.6,
            ..d()
        },
        "timpani" => Recipe {
            // ティンパニ。皮と胴。音程はあるが、頭は打撃
            osc: vec![Osc { wave: Wave::Sine, mix: 0.8, ..o() },
                      Osc { wave: Wave::Sine, mix: 0.3, octave: -1, ..o() }],
            partials: vec![(1.5, 0.18, 0.6), (2.3, 0.1, 0.35)],
            env: Env { a: 0.002, d: 1.2, s: 0.0, r: 0.3 },
            attack: Attack { amount: 0.3, hp: 400.0, a: 0.0004, d: 0.02 },
            body: vec![(90.0, 4.0, 0.4)],
            drive: 1.3,
            gain: 0.406,
            ring: 0.8,
            ..d()
        },
        "taiko" => Recipe {
            // 和太鼓。胴が長く、低い所が伸びる
            osc: vec![Osc { wave: Wave::Sine, mix: 0.85, ..o() }],
            env: Env { a: 0.001, d: 0.55, s: 0.0, r: 0.15 },
            attack: Attack { amount: 0.45, hp: 250.0, a: 0.0003, d: 0.03 },
            body: vec![(70.0, 3.5, 0.5), (180.0, 4.0, 0.2)],
            drive: 1.6,
            gain: 0.303,
            ring: 0.4,
            ..d()
        },
        "woodblock" => Recipe {
            // 木。音程はあるが、ほぼ一瞬で消える
            osc: vec![Osc { wave: Wave::Sine, mix: 0.5, ..o() }],
            partials: vec![(2.8, 0.3, 0.05), (5.1, 0.15, 0.03)],
            env: Env { a: 0.0003, d: 0.09, s: 0.0, r: 0.03 },
            attack: Attack { amount: 0.25, hp: 2000.0, a: 0.0002, d: 0.004 },
            gain: 0.379,
            ring: 0.05,
            ..d()
        },
        "logdrum" => Recipe {
            // スリットドラム。木の箱の中で鳴る
            osc: vec![Osc { wave: Wave::Sine, ..o() }],
            partials: vec![(2.4, 0.16, 0.35)],
            env: Env { a: 0.001, d: 0.5, s: 0.0, r: 0.12 },
            body: vec![(400.0, 6.0, 0.3)],
            gain: 0.387,
            ring: 0.3,
            ..d()
        },

        // ---- 声
        "aah" => Recipe {
            // 「あー」。口を開けた母音。下の2つの山がその正体
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -6.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 6.0, ..o() }],
            env: Env { a: 0.06, d: 0.15, s: 0.88, r: 0.25 },
            filter: Filter { kind: FilterKind::Ladder, base: 1100.0, sweep: 600.0, res: 0.15,
                             env: (0.05, 0.3, 1.2), ..f() },
            body: vec![(730.0, 9.0, 0.5), (1090.0, 9.0, 0.35), (2440.0, 7.0, 0.15)],
            vibrato: Vibrato { rate: 5.2, depth: 0.006, delay: 0.35 },
            gain: 0.801,
            ..d()
        },
        "ooh" => Recipe {
            // 「うー」。口を丸めた母音。上が少ない
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -5.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 5.0, ..o() }],
            env: Env { a: 0.07, d: 0.15, s: 0.9, r: 0.3 },
            filter: Filter { kind: FilterKind::Ladder, base: 700.0, sweep: 300.0, res: 0.15,
                             env: (0.06, 0.3, 1.2), ..f() },
            body: vec![(300.0, 10.0, 0.55), (870.0, 9.0, 0.25)],
            vibrato: Vibrato { rate: 5.0, depth: 0.005, delay: 0.4 },
            gain: 0.788,
            ..d()
        },
        "hum" => Recipe {
            // 鼻歌。口を閉じているので、外へ出る上が少ない
            osc: vec![Osc { wave: Wave::Saw, mix: 1.0, detune: -4.0, ..o() },
                      Osc { wave: Wave::Saw, mix: 1.0, detune: 4.0, ..o() }],
            env: Env { a: 0.05, d: 0.12, s: 0.9, r: 0.25 },
            filter: Filter { kind: FilterKind::Ladder, base: 500.0, sweep: 200.0, res: 0.12,
                             env: (0.05, 0.3, 1.2), ..f() },
            body: vec![(280.0, 10.0, 0.5), (1100.0, 6.0, 0.12)],
            vibrato: Vibrato { rate: 4.8, depth: 0.004, delay: 0.4 },
            gain: 0.709,
            ..d()
        },
        "whisper" => Recipe {
            // ささやき。声帯を使わないので、音程が無く雑音だけ
            osc: vec![Osc { wave: Wave::Noise, ..o() }],
            env: Env { a: 0.04, d: 0.1, s: 0.85, r: 0.15 },
            filter: Filter { kind: FilterKind::Bandpass, base: 1600.0, res: 0.25, track: 0.6, ..f() },
            body: vec![(730.0, 6.0, 0.3), (2400.0, 5.0, 0.2)],
            gain: 1.447,
            ..d()
        },

        // ---- チップチューン。パルス幅を切り替えるのがこの音の要
        "nespulse" => Recipe {
            // ファミコンの主旋律。25% のパルス
            osc: vec![Osc { wave: Wave::Pulse, width: 0.25, ..o() }],
            env: Env { a: 0.0, d: 0.02, s: 0.95, r: 0.01 },
            gain: 0.189,
            ..d()
        },
        "nesthin" => Recipe {
            // 12.5%。細くて鼻に掛かる。裏メロに使う
            osc: vec![Osc { wave: Wave::Pulse, width: 0.125, ..o() }],
            env: Env { a: 0.0, d: 0.02, s: 0.95, r: 0.01 },
            gain: 0.248,
            ..d()
        },
        "neslead" => Recipe {
            // 50%（矩形波）に揺れを付けたもの
            osc: vec![Osc { wave: Wave::Pulse, width: 0.5, ..o() }],
            env: Env { a: 0.0, d: 0.03, s: 0.92, r: 0.02 },
            vibrato: Vibrato { rate: 6.5, depth: 0.008, delay: 0.12 },
            gain: 0.174,
            ..d()
        },
        "nestri" => Recipe {
            // ファミコンの三角波。ベースを担当していた
            osc: vec![Osc { wave: Wave::Sine, mix: 1.0, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.25, width: 0.5, ..o() }],
            env: Env { a: 0.0, d: 0.02, s: 0.95, r: 0.01 },
            gain: 0.224,
            ..d()
        },
        "nesarp" => Recipe {
            // 和音を高速で回して和音に聞かせる、あの音。短く硬い
            osc: vec![Osc { wave: Wave::Pulse, width: 0.125, ..o() }],
            env: Env { a: 0.0, d: 0.05, s: 0.0, r: 0.01 },
            gain: 0.495,
            ring: 0.02,
            ..d()
        },
        "gameboy" => Recipe {
            // ゲームボーイ。同じパルスだが、少し濁って丸い
            osc: vec![Osc { wave: Wave::Pulse, width: 0.25, ..o() },
                      Osc { wave: Wave::Pulse, mix: 0.3, width: 0.125, detune: 7.0, ..o() }],
            env: Env { a: 0.001, d: 0.04, s: 0.9, r: 0.02 },
            filter: Filter { kind: FilterKind::Ladder, base: 5000.0, sweep: 0.0, res: 0.1, ..f() },
            drive: 1.3,
            gain: 0.271,
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
            gain: 0.353,
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
            gain: 0.477,
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
            gain: 0.285,
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
            gain: 0.63,
            ..d()
        },
        "horn" => Recipe {
            // 丸い管。上が少ないので柔らかい
            osc: vec![Osc { wave: Wave::Saw, ..o() }],
            env: Env { a: 0.07, d: 0.18, s: 0.85, r: 0.18 },
            filter: Filter { kind: FilterKind::Ladder, base: 500.0, sweep: 1600.0, res: 0.18,
                             vel: 900.0, env: (0.06, 0.3, 1.3), ..f() },
            body: vec![(480.0, 4.0, 0.25)],
            gain: 0.544,
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
            gain: 0.713,
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
            gain: 0.934,
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
            gain: 0.44,
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
            gain: 0.333,
            ..d()
        },
        _ => return None,
    };
    Some(r)
}

/// ここで足した楽器の名前。
pub const NAMES: &[&str] = &[
    "kotostring",
    "shamisenstring",
    "biwa",
    "sanshin",
    "kokyu",
    "sho",
    "hichiriki",
    "koto12",
    "charango",
    "kora",
    "steeldrum",
    "sitarstring",
    "warmpad",
    "glasspad",
    "choirpad",
    "sweeppad",
    "stringmachine",
    "bellpad",
    "plucksynth",
    "bellsynth",
    "pwmlead",
    "hoover",
    "organbass",
    "clickbass",
    "wurli",
    "celesta",
    "musicbox",
    "toypiano",
    "harmonium",
    "leslie",
    "xylophone",
    "tubularbell",
    "handpan",
    "timpani",
    "taiko",
    "woodblock",
    "logdrum",
    "aah",
    "ooh",
    "hum",
    "whisper",
    "nespulse",
    "nesthin",
    "neslead",
    "nestri",
    "nesarp",
    "gameboy",
    "trombone",
    "tuba",
    "mutetrumpet",
    "bassoon",
    "piccolo",
    "recorder",
    "melodica",
    "bagpipe",
    "viola",
    "contrabass",
    "tremolostrings",
    "slapbass",
    "fretless",
    "twelvestring",
    "balalaika",
    "bouzouki",
    "guzheng",
    "pipa",
    "dulcimer",
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

    /// **鳴っている区間**の実効値。
    ///
    /// 決まった長さで測ると比べられない。木魚は 0.1 秒で消えるし、パッドは
    /// 0.8 秒かけて立ち上がる。頭で測るとパッドが不当に小さく出て、1秒で
    /// 測ると木魚が不当に小さく出る。ピークの 1割 を超えている区間だけ見る。
    fn active_rms(x: &[f32]) -> f32 {
        let peak = x.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        if peak <= 1e-9 {
            return 0.0;
        }
        let live = peak * 0.1;
        let from = x.iter().position(|v| v.abs() > live).unwrap_or(0);
        let to = x.iter().rposition(|v| v.abs() > live).unwrap_or(x.len() - 1);
        rms(&x[from..=to.max(from)])
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
                // **単体で 1.0 を超えないこと。** 超えるものは、1本鳴らした
                // だけで割れる。混ぜる側で下げれば聞こえはするが、それは
                // 「この楽器だけ音量を下げて使う」という縛りになる
                let peak = w.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                assert!(peak < 1.0, "{n} が {hz}Hz で割れる（ピーク {peak:.2}）");
            }
        }
    }

    #[test]
    fn the_levels_are_in_the_same_ballpark() {
        // 持ち替えたら音量が桁違い、では使えない。
        //
        // 鳴っている区間で測る（[`active_rms`] を見よ）
        let mut loud: Vec<(f32, &str)> = NAMES
            .iter()
            .map(|n| {
                let w = recipe::render(&get(n).unwrap(), 261.63, 48_000 * 2, 1.0, 7);
                (active_rms(&w), *n)
            })
            .collect();
        loud.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let (min, max) = (loud[0], loud[loud.len() - 1]);
        assert!(
            max.0 / min.0 < 8.0,
            "音量の差が大きすぎる: {} {:.4} 〜 {} {:.4}（{:.1}倍）",
            min.1,
            min.0,
            max.1,
            max.0,
            max.0 / min.0
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
