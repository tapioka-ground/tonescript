//! 長く作った音の頭は、短く作った音と同じか。
//! 同じなら、押している音を「少しずつ作り足す」ことができる。
use tonescript_engine::voice;

fn main() {
    let src = r#"
        let BPM = 120;
        let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
        let VOICES = #{ lead: #{ ch: 0, patch: "PATCH" } };
    "#;
    for patch in ["piano", "supersaw", "strings", "organ", "choir", "gamelan", "hardlead", "sub", "acid", "flute"] {
        let s = tonescript_song::load_str(&src.replace("PATCH", patch)).unwrap();
        let secs = 0.4f32;
        let short = voice::render_live(&s, "lead", 69, 100, secs).unwrap();
        let long = voice::render_live(&s, "lead", 69, 100, 6.0).unwrap();
        // 継ぎ目に選びたい所（頼んだ長さの 62%）で、どれだけ違うか
        let sr = 48_000f32;
        let swap = (secs * 0.62 * sr) as usize;
        let win = (0.010 * sr) as usize;
        let n = short.len().min(long.len());
        if swap + win < n {
            let (x, y) = (&short[swap..swap + win], &long[swap..swap + win]);
            let d: f32 = (x.iter().zip(y).map(|(a, b)| (a - b).powi(2)).sum::<f32>() / win as f32).sqrt();
            let r: f32 = (x.iter().map(|v| v * v).sum::<f32>() / win as f32).sqrt();
            let db = 20.0 * (d / r.max(1e-9)).log10();
            println!(
                "{patch:<10} 継ぎ目 {:.2}秒  違い {:.2}% （信号より {db:.0}dB 小さい）",
                swap as f32 / sr,
                d / r.max(1e-9) * 100.0
            );
        }
    }
}
