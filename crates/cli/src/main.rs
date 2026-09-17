//! Tonescript の入口。
//!
//!   tonescript list              曲の一覧
//!   tonescript check <曲>        曲ファイルを読めるか確かめる
//!   tonescript render <曲>       音にして WAV へ書き出す
//!   tonescript patches           使える音色の一覧
//!
//! 曲は songs/<名前>.rhai。書き出し先は TONESCRIPT_ROOT（既定は ./out）。

use tonescript_render::{export, mix, render_song_at, wav};
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
        Some("patches") => cmd_patches(args.get(1)),
        Some("check") => cmd_check(args.get(1)),
        Some("render") => cmd_render(args.get(1), &args[1..]),
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
    println!("      --rate 44100|48000|96000   周波数（既定 48000）");
    println!("      --depth 16|24|32           深さ（既定 16。32 は小数）");
    println!("      --no-dither                16bit の丸めの粉を足さない");
    println!("  tonescript patches [曲]   音色の一覧（曲を渡すとその曲が作った音色も）");
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

/// 音色の一覧。曲を渡せば、その曲が作った音色も出す。
fn cmd_patches(name: Option<&String>) -> i32 {
    let names = tonescript_dsp::patch::NAMES;
    println!("内蔵 {} 種:", names.len());
    for row in names.chunks(6) {
        println!("  {}", row.join("  "));
    }
    let Some(name) = name else {
        println!();
        println!("曲の名前を渡すと、その曲が作った音色も出します");
        println!("  tonescript patches <曲>");
        return 0;
    };
    let song = match load(name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[!] {e}");
            return 1;
        }
    };
    if song.patches.is_empty() {
        println!();
        println!("{name} には PATCHES がありません（SONGFILE.md の §10）");
        return 0;
    }
    let mut own: Vec<&String> = song.patches.keys().collect();
    own.sort();
    println!();
    println!("{name} が作った音色 {} 種:", own.len());
    for k in own {
        let r = &song.patches[k];
        // 中身を一行で言う。どういう作りか思い出せるように
        let mut how = Vec::new();
        if !r.osc.is_empty() {
            how.push(format!("発振器 {}本", r.osc.len()));
        }
        if !r.partials.is_empty() {
            how.push(format!("倍音 {}個", r.partials.len()));
        }
        if r.filter.kind != tonescript_dsp::recipe::FilterKind::None {
            how.push(format!("{:?} {:.0}Hz", r.filter.kind, r.filter.base));
        }
        if r.fm.index > 0.0 {
            how.push(format!("FM x{:.1}", r.fm.ratio));
        }
        if r.attack.amount > 0.0 {
            how.push("頭に雑音".into());
        }
        if r.ring > 0.0 {
            how.push(format!("余韻 {:.2}秒", r.ring));
        }
        // 内蔵と同じ名前なら、上書きしていることを言う
        let over = if names.contains(&k.as_str()) { "（内蔵を上書き）" } else { "" };
        println!("  {:<14}{}  {}", k, over, how.join(" / "));
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

/// 書き出しの形を、渡された言葉から読む。
///
/// `--rate 44100` `--depth 24` `--no-dither`。書かなければ 48kHz / 16bit。
fn read_format(args: &[String]) -> Result<export::Format, String> {
    let mut fmt = export::Format::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--rate" => {
                let v = args.get(i + 1).ok_or("--rate のあとに周波数を書いてください")?;
                fmt.rate = match v.as_str() {
                    "44100" | "44.1" => export::Rate::R44100,
                    "48000" | "48" => export::Rate::R48000,
                    "96000" | "96" => export::Rate::R96000,
                    _ => return Err(format!("{v} Hz は選べません（44100 / 48000 / 96000）")),
                };
                i += 1;
            }
            "--depth" => {
                let v = args.get(i + 1).ok_or("--depth のあとに深さを書いてください")?;
                fmt.depth = match v.as_str() {
                    "16" => export::Depth::I16,
                    "24" => export::Depth::I24,
                    "32" | "float" => export::Depth::F32,
                    _ => return Err(format!("{v} bit は選べません（16 / 24 / 32）")),
                };
                i += 1;
            }
            "--no-dither" => fmt.dither = false,
            a if a.starts_with("--") => return Err(format!("{a} は知りません")),
            _ => {}
        }
        i += 1;
    }
    Ok(fmt)
}

fn cmd_render(name: Option<&String>, args: &[String]) -> i32 {
    let Some(name) = name else {
        eprintln!("[!] 曲の名前を指定してください");
        return 2;
    };
    let fmt = match read_format(args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[!] {e}");
            return 2;
        }
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
    // 形を整えてから書く。48kHz のままなら何もしない
    let shaped = export::shape(&out, 48_000, fmt);
    if let Err(e) =
        wav::write_stereo_as(&full, &shaped, fmt.rate.hz(), fmt.depth, fmt.dither)
    {
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
    println!(
        "  -> {}  （{} / {}{}）",
        full.display(),
        fmt.rate.label(),
        fmt.depth.label(),
        if fmt.dither && fmt.depth == export::Depth::I16 { " / 丸めの粉あり" } else { "" }
    );
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
