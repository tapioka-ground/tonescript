//! 鍵を押してから音が出るまで、どれだけ掛かるか。
//!
//! 押した瞬間に作るのは [`sched`] の `FIRST` 秒ぶんだけ。ここが全音色で
//! 十分速いことを確かめる。遅い音色が1つでもあると、その音色のときだけ
//! 「弾けない」と言われる。

use std::time::Instant;
use tonescript_engine::voice;

const FIRST: f32 = 0.30;

fn main() {
    let src = r#"
        let BPM = 120;
        let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
        let VOICES = #{ lead: #{ ch: 0, patch: "PATCH" } };
    "#;
    let names = tonescript_dsp::patch::NAMES;
    let mut rows: Vec<(f32, String)> = Vec::new();
    for patch in names {
        let s = tonescript_song::load_str(&src.replace("PATCH", patch)).unwrap();
        // 一度作って温めてから測る
        let _ = voice::render_live(&s, "lead", 69, 100, FIRST);
        let t = Instant::now();
        let n = 10;
        for _ in 0..n {
            std::hint::black_box(voice::render_live(&s, "lead", 69, 100, FIRST));
        }
        rows.push((t.elapsed().as_secs_f32() * 1000.0 / n as f32, patch.to_string()));
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("押した瞬間に作る {FIRST} 秒ぶんに掛かる時間（遅い順）");
    for (ms, name) in rows.iter().take(8) {
        println!("  {name:<12} {ms:>6.2} ms");
    }
    let worst = rows[0].0;
    let mean: f32 = rows.iter().map(|r| r.0).sum::<f32>() / rows.len() as f32;
    println!("  … 全 {} 音色  平均 {mean:.2} ms  最悪 {worst:.2} ms", rows.len());
    println!();
    println!("押してから鳴るまで = 作る時間 + 係の間隔 4ms + 出口の溜め（5〜10ms）");
    println!("               = だいたい {:.0}〜{:.0} ms", mean + 9.0, worst + 14.0);
}
