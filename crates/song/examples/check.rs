fn main() {
    let p = std::env::args().nth(1).unwrap();
    match tonescript_song::load_file(std::path::Path::new(&p)) {
        Ok(s) => {
            println!("曲: {}  {} BPM  {}", s.title, s.bpm, s.key);
            println!("  全{}小節（{}）", s.bars(),
                s.sections.iter().map(|x| format!("{}{}", x.name, x.bars))
                    .collect::<Vec<_>>().join(" + "));
            println!("  和音 {} 小節 / 旋律 {} 小節 / 編成 {} 小節",
                s.chords.len(), s.melody.len(), s.arrange.len());
            println!("  音色 {} / 編集パート {}", s.voices.len(), s.edit_parts.len());
            let mut b: Vec<_> = s.bass_patterns.keys().collect(); b.sort();
            let mut a: Vec<_> = s.arp_patterns.keys().collect(); a.sort();
            let mut k: Vec<_> = s.drum_kits.keys().collect(); k.sort();
            println!("  ベース型 {:?} / リフ型 {:?} / キット {:?}", b, a, k);
            println!("  octa の打点数 {}", s.bass_patterns.get("octa").map(|v| v.len()).unwrap_or(0));
            println!("  サイドチェイン {:?} / 残響 {:?}", s.sidechain, s.reverb);
            println!("  1小節目の和音 {:?}", s.chords.get(&1).map(|c| &c.name));
            println!("  9小節目の編成 {} パート", s.arrange.get(&9).map(|v| v.len()).unwrap_or(0));
        }
        Err(e) => { eprintln!("{e}"); std::process::exit(1); }
    }
}
