//! 長く鳴らしたときに、使う覚えが増え続けないかを見る。
//!
//! 画面を開かずに、鳴らす側だけを本番と同じように回す。外から
//! `Get-Process` で見張れば、増え続けているかどうかが分かる。
//!
//! 曲を鳴らし、途中で頭出しと設定変更を混ぜる。**頭出しは代を進めるので、
//! 作り置きが捨てられて作り直される。** 捨て損ねがあればここで太る。

use std::sync::Arc;
use std::time::{Duration, Instant};

use tonescript_dsp::osc::SR;
use tonescript_engine::Engine;
use tonescript_render::arrange;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "songs/example.rhai".into());
    let secs: f32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let song =
        Arc::new(tonescript_song::load_file(std::path::Path::new(&path)).expect("曲が読めない"));
    let score = Arc::new(arrange::build(&song).expect("譜面が組めない"));
    let notes: usize = score.values().map(|v| v.len()).sum();
    println!("{path}  {} パート  {notes} ノート  {secs}秒ぶん回す", score.len());
    println!("外から見張る: Get-Process mem | Select WorkingSet64");

    // 画面で音符を引きずっている間と同じことをする。
    // **譜面を毎フレーム渡す。** ここで裏の測りが1本ずつ立つと、
    // すぐに機械の覚えを食い潰す
    let hammer = std::env::args().any(|a| a == "--hammer");

    let (mut e, mut m) = Engine::new();
    e.set_song(song.clone(), score.clone());
    std::thread::sleep(Duration::from_millis(900));
    e.play();

    let block = 512usize;
    let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
    let t0 = Instant::now();
    let mut pulled = 0u64;
    let mut round = 0u32;
    while t0.elapsed().as_secs_f32() < secs {
        m.fill(&mut l, &mut r);
        pulled += block as u64;
        // 出口と同じ速さで引き取る（先回り係に作る間を渡す）
        std::thread::sleep(Duration::from_micros((block as f32 / SR * 1e6) as u64));

        if hammer {
            // 毎フレーム譜面を渡し直す（音符を1つ引きずっている最中と同じ）
            e.set_score(song.clone(), score.clone());
        }

        // 2秒ごとに、人がしそうなことを混ぜる
        if pulled % (SR as u64 * 2) < block as u64 {
            round += 1;
            match round % 4 {
                0 => {
                    e.stop();
                    e.seek(0);
                    e.play();
                }
                1 => e.set_gain("lead", 0.5 + (round % 3) as f32 * 0.2),
                2 => e.set_eq("lead", -3.0, 0.0, 3.0),
                _ => e.set_pan("lead", ((round % 5) as f32 - 2.0) / 2.0),
            }
            println!(
                "{:>5.1}秒  鳴らした {:>6.1}秒  声 {:>3}  代 {}",
                t0.elapsed().as_secs_f32(),
                pulled as f32 / SR,
                e.voices(),
                round
            );
        }
    }
    println!("おわり。{:.1}秒ぶん鳴らした", pulled as f32 / SR);
}
