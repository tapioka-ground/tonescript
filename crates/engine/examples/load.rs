//! 締め切りに間に合っているかを測る。
//!
//! 音の出口は「1ブロックぶん鳴っているあいだに次のブロックを作れ」と
//! 言ってくる。512 サンプルなら 10.7 ミリ秒。これを1回でも超えるとプツッと
//! 鳴る。**平均ではなく最悪値**を見る。

use std::sync::Arc;
use std::time::{Duration, Instant};
use tonescript_dsp::osc::SR;
use tonescript_engine::Engine;
use tonescript_render::arrange;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "songs/example.rhai".into());
    let song = Arc::new(tonescript_song::load_file(std::path::Path::new(&path)).expect("曲が読めない"));
    let score = Arc::new(arrange::build(&song).expect("譜面が組めない"));
    let notes: usize = score.values().map(|v| v.len()).sum();
    println!("{path}  {} パート  {notes} ノート", score.len());

    for block in [256usize, 512, 1024] {
        let budget = block as f32 / SR * 1000.0;
        let (mut e, mut m) = Engine::new();
        e.set_song(song.clone(), score.clone());
        std::thread::sleep(Duration::from_millis(900));
        e.play();

        let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
        let mut worst = 0.0f32;
        let mut total = 0.0f64;
        let mut n = 0usize;
        let mut most_voices = 0u64;
        // 実時間で回す。出口と同じ速さで呼ぶ
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(6) && e.is_playing() {
            let t = Instant::now();
            m.fill(&mut l, &mut r);
            let ms = t.elapsed().as_secs_f32() * 1000.0;
            worst = worst.max(ms);
            total += ms as f64;
            n += 1;
            most_voices = most_voices.max(e.voices());
            // 鳴っているぶんだけ待つ
            let left = Duration::from_secs_f32(block as f32 / SR).saturating_sub(t.elapsed());
            std::thread::sleep(left);
        }
        println!(
            "  {block:>5} サンプル（締め切り {budget:>5.2}ms）  平均 {:.3}ms  最悪 {:.3}ms  \
             余裕 {:.0}倍  同時に鳴った音 最大 {most_voices}",
            total / n.max(1) as f64,
            worst,
            budget / worst.max(1e-6),
        );
    }
}
