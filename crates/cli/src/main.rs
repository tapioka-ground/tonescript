//! Tonescript の入口。
//!
//!   tonescript list              曲の一覧
//!   tonescript check <曲>        曲ファイルを読めるか確かめる
//!   tonescript render <曲>       音にして WAV へ書き出す
//!   tonescript patches           使える音色の一覧
//!
//! 曲は songs/<名前>.rhai。書き出し先は TONESCRIPT_ROOT（既定は ./out）。

use tonescript_render::{mix, render_song_at, wav};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn songs_dir() -> PathBuf {
    std::env::var("TONESCRIPT_SONGS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("songs"))
}

fn out_dir() -> PathBuf {
    std::env::var("TONESCRIPT_ROOT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("out"))
}

fn song_names() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(songs_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("rhai") {
                if let Some(n) = p.file_stem().and_then(|s| s.to_str()) {
                    out.push(n.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

fn song_path(name: &str) -> PathBuf {
    songs_dir().join(format!("{name}.rhai"))
}

fn load(name: &str) -> Result<tonescript_song::Song, String> {
    let p = song_path(name);
    if !p.exists() {
        let have = song_names();
        return Err(format!(
            "曲が見つかりません: {}\n    あるのは: {}\n    新しく作るには songs/example.rhai を写してください",
            p.display(),
            if have.is_empty() { "（1つもありません）".into() } else { have.join(" / ") }
        ));
    }
    tonescript_song::load_file(&p).map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("list") => cmd_list(),
        Some("patches") => cmd_patches(),
        Some("check") => cmd_check(args.get(1)),
        Some("render") => cmd_render(args.get(1)),
        Some("project") => cmd_project(args.get(1)),
        Some("midi-out") => cmd_midi_out(args.get(1), args.get(2)),
        Some("midi-in") => cmd_midi_in(args.get(1), args.get(2)),
        Some("-h") | Some("--help") | Some("help") | None => {
            usage();
            0
        }
        Some(other) => {
            eprintln!("[!] 知らないコマンド: {other}");
            usage();
            2
        }
    };
    std::process::exit(code);
}

fn usage() {
    println!("Tonescript — Rust だけで書いた音楽制作環境");
    println!();
    println!("  tonescript list           曲の一覧");
    println!("  tonescript check  <曲>    曲ファイルを読めるか確かめる");
    println!("  tonescript render <曲>    音にして WAV へ書き出す");
    println!("  tonescript patches        使える音色の一覧");
    println!("  tonescript project <曲>   保存の状態（世代・自動保存）を見る");
    println!("  tonescript midi-out <曲> [書き出し先]   MIDI へ持ち出す");
    println!("  tonescript midi-in  <曲> <MIDI>         MIDI から持ち込む（中身を見るだけ）");
    println!();
    println!("  曲の置き場   {}  （TONESCRIPT_SONGS で変えられる）", songs_dir().display());
    println!("  書き出し先   {}  （TONESCRIPT_ROOT で変えられる）", out_dir().display());
}

fn cmd_list() -> i32 {
    let names = song_names();
    if names.is_empty() {
        println!("曲がありません。songs/example.rhai を写して作ってください。");
        return 0;
    }
    for n in &names {
        match tonescript_song::load_file(&song_path(n)) {
            Ok(s) => println!("  {:<20} {:<24} {:>3} BPM  {:>3}小節  {}", n, s.title, s.bpm, s.bars(), s.key),
            Err(e) => println!("  {:<20} [!] {e}", n),
        }
    }
    0
}

fn cmd_patches() -> i32 {
    let names = tonescript_dsp::patch::NAMES;
    println!("音色 {} 種:", names.len());
    for row in names.chunks(6) {
        println!("  {}", row.join("  "));
    }
    0
}

fn cmd_check(name: Option<&String>) -> i32 {
    let Some(name) = name else {
        eprintln!("[!] 曲の名前を指定してください");
        return 2;
    };
    match load(name) {
        Ok(s) => {
            println!("OK  {} / {} BPM / {} / 全{}小節", s.title, s.bpm, s.key, s.bars());
            match tonescript_render::build(&s) {
                Ok(score) => {
                    let mut parts: Vec<&String> = score.keys().collect();
                    parts.sort();
                    for p in parts {
                        println!("    {:<8} {:>5} ノート", p, score[p].len());
                    }
                    0
                }
                Err(e) => {
                    eprintln!("[!] {e}");
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("[!] {e}");
            1
        }
    }
}

fn cmd_render(name: Option<&String>) -> i32 {
    let Some(name) = name else {
        eprintln!("[!] 曲の名前を指定してください");
        return 2;
    };
    let song = match load(name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[!] {e}");
            return 1;
        }
    };
    let t = Instant::now();
    let say = |m: &str| println!("{m}");
    let root = out_dir();
    let (out, stems) = match render_song_at(&song, &root, &say) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[!] {e}");
            return 1;
        }
    };
    let dir = out_dir().join(name);
    let full = dir.join("full.wav");
    if let Err(e) = wav::write_stereo(&full, &out, 48_000) {
        eprintln!("[!] 書き出せません: {e}");
        return 1;
    }
    for (part, buf) in &stems {
        let p = dir.join("parts").join(format!("{part}.wav"));
        if let Err(e) = wav::write_mono(&p, buf, 48_000) {
            eprintln!("[!] {part}: {e}");
        }
    }
    let secs = out.len() as f32 / 48_000.0;
    let took = t.elapsed().as_secs_f32();
    println!("  -> {}", full.display());
    println!("  -> パート別 {}", dir.join("parts").display());
    println!(
        "  {:.1}秒の曲を {:.2}秒で作った（実時間の {:.0}倍速） / 音圧 {:.1} LUFS",
        secs,
        took,
        secs / took.max(1e-6),
        mix::lufs(&out.l, &out.r)
    );
    0
}

/// 保存の状態を見る。自動保存が残っていればそれも言う。
fn cmd_project(name: Option<&String>) -> i32 {
    let Some(name) = name else {
        eprintln!("[!] 曲の名前を指定してください");
        return 2;
    };
    let dir = std::env::var("TONESCRIPT_PROJECTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| out_dir().join("projects"));
    let st = tonescript_project::store::Store::new(&dir, name);
    println!("置き場  {}", dir.display());
    match st.load() {
        Ok(Some(p)) => {
            println!("本体    {}", st.main_path().display());
            let mut parts: Vec<&String> = p.notes.keys().collect();
            parts.sort();
            for part in parts {
                println!("  手で編集 {:<8} {:>4} ノート", part, p.notes[part].len());
            }
            for (part, lanes) in &p.automation {
                let mut names: Vec<&str> = lanes.keys().map(|l| l.name()).collect();
                names.sort();
                println!("  線       {:<8} {}", part, names.join(" "));
            }
            if !p.muted.is_empty() {
                println!("  黙らせ   {}", p.muted.join(" "));
            }
        }
        Ok(None) => println!("本体    まだありません"),
        Err(e) => {
            eprintln!("[!] {e}");
            return 1;
        }
    }
    let n = st.backups().len();
    println!("世代    {n} / {}", tonescript_project::store::BACKUPS);
    match st.pending_autosave() {
        Some(p) => {
            println!("自動保存 {}  ★ 前回きちんと閉じていません", p.display());
            println!("         画面を開くと、戻すか捨てるか聞かれます");
        }
        None => println!("自動保存 なし"),
    }
    0
}

/// MIDI へ持ち出す。
fn cmd_midi_out(name: Option<&String>, dest: Option<&String>) -> i32 {
    let Some(name) = name else {
        eprintln!("[!] 曲の名前を指定してください");
        return 2;
    };
    let song = match load(name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[!] {e}");
            return 1;
        }
    };
    let score = match tonescript_render::build(&song) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[!] {e}");
            return 1;
        }
    };
    let path = dest
        .map(PathBuf::from)
        .unwrap_or_else(|| out_dir().join(name).join(format!("{name}.mid")));
    match tonescript_midi::export(&song, &score, &path) {
        Ok(n) => {
            let smf = tonescript_midi::to_smf(&song, &score);
            println!("  -> {}  ({n} バイト)", path.display());
            println!("     トラック {} / テンポ {} 箇所 / 拍子 {} 箇所",
                     smf.tracks.len(), smf.tempos.len(), smf.time_sigs.len());
            for t in &smf.tracks {
                println!("     {:<12} ch{:<3} {:>5} ノート", t.name, t.channel, t.notes.len());
            }
            println!("     ※ MIDI は音を運びません。音色は番号だけです");
            0
        }
        Err(e) => {
            eprintln!("[!] {e}");
            1
        }
    }
}

/// MIDI を読んで、何が入っているか見せる。
fn cmd_midi_in(name: Option<&String>, src: Option<&String>) -> i32 {
    let (Some(name), Some(src)) = (name, src) else {
        eprintln!("[!] 曲の名前と MIDI ファイルを指定してください");
        return 2;
    };
    let song = match load(name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[!] {e}");
            return 1;
        }
    };
    match tonescript_midi::import(Path::new(src), &song) {
        Ok(got) => {
            println!("{}", src);
            if let Some(b) = got.bpm {
                println!("  テンポ {b:.1} BPM");
            }
            println!("  およそ {} 小節", got.bars);
            let mut parts: Vec<&String> = got.score.keys().collect();
            parts.sort();
            for p in parts {
                println!("  {:<10} {:>5} ノート", p, got.score[p].len());
            }
            for s in &got.skipped {
                println!("  [!] {s}");
            }
            println!("  （取り込みは画面から行ってください。ここでは中身を見るだけです）");
            0
        }
        Err(e) => {
            eprintln!("[!] {e}");
            1
        }
    }
}

fn _unused(_: &Path) {}
