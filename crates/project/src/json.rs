//! 小さな JSON。書くのと読むのを、必要なぶんだけ。
//!
//! 中身は数・文字列・配列・表の入れ子しかない。serde を入れると
//! 依存がいくつも増えるので、ここは自分で書く。
//!
//! 読むほうは「自分が書いたものを読み返す」ためのもの。手で書き換えた
//! ファイルも読めるようにしてあるが、壊れていれば場所を添えて返す。

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// JSON の値。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    /// 鍵の順を決めておく。並びが毎回同じなら、差分が読める
    Obj(BTreeMap<String, Value>),
}

impl Value {
    pub fn obj() -> Self {
        Value::Obj(BTreeMap::new())
    }
    pub fn insert(&mut self, k: &str, v: Value) {
        if let Value::Obj(m) = self {
            m.insert(k.to_string(), v);
        }
    }
    pub fn get(&self, k: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.get(k),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_u32(&self) -> Option<u32> {
        self.as_f64().map(|v| v.max(0.0) as u32)
    }
    pub fn as_i32(&self) -> Option<i32> {
        self.as_f64().map(|v| v as i32)
    }
    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Obj(m) => Some(m),
            _ => None,
        }
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Num(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Num(v as f64)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Num(v as f64)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Num(v as f64)
    }
}
impl From<u8> for Value {
    fn from(v: u8) -> Self {
        Value::Num(v as f64)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::Arr(v.into_iter().map(|x| x.into()).collect())
    }
}

// ---------------------------------------------------------------- 書く

/// 読める形に整えて文字列にする。
pub fn to_string(v: &Value) -> String {
    let mut s = String::new();
    write(&mut s, v, 0);
    s.push('\n');
    s
}

fn write(out: &mut String, v: &Value, depth: usize) {
    let pad = "  ".repeat(depth + 1);
    let close = "  ".repeat(depth);
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => {
            if n.is_finite() {
                // 整数はそのまま整数で書く。1.0 より 1 のほうが読める
                if *n == n.trunc() && n.abs() < 1e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{}", (n * 1e6).round() / 1e6);
                }
            } else {
                out.push('0'); // NaN と無限は JSON に無い
            }
        }
        Value::Str(s) => escape(out, s),
        Value::Arr(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            // 数だけの短い配列は1行に収める。節の並びが読みやすくなる
            let flat = a.iter().all(|x| matches!(x, Value::Num(_) | Value::Bool(_)));
            if flat {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write(out, x, depth);
                }
                out.push(']');
                return;
            }
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(&pad);
                write(out, x, depth + 1);
                if i + 1 < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&close);
            out.push(']');
        }
        Value::Obj(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (k, x)) in m.iter().enumerate() {
                out.push_str(&pad);
                escape(out, k);
                out.push_str(": ");
                write(out, x, depth + 1);
                if i + 1 < m.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&close);
            out.push('}');
        }
    }
}

fn escape(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // 日本語はそのまま書く。\u エスケープにすると読めなくなる
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------- 読む

pub fn parse(src: &str) -> Result<Value, String> {
    let b: Vec<char> = src.chars().collect();
    let mut p = Parser { b, i: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i < p.b.len() {
        return Err(p.err("後ろに余計なものがあります"));
    }
    Ok(v)
}

struct Parser {
    b: Vec<char>,
    i: usize,
}

impl Parser {
    fn err(&self, msg: &str) -> String {
        // 何文字目かだけでなく、その前後も見せる
        let from = self.i.saturating_sub(20);
        let to = (self.i + 20).min(self.b.len());
        let near: String = self.b[from..to].iter().collect();
        format!("{msg}（{}文字目あたり: …{near}…）", self.i)
    }
    fn peek(&self) -> Option<char> {
        self.b.get(self.i).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn lit(&mut self, s: &str) -> bool {
        let n = s.chars().count();
        if self.i + n <= self.b.len() && self.b[self.i..self.i + n].iter().collect::<String>() == s {
            self.i += n;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.ws();
        match self.peek() {
            None => Err(self.err("値がありません")),
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(Value::Str(self.string()?)),
            Some('t') if self.lit("true") => Ok(Value::Bool(true)),
            Some('f') if self.lit("false") => Ok(Value::Bool(false)),
            Some('n') if self.lit("null") => Ok(Value::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            _ => Err(self.err("読めない値です")),
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.i += 1; // {
        let mut m = BTreeMap::new();
        self.ws();
        if self.eat('}') {
            return Ok(Value::Obj(m));
        }
        loop {
            self.ws();
            let k = self.string()?;
            self.ws();
            if !self.eat(':') {
                return Err(self.err("鍵のあとに : がありません"));
            }
            let v = self.value()?;
            m.insert(k, v);
            self.ws();
            if self.eat(',') {
                continue;
            }
            if self.eat('}') {
                return Ok(Value::Obj(m));
            }
            return Err(self.err("表が閉じていません"));
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.i += 1; // [
        let mut a = Vec::new();
        self.ws();
        if self.eat(']') {
            return Ok(Value::Arr(a));
        }
        loop {
            a.push(self.value()?);
            self.ws();
            if self.eat(',') {
                continue;
            }
            if self.eat(']') {
                return Ok(Value::Arr(a));
            }
            return Err(self.err("配列が閉じていません"));
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat('"') {
            return Err(self.err("文字列の始まりが \" ではありません"));
        }
        let mut s = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("文字列が閉じていません"));
            };
            self.i += 1;
            match c {
                '"' => return Ok(s),
                '\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("\\ の後が切れています"));
                    };
                    self.i += 1;
                    match e {
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        '/' => s.push('/'),
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        'b' => s.push('\u{8}'),
                        'f' => s.push('\u{c}'),
                        'u' => {
                            let h: String = self.b.iter().skip(self.i).take(4).collect();
                            if h.len() < 4 {
                                return Err(self.err("\\u の後が足りません"));
                            }
                            self.i += 4;
                            let n = u32::from_str_radix(&h, 16)
                                .map_err(|_| self.err("\\u の後が16進数ではありません"))?;
                            s.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.err("知らないエスケープです")),
                    }
                }
                c => s.push(c),
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let from = self.i;
        if self.peek() == Some('-') {
            self.i += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-')
        {
            self.i += 1;
        }
        let s: String = self.b[from..self.i].iter().collect();
        s.parse::<f64>().map(Value::Num).map_err(|_| self.err("数として読めません"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back() {
        let mut v = Value::obj();
        v.insert("名前", "テスト曲".into());
        v.insert("bpm", 128u32.into());
        v.insert("gain", 0.75f32.into());
        v.insert("on", true.into());
        v.insert("points", Value::Arr(vec![0u32.into(), 1.5f32.into()]));
        let mut inner = Value::obj();
        inner.insert("x", 1i32.into());
        v.insert("nest", inner);

        let s = to_string(&v);
        let back = parse(&s).expect("読めるはず");
        assert_eq!(back, v);
    }

    #[test]
    fn japanese_is_written_raw() {
        let mut v = Value::obj();
        v.insert("label", "主旋律".into());
        let s = to_string(&v);
        assert!(s.contains("主旋律"), "\\u に化けている: {s}");
    }

    #[test]
    fn integers_stay_integers() {
        let v = Value::Arr(vec![1.0f64.into(), 2.5f64.into()]);
        let s = to_string(&v);
        assert!(s.contains("[1, 2.5]"), "{s}");
    }

    #[test]
    fn escapes_round_trip() {
        let mut v = Value::obj();
        v.insert("s", "改行\nタブ\t引用\"逆\\".into());
        let back = parse(&to_string(&v)).unwrap();
        assert_eq!(back.get("s").unwrap().as_str().unwrap(), "改行\nタブ\t引用\"逆\\");
    }

    #[test]
    fn empty_containers() {
        assert_eq!(parse("{}").unwrap(), Value::obj());
        assert_eq!(parse("[]").unwrap(), Value::Arr(vec![]));
        assert_eq!(to_string(&Value::obj()).trim(), "{}");
    }

    #[test]
    fn broken_json_says_where() {
        for bad in ["{", "[1,", r#"{"a" 1}"#, r#"{"a": }"#, "", "{} ゴミ"] {
            let e = parse(bad).unwrap_err();
            assert!(!e.is_empty(), "エラーが空: {bad}");
            assert!(e.contains("文字目"), "場所が無い: {e}");
        }
    }

    #[test]
    fn unicode_escape_is_read() {
        let v = parse(r#"{"a": "あ"}"#).unwrap();
        assert_eq!(v.get("a").unwrap().as_str().unwrap(), "あ");
    }

    #[test]
    fn nan_does_not_break_the_file() {
        // NaN は JSON に無い。0 として書いて、読み返せること
        let v = Value::Num(f64::NAN);
        let s = to_string(&v);
        assert!(parse(&s).is_ok(), "{s}");
    }
}
