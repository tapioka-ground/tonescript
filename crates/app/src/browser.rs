//! 曲の一覧。開いたら最初に出る画面。
//!
//! Android Studio や GarageBand と同じ形で、まず「どれを開くか」を選ぶ。
//! 編集の画面に一覧を混ぜていたが、あそこは曲を作る所で、曲を選ぶ所ではない。
//! 分けたほうが、どちらも見やすくなる。
//!
//! 一覧に出す情報は、曲ファイルを**実行せずに**読めるものだけにしてある。
//! 曲が10本あったら10本ぶん Rhai を回すことになり、開くのが遅くなるため。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 一覧に出す1件。
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    /// 曲名。読めなければ空
    pub title: String,
    pub bpm: Option<u32>,
    pub key: String,
    /// 全部で何小節か。読めなければ None
    pub bars: Option<u32>,
    /// 最後に直した時刻
    pub modified: Option<SystemTime>,
    /// 読めないときの理由
    pub error: Option<String>,
    /// 編集の保存があるか
    pub has_project: bool,
    /// 前回きちんと閉じていないか
    pub pending: bool,
}

impl Entry {
    /// 「3日前」のような言い方。
    pub fn when(&self) -> String {
        let Some(t) = self.modified else { return String::new() };
        let Ok(d) = SystemTime::now().duration_since(t) else {
            return "たった今".into();
        };
        let s = d.as_secs();
        match s {
            0..=59 => "たった今".into(),
            60..=3599 => format!("{}分前", s / 60),
            3600..=86399 => format!("{}時間前", s / 3600),
            86400..=2591999 => format!("{}日前", s / 86400),
            _ => format!("{}か月前", s / 2592000),
        }
    }
}

/// 曲ファイルの頭のほうから、行頭の `let 名前 = 値;` を拾う。
///
/// Rhai を実行しない。一覧に出すだけなので、動かす必要がない。
/// 式で書かれていて読めない値は諦める（そのぶんは出さない）。
fn scan(src: &str) -> (String, Option<u32>, String, Option<u32>) {
    let mut title = String::new();
    let mut bpm = None;
    let mut key = String::new();
    let mut bars = 0u32;
    let mut in_sections = false;
    let mut depth = 0i32;

    for line in src.lines() {
        let t = line.trim();
        if let Some(v) = decl_str(t, "TITLE") {
            title = v;
        } else if let Some(v) = decl_str(t, "KEY") {
            key = v;
        } else if let Some(v) = decl_num(t, "BPM") {
            bpm = Some(v as u32);
        }

        // SECTIONS の中から小節数を拾う。[名前, 小節数, ...] の2つめ
        if t.starts_with("let SECTIONS") {
            in_sections = true;
            depth = 0;
        }
        if in_sections {
            depth += t.matches('[').count() as i32;
            depth -= t.matches(']').count() as i32;
            if t.starts_with('[') || t.starts_with("[\"") {
                if let Some(n) = second_number(t) {
                    bars += n;
                }
            }
            if depth <= 0 && !t.starts_with("let SECTIONS") {
                in_sections = false;
            }
        }
    }
    (title, bpm, key, if bars > 0 { Some(bars) } else { None })
}

fn decl_str(line: &str, name: &str) -> Option<String> {
    let rest = line.strip_prefix("let ")?.trim_start().strip_prefix(name)?;
    let rest = rest.trim_start().strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn decl_num(line: &str, name: &str) -> Option<f64> {
    let rest = line.strip_prefix("let ")?.trim_start().strip_prefix(name)?;
    let rest = rest.trim_start().strip_prefix('=')?.trim_start();
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))?;
    rest[..end].parse().ok()
}

/// `["イントロ", 8, ...]` の 8 を拾う。
fn second_number(line: &str) -> Option<u32> {
    let rest = line.strip_prefix('[')?;
    // 1つめは名前（文字列）。閉じる引用符の後ろから探す
    let rest = rest.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    let rest = rest[end + 1..].trim_start().strip_prefix(',')?.trim_start();
    let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    n.parse().ok()
}

/// 一覧を作る。
pub fn list(songs_dir: &Path, project_dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(songs_dir) else { return out };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rhai") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else {
            continue;
        };
        let modified = e.metadata().and_then(|m| m.modified()).ok();
        let store = tonescript_project::store::Store::new(project_dir, &name);
        let mut entry = Entry {
            title: String::new(),
            bpm: None,
            key: String::new(),
            bars: None,
            modified,
            error: None,
            has_project: store.main_path().exists(),
            pending: store.pending_autosave().is_some(),
            name,
            path: path.clone(),
        };
        match std::fs::read_to_string(&path) {
            Ok(src) => {
                let (t, b, k, n) = scan(&src);
                entry.title = t;
                entry.bpm = b;
                entry.key = k;
                entry.bars = n;
            }
            Err(e) => entry.error = Some(e.to_string()),
        }
        out.push(entry);
    }
    // 最近直したものを上に。作業の続きから始められる
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"// 説明
let TITLE = "テスト曲";
let BPM = 174;
let KEY = "F# minor";
let SECTIONS = [
    // [名前, 小節数, ...]
    ["イントロ", 8, "plain", "light", "main", 0.85],
    ["サビ", 12, "octa", "full", "high", 1.00, [7, 8]],
];
let VOICES = #{ lead: #{ ch: 0 } };
"#;

    fn dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("mp_br_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn scan_reads_the_basics_without_running_rhai() {
        let (title, bpm, key, bars) = scan(SRC);
        assert_eq!(title, "テスト曲");
        assert_eq!(bpm, Some(174));
        assert_eq!(key, "F# minor");
        assert_eq!(bars, Some(20), "8 + 12");
    }

    #[test]
    fn scan_survives_a_song_it_cannot_understand() {
        // 式で書かれていて読めなくても、落ちずに分かるぶんだけ返す
        let src = "let TITLE = \"x\";\nlet BPM = base * 2;\nlet SECTIONS = make();\n";
        let (title, bpm, _, bars) = scan(src);
        assert_eq!(title, "x");
        assert_eq!(bpm, None);
        assert_eq!(bars, None);
    }

    #[test]
    fn scan_ignores_comments_and_indented_lets() {
        let src = "// let TITLE = \"ちがう\";\nlet TITLE = \"ほんもの\";\n";
        let (title, ..) = scan(src);
        assert_eq!(title, "ほんもの");
    }

    #[test]
    fn empty_song_is_fine() {
        let (title, bpm, key, bars) = scan("");
        assert!(title.is_empty() && key.is_empty());
        assert_eq!((bpm, bars), (None, None));
    }

    #[test]
    fn list_finds_songs_and_sorts_by_recency() {
        let songs = dir("list");
        let proj = dir("list_p");
        std::fs::write(songs.join("a.rhai"), SRC).unwrap();
        // 少し待ってから2つめ。更新時刻を確実にずらす
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(songs.join("b.rhai"), SRC).unwrap();
        std::fs::write(songs.join("mem.txt"), "これは曲ではない").unwrap();

        let got = list(&songs, &proj);
        assert_eq!(got.len(), 2, ".rhai 以外を拾っている");
        assert_eq!(got[0].name, "b", "新しいほうが上に来ていない");
        assert_eq!(got[0].title, "テスト曲");
        assert_eq!(got[0].bars, Some(20));
        assert!(!got[0].has_project);
        assert!(!got[0].pending);
        assert!(!got[0].when().is_empty());

        std::fs::remove_dir_all(&songs).ok();
        std::fs::remove_dir_all(&proj).ok();
    }

    #[test]
    fn list_marks_songs_that_have_edits() {
        let songs = dir("marks");
        let proj = dir("marks_p");
        std::fs::write(songs.join("a.rhai"), SRC).unwrap();

        let store = tonescript_project::store::Store::new(&proj, "a");
        let mut p = tonescript_project::Project::new("a");
        p.add_note(
            "lead",
            tonescript_song::model::Note {
                pos: 0,
                len: 4,
                pitch: 60,
                vel: 100,
                mora: String::new(),
            },
        );
        store.save(&p).unwrap();

        let got = list(&songs, &proj);
        assert!(got[0].has_project, "編集があることを見ていない");
        assert!(!got[0].pending);

        // 閉じ損ねた状態を作る
        store.autosave(&p).unwrap();
        let got = list(&songs, &proj);
        assert!(got[0].pending, "閉じ損ねを見ていない");

        std::fs::remove_dir_all(&songs).ok();
        std::fs::remove_dir_all(&proj).ok();
    }

    #[test]
    fn missing_directory_is_empty_not_a_crash() {
        let got = list(Path::new("そんなフォルダはない"), Path::new("これも"));
        assert!(got.is_empty());
    }

    #[test]
    fn when_says_something_reasonable() {
        let mut e = Entry {
            name: "x".into(),
            path: PathBuf::new(),
            title: String::new(),
            bpm: None,
            key: String::new(),
            bars: None,
            modified: None,
            error: None,
            has_project: false,
            pending: false,
        };
        assert_eq!(e.when(), "");
        e.modified = Some(SystemTime::now());
        assert_eq!(e.when(), "たった今");
        e.modified = Some(SystemTime::now() - std::time::Duration::from_secs(120));
        assert_eq!(e.when(), "2分前");
        e.modified = Some(SystemTime::now() - std::time::Duration::from_secs(7200));
        assert_eq!(e.when(), "2時間前");
        e.modified = Some(SystemTime::now() - std::time::Duration::from_secs(86400 * 3));
        assert_eq!(e.when(), "3日前");
    }
}
