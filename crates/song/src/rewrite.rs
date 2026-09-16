//! 曲ファイルを、壊さずに書き換える。
//!
//! 画面から BPM を直したら、曲ファイルの側も直す。片方だけ変えて
//! 「ファイルには 128 と書いてあるのに 140 で鳴る」が起きるのが一番よくない。
//! 曲ファイルを正本にして、そこを書き換える。
//!
//! 気をつけていること
//! ------------------
//! - 直すのは**行頭の `let 名前 = ...;` だけ**。コメントの中にも同じ語が
//!   出てくるし、字下げされた所は別の意味を持つ
//! - 触った行以外は1バイトも変えない。手で書いたコメントも式も残す
//! - 書き換えたあと、必ず読み直して通ることを確かめる。
//!   通らなければ書かない
//!
//! ここで扱うのは「1行で書かれた値」と `SECTIONS` だけ。旋律や和音のような
//! 作りものは画面から触らない。あれは手で書くもの。

use crate::model::{Meter, Section};
use crate::{load_str, Song};

/// 書き換えられなかった理由。
#[derive(Debug, PartialEq)]
pub enum Error {
    /// その名前の行が見つからない
    NotFound(String),
    /// 同じ名前の行が複数ある。どれを直すべきか決められない
    Ambiguous(String),
    /// 書き換えたら読めなくなった
    Broken(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound(n) => write!(f, "{n} を書いている行が見つかりません"),
            Error::Ambiguous(n) => write!(f, "{n} を書いている行が複数あります"),
            Error::Broken(m) => write!(f, "書き換えたら読めなくなりました: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type R<T> = Result<T, Error>;

/// 行頭の `let 名前` で始まる行か。
///
/// 字下げされた行は見ない。中括弧の中の `let` は別のものなので。
fn is_decl(line: &str, name: &str) -> bool {
    let Some(rest) = line.strip_prefix("let ") else { return false };
    let rest = rest.trim_start();
    let Some(after) = rest.strip_prefix(name) else { return false };
    // 名前の直後が区切りであること（BPM と BPM2 を取り違えない）
    after.trim_start().starts_with('=')
}

/// その名前を書いている行を1つだけ見つける。
fn find_line(text: &str, name: &str) -> R<usize> {
    let hits: Vec<usize> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| is_decl(l, name))
        .map(|(i, _)| i)
        .collect();
    match hits.len() {
        0 => Err(Error::NotFound(name.into())),
        1 => Ok(hits[0]),
        _ => Err(Error::Ambiguous(name.into())),
    }
}

/// 行を差し替える。改行の形は元のまま残す。
fn replace_line(text: &str, at: usize, new: &str) -> String {
    let mut out = String::with_capacity(text.len() + new.len());
    for (i, line) in text.lines().enumerate() {
        out.push_str(if i == at { new } else { line });
        out.push('\n');
    }
    out
}

/// Rhai の文字列に入れて安全な形にする。
fn q(s: &str) -> String {
    s.replace('\\', "").replace('"', "").replace(['\n', '\r'], " ")
}

/// 数を書き換える。整数なら整数のまま書く。
pub fn set_number(text: &str, name: &str, value: f64) -> R<String> {
    let at = find_line(text, name)?;
    let v = if value == value.trunc() && value.abs() < 1e9 {
        format!("{}", value as i64)
    } else {
        // 小数は 3 桁まで。曲ファイルに 0.7000000001 と書かれても読みにくい
        format!("{:.3}", value).trim_end_matches('0').trim_end_matches('.').to_string()
    };
    Ok(replace_line(text, at, &format!("let {name} = {v};")))
}

/// 文字列を書き換える。
pub fn set_text(text: &str, name: &str, value: &str) -> R<String> {
    let at = find_line(text, name)?;
    Ok(replace_line(text, at, &format!("let {name} = \"{}\";", q(value))))
}

/// `let SECTIONS = [ ... ];` をまるごと書き換える。
///
/// 中身が複数行にわたるので、`[` と `]` の釣り合いで終わりを探す。
pub fn set_sections(text: &str, sections: &[Section]) -> R<String> {
    let start = find_line(text, "SECTIONS")?;
    let lines: Vec<&str> = text.lines().collect();

    // 終わりを探す。文字列の中の括弧は数えない
    let mut depth = 0i32;
    let mut end = None;
    let mut started = false;
    for (i, line) in lines.iter().enumerate().skip(start) {
        let mut in_str = false;
        let mut prev_slash = false;
        for c in line.chars() {
            if in_str {
                if c == '"' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '/' if prev_slash => break, // ここから先はコメント
                '[' => {
                    depth += 1;
                    started = true;
                }
                ']' => depth -= 1,
                _ => {}
            }
            prev_slash = c == '/';
        }
        if started && depth <= 0 {
            end = Some(i);
            break;
        }
    }
    let end = end.ok_or_else(|| Error::Broken("SECTIONS が閉じていません".into()))?;

    let mut body = String::from("let SECTIONS = [\n");
    body.push_str("    // [名前, 小節数, ベース型, ドラムキット, リフ型, 音量, 拍子]\n");
    for s in sections {
        let meter = if s.meter == Meter::default() {
            String::new()
        } else {
            format!(", [{}, {}]", s.meter.num, s.meter.den)
        };
        body.push_str(&format!(
            "    [\"{}\", {}, \"{}\", \"{}\", \"{}\", {:.2}{}],\n",
            q(&s.name),
            s.bars,
            q(&s.bass),
            q(&s.kit),
            q(&s.arp),
            s.gain,
            meter
        ));
    }
    body.push_str("];");

    let mut out = String::with_capacity(text.len() + body.len());
    for (i, line) in lines.iter().enumerate() {
        if i < start || i > end {
            out.push_str(line);
            out.push('\n');
        } else if i == start {
            out.push_str(&body);
            out.push('\n');
        }
    }
    Ok(out)
}

/// 書き換えたものが読めるか確かめる。通らなければ元のまま返らない。
pub fn verify(text: &str) -> R<Song> {
    load_str(text).map_err(|e| Error::Broken(e.to_string()))
}

/// 画面から直せる値をまとめて反映する。
///
/// ひとつでも失敗したら、何も書き換えずに理由を返す。
/// 半分だけ反映された曲ファイルを残さない。
pub struct Edit {
    pub title: Option<String>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub master_lufs: Option<f32>,
    pub master_gain: Option<f32>,
    pub sections: Option<Vec<Section>>,
}

impl Edit {
    pub fn apply(&self, text: &str) -> R<String> {
        let mut out = text.to_string();
        if let Some(v) = &self.title {
            out = set_text(&out, "TITLE", v)?;
        }
        if let Some(v) = self.bpm {
            out = set_number(&out, "BPM", v as f64)?;
            // テンポの節目が「頭に1つだけ」の形なら、そこも合わせる。
            // 合わせないと、BPM を直したのに鳴る速さが変わらない
            if let Ok(at) = find_line(&out, "TEMPO_MAP") {
                let line = out.lines().nth(at).unwrap_or("");
                if is_simple_tempo_map(line) {
                    let v = v as i64;
                    out = replace_line(&out, at, &format!("let TEMPO_MAP = #{{ \"1\": {v} }};"));
                }
            }
        }
        if let Some(v) = &self.key {
            out = set_text(&out, "KEY", v)?;
        }
        if let Some(v) = self.master_lufs {
            out = set_number(&out, "MASTER_LUFS", v as f64)?;
        }
        if let Some(v) = self.master_gain {
            out = set_number(&out, "MASTER_GAIN", v as f64)?;
        }
        if let Some(v) = &self.sections {
            out = set_sections(&out, v)?;
        }
        verify(&out)?;
        Ok(out)
    }
}

/// `let TEMPO_MAP = #{ "1": 128 };` の形か。
///
/// 途中でテンポが変わる曲は触らない。手で書いた節目を消してしまう。
fn is_simple_tempo_map(line: &str) -> bool {
    let body = match line.split_once('{') {
        Some((_, b)) => b,
        None => return false,
    };
    // 節目が1つだけ（カンマが無い）で、鍵が "1"
    !body.contains(',') && body.contains("\"1\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"// これは曲の説明。BPM のことも書いてある。
let TITLE = "もとの名前";
let BPM = 128;              // ここのコメントも残ってほしい
let KEY = "A minor";
let TEMPO_MAP = #{ "1": 128 };

let SECTIONS = [
    // [名前, 小節数, ...]
    ["イントロ", 8, "plain", "light", "main", 0.85],
    ["サビ",     8, "octa",  "full",  "high", 1.00],
];

let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
let MASTER_GAIN = 1.0;
let MASTER_LUFS = -9;
"#;

    fn sec(name: &str, bars: u32, meter: Meter) -> Section {
        Section {
            name: name.into(),
            bars,
            bass: "plain".into(),
            kit: "light".into(),
            arp: "main".into(),
            gain: 1.0,
            meter,
        }
    }

    #[test]
    fn number_is_replaced_and_nothing_else_moves() {
        let out = set_number(SRC, "BPM", 174.0).unwrap();
        assert!(out.contains("let BPM = 174;"));
        // 他の行はそのまま
        assert!(out.contains("let TITLE = \"もとの名前\";"));
        assert!(out.contains("// これは曲の説明。BPM のことも書いてある。"));
        assert_eq!(SRC.lines().count(), out.lines().count(), "行が増減した");
    }

    #[test]
    fn comments_mentioning_the_name_are_not_touched() {
        // 1行目のコメントに BPM と書いてあるが、そこは直らない
        let out = set_number(SRC, "BPM", 90.0).unwrap();
        assert!(out.lines().next().unwrap().contains("BPM のことも書いてある"));
    }

    #[test]
    fn text_is_quoted_safely() {
        let out = set_text(SRC, "TITLE", "変な\"名前\\です").unwrap();
        // そのまま書くと Rhai の文字列が壊れる
        assert!(verify(&out).is_ok(), "壊れた: {out}");
    }

    #[test]
    fn missing_name_is_reported() {
        assert_eq!(
            set_number(SRC, "ナイヨ", 1.0).unwrap_err(),
            Error::NotFound("ナイヨ".into())
        );
    }

    #[test]
    fn duplicate_name_is_refused() {
        let src = format!("{SRC}let BPM = 200;\n");
        assert_eq!(set_number(&src, "BPM", 1.0).unwrap_err(), Error::Ambiguous("BPM".into()));
    }

    #[test]
    fn similar_names_are_not_confused() {
        let src = "let BPM = 128;\nlet BPM2 = 999;\n";
        let out = set_number(src, "BPM", 140.0).unwrap();
        assert!(out.contains("let BPM = 140;"));
        assert!(out.contains("let BPM2 = 999;"), "似た名前まで直した");
    }

    #[test]
    fn indented_lets_are_ignored() {
        let src = "let BPM = 128;\nfn f() {\n    let BPM = 1;\n}\n";
        let out = set_number(src, "BPM", 140.0).unwrap();
        assert!(out.contains("let BPM = 140;"));
        assert!(out.contains("    let BPM = 1;"), "中の let まで直した");
    }

    #[test]
    fn sections_are_rewritten_whole() {
        let out = set_sections(
            SRC,
            &[
                sec("Aメロ", 4, Meter::default()),
                sec("Bメロ", 6, Meter::new(7, 8)),
                sec("サビ", 8, Meter::default()),
            ],
        )
        .unwrap();
        let s = verify(&out).unwrap();
        assert_eq!(s.sections.len(), 3);
        assert_eq!(s.sections[0].name, "Aメロ");
        assert_eq!(s.sections[1].bars, 6);
        assert_eq!(s.sections[1].meter, Meter::new(7, 8));
        assert_eq!(s.bars(), 18);
        // 前後の行は残っている
        assert!(out.contains("let TITLE = \"もとの名前\";"));
        assert!(out.contains("let VOICES ="));
    }

    #[test]
    fn edit_applies_everything_at_once() {
        let e = Edit {
            title: Some("新しい名前".into()),
            bpm: Some(174.0),
            key: Some("F# minor".into()),
            master_lufs: Some(-12.0),
            master_gain: Some(0.9),
            sections: Some(vec![sec("A", 4, Meter::default())]),
        };
        let out = e.apply(SRC).unwrap();
        let s = verify(&out).unwrap();
        assert_eq!(s.title, "新しい名前");
        assert_eq!(s.bpm, 174.0);
        assert_eq!(s.key, "F# minor");
        assert_eq!(s.master_lufs, -12.0);
        assert!((s.master_gain - 0.9).abs() < 1e-6);
        assert_eq!(s.bars(), 4);
    }

    #[test]
    fn changing_bpm_also_moves_a_simple_tempo_map() {
        // 直したのに鳴る速さが変わらない、が起きないこと
        let e = Edit {
            title: None,
            bpm: Some(90.0),
            key: None,
            master_lufs: None,
            master_gain: None,
            sections: None,
        };
        let out = e.apply(SRC).unwrap();
        assert!(out.contains("let TEMPO_MAP = #{ \"1\": 90 };"), "{out}");
        let s = verify(&out).unwrap();
        assert_eq!(s.tempo_map[&1], 90.0);
    }

    #[test]
    fn a_hand_written_tempo_map_is_left_alone() {
        // 途中でテンポが変わる曲。手で書いた節目を消さないこと
        let src = SRC.replace(
            r#"let TEMPO_MAP = #{ "1": 128 };"#,
            r#"let TEMPO_MAP = #{ "1": 128, "17": 174 };"#,
        );
        let e = Edit {
            title: None,
            bpm: Some(90.0),
            key: None,
            master_lufs: None,
            master_gain: None,
            sections: None,
        };
        let out = e.apply(&src).unwrap();
        assert!(out.contains(r#""17": 174"#), "手で書いた節目が消えた");
        assert!(out.contains("let BPM = 90;"));
    }

    #[test]
    fn nothing_is_written_when_the_result_would_not_load() {
        // 小節数 0 は読み込みで弾かれる。書き換え自体が失敗すること
        let e = Edit {
            title: None,
            bpm: None,
            key: None,
            master_lufs: None,
            master_gain: None,
            sections: Some(vec![sec("A", 0, Meter::default())]),
        };
        let err = e.apply(SRC).unwrap_err();
        assert!(matches!(err, Error::Broken(_)), "{err}");
    }

    #[test]
    fn empty_edit_changes_nothing_meaningful() {
        let e = Edit {
            title: None,
            bpm: None,
            key: None,
            master_lufs: None,
            master_gain: None,
            sections: None,
        };
        let out = e.apply(SRC).unwrap();
        assert_eq!(out.trim_end(), SRC.trim_end(), "触っていないのに変わった");
    }

    #[test]
    fn decimals_are_written_readably() {
        let out = set_number(SRC, "MASTER_GAIN", 0.85).unwrap();
        assert!(out.contains("let MASTER_GAIN = 0.85;"), "{out}");
        let out = set_number(SRC, "MASTER_GAIN", 1.0).unwrap();
        assert!(out.contains("let MASTER_GAIN = 1;"), "{out}");
        // 0.7000000001 のような値を書かない
        let out = set_number(SRC, "MASTER_GAIN", 0.700000001).unwrap();
        assert!(out.contains("let MASTER_GAIN = 0.7;"), "{out}");
    }
}
