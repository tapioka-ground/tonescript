//! Rhai で書かれた曲ファイルを読む。
//!
//! 曲ファイルは「値をいくつも定義したスクリプト」。最後まで実行してから、
//! グローバルに残った変数を拾って `Song` へ詰める。
//!
//! 読み方の決まり
//!   - 必須の値が無ければエラーにする（黙って既定値で走らせない）
//!   - 型が違えば「どの値がどう違うか」を日本語で返す
//!   - 曲ファイルは信用しない。実行の上限を入れて、無限ループで
//!     固まらないようにしてある

use crate::model::*;
use tonescript_dsp::recipe;
use rhai::{Array, Dynamic, Engine, Map, Scope};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug)]
pub enum LoadError {
    /// ファイルが読めない
    Io(String),
    /// Rhai の構文エラーや実行時エラー
    Script(String),
    /// 値が足りない、または型が違う
    Shape(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(m) => write!(f, "曲ファイルを読めません: {m}"),
            LoadError::Script(m) => write!(f, "曲ファイルの中でエラー: {m}"),
            LoadError::Shape(m) => write!(f, "曲ファイルの書き方: {m}"),
        }
    }
}

impl std::error::Error for LoadError {}

type R<T> = Result<T, LoadError>;

fn shape<T>(msg: impl Into<String>) -> R<T> {
    Err(LoadError::Shape(msg.into()))
}

// ---------------------------------------------------------------- 取り出し

fn num(v: &Dynamic, what: &str) -> R<f32> {
    if let Ok(i) = v.as_int() {
        return Ok(i as f32);
    }
    if let Ok(f) = v.as_float() {
        return Ok(f as f32);
    }
    shape(format!("{what} は数でなければいけません（今は {}）", v.type_name()))
}

fn int(v: &Dynamic, what: &str) -> R<i64> {
    if let Ok(i) = v.as_int() {
        return Ok(i);
    }
    if let Ok(f) = v.as_float() {
        return Ok(f as i64);
    }
    shape(format!("{what} は整数でなければいけません（今は {}）", v.type_name()))
}

fn text(v: &Dynamic, what: &str) -> R<String> {
    v.clone()
        .into_string()
        .map_err(|t| LoadError::Shape(format!("{what} は文字列でなければいけません（今は {t}）")))
}

fn arr(v: &Dynamic, what: &str) -> R<Array> {
    v.clone()
        .try_cast::<Array>()
        .ok_or_else(|| LoadError::Shape(format!("{what} は配列でなければいけません")))
}

fn map(v: &Dynamic, what: &str) -> R<Map> {
    v.clone()
        .try_cast::<Map>()
        .ok_or_else(|| LoadError::Shape(format!("{what} は #{{...}} の表でなければいけません")))
}

/// スコープから値を取る。無ければ None。
fn get(scope: &Scope, name: &str) -> Option<Dynamic> {
    scope.get_value::<Dynamic>(name)
}

fn need(scope: &Scope, name: &str) -> R<Dynamic> {
    get(scope, name).ok_or_else(|| LoadError::Shape(format!("{name} が定義されていません")))
}

fn num_or(scope: &Scope, name: &str, dflt: f32) -> R<f32> {
    match get(scope, name) {
        Some(v) => num(&v, name),
        None => Ok(dflt),
    }
}

fn text_or(scope: &Scope, name: &str, dflt: &str) -> R<String> {
    match get(scope, name) {
        Some(v) => text(&v, name),
        None => Ok(dflt.to_string()),
    }
}

/// 表の要素を取る。
fn field<'a>(m: &'a Map, key: &str) -> Option<&'a Dynamic> {
    m.get(key)
}

fn need_field<'a>(m: &'a Map, key: &str, what: &str) -> R<&'a Dynamic> {
    field(m, key).ok_or_else(|| LoadError::Shape(format!("{what} に {key} がありません")))
}

/// 「小節番号 -> 何か」の表。Rhai の表は鍵が文字列なので、数として読む。
fn bar_map<T, F>(v: &Dynamic, what: &str, mut f: F) -> R<HashMap<u32, T>>
where
    F: FnMut(&Dynamic) -> R<T>,
{
    let m = map(v, what)?;
    let mut out = HashMap::new();
    for (k, val) in m.iter() {
        let bar: u32 = k
            .parse()
            .map_err(|_| LoadError::Shape(format!("{what} の鍵 \"{k}\" が小節番号ではありません")))?;
        out.insert(bar, f(val)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------- 本体

/// 曲ファイルを読む。
pub fn load_file(path: &std::path::Path) -> R<Song> {
    let src = std::fs::read_to_string(path).map_err(|e| LoadError::Io(format!("{path:?}: {e}")))?;
    load_str(&src).map_err(|e| match e {
        // どのファイルで落ちたか分かるようにする
        LoadError::Script(m) => LoadError::Script(format!("{}: {m}", path.display())),
        LoadError::Shape(m) => LoadError::Shape(format!("{}: {m}", path.display())),
        other => other,
    })
}

/// 曲ファイルの中身を読む。
pub fn load_str(src: &str) -> R<Song> {
    let mut engine = Engine::new();
    // 曲ファイルは信用しない。無限ループや深すぎる入れ子で固まらせない。
    engine.set_max_operations(50_000_000);
    engine.set_max_expr_depths(128, 128);
    engine.set_max_string_size(100_000);
    engine.set_max_array_size(1_000_000);

    register_helpers(&mut engine);

    let mut scope = Scope::new();
    engine
        .run_with_scope(&mut scope, src)
        .map_err(|e| LoadError::Script(e.to_string()))?;

    build(&scope)
}

/// 曲ファイルから呼べる小道具。
///
/// Python 側で `_bar()` や内包表記がやっていたことを、ここで用意する。
fn register_helpers(engine: &mut Engine) {
    // 音名 -> MIDI ノート番号。曲ファイルで音高を数で書かなくて済む。
    engine.register_fn("note", |name: &str| -> i64 {
        note_number(name).map(|v| v as i64).unwrap_or(-1)
    });

    // 1小節ぶんの旋律。長さの合計が 16 でなければその場で止める。
    // Python 版の `_bar()` と同じ役目で、書き間違いを曲の読み込み時に見つける。
    engine.register_fn("bar", |items: Array| -> Result<Array, Box<rhai::EvalAltResult>> {
        let mut total = 0i64;
        let mut out = Array::new();
        for it in items.iter() {
            let pair = it
                .clone()
                .try_cast::<Array>()
                .ok_or_else(|| "bar() の中身は [長さ, 音名] の配列です".to_string())?;
            if pair.len() < 2 {
                return Err("bar() の中身は [長さ, 音名] の2つです".into());
            }
            let len = pair[0].as_int().map_err(|_| "長さは整数です".to_string())?;
            total += len;
            out.push(Dynamic::from(pair));
        }
        // bar() は拍子を知らない（曲のどの小節に置かれるかは、この時点では
        // まだ決まっていない）。4/4 を前提にした警告だけ出して、
        // 本当の検算は曲を組み立てるときに拍子ごとに行う。
        if total != STEPS_PER_BAR as i64 && total % 2 != 0 {
            // 奇数の合計はまず書き間違い。拍子が変でも偶数にはなる。
            return Err(format!("1小節の合計が {total} で奇数です。書き間違いでは").into());
        }
        Ok(out)
    });

    // range(start, stop, step)。Python の range と同じ。
    engine.register_fn("steps", |start: i64, stop: i64, step: i64| -> Array {
        let mut out = Array::new();
        if step == 0 {
            return out;
        }
        let mut i = start;
        while (step > 0 && i < stop) || (step < 0 && i > stop) {
            out.push(Dynamic::from(i));
            i += step;
        }
        out
    });
}

fn build(scope: &Scope) -> R<Song> {
    let mut s = Song {
        title: text_or(scope, "TITLE", "untitled")?,
        bpm: num(&need(scope, "BPM")?, "BPM")?,
        key: text_or(scope, "KEY", "")?,
        tempo_curve: text_or(scope, "TEMPO_CURVE", "smooth")?,
        master_gain: num_or(scope, "MASTER_GAIN", 1.0)?,
        master_lufs: num_or(scope, "MASTER_LUFS", -9.0)?,
        premix_lufs: num_or(scope, "PREMIX_LUFS", -20.0)?,
        scale_root: num_or(scope, "SCALE_ROOT", 0.0)? as i32,
        ..Default::default()
    };

    if !(20.0..=400.0).contains(&s.bpm) {
        return shape(format!("BPM が {} です。20〜400 の間にしてください", s.bpm));
    }

    // --- テンポ
    s.tempo_map = match get(scope, "TEMPO_MAP") {
        Some(v) => bar_map(&v, "TEMPO_MAP", |x| num(x, "TEMPO_MAP の値"))?,
        None => HashMap::from([(1, s.bpm)]),
    };

    // --- 構成
    let secs = arr(&need(scope, "SECTIONS")?, "SECTIONS")?;
    if secs.is_empty() {
        return shape("SECTIONS が空です。セクションを1つ以上書いてください");
    }
    for (i, v) in secs.iter().enumerate() {
        let a = arr(v, &format!("SECTIONS[{i}]"))?;
        if a.len() < 6 {
            return shape(format!(
                "SECTIONS[{i}] は [名前, 小節数, ベース型, キット, リフ型, 音量] の6つです
                 （7つめに拍子 [分子, 分母] を足せます。省くと 4/4）"
            ));
        }
        // 7つめは拍子。省いたら 4/4。
        let meter = match a.get(6) {
            None => Meter::default(),
            Some(v) => {
                let m = arr(v, &format!("SECTIONS[{i}] の拍子"))?;
                if m.len() < 2 {
                    return shape(format!("SECTIONS[{i}] の拍子は [分子, 分母] の2つです"));
                }
                let num = int(&m[0], "拍子の分子")?;
                let den = int(&m[1], "拍子の分母")?;
                if !(1..=32).contains(&num) {
                    return shape(format!("SECTIONS[{i}] の拍子の分子が {num} です。1〜32 に"));
                }
                if ![1i64, 2, 4, 8, 16].contains(&den) {
                    return shape(format!(
                        "SECTIONS[{i}] の拍子の分母が {den} です。1 / 2 / 4 / 8 / 16 のどれかに"
                    ));
                }
                Meter::new(num as u32, den as u32)
            }
        };
        let bars = int(&a[1], &format!("SECTIONS[{i}] の小節数"))?;
        if bars <= 0 {
            return shape(format!("SECTIONS[{i}] の小節数が {bars} です"));
        }
        s.sections.push(Section {
            name: text(&a[0], "セクション名")?,
            bars: bars as u32,
            bass: text(&a[2], "ベース型")?,
            kit: text(&a[3], "キット名")?,
            arp: text(&a[4], "リフ型")?,
            gain: num(&a[5], "セクションの音量")?,
            meter,
        });
    }
    let total_bars = s.bars();

    // --- 和音
    if let Some(v) = get(scope, "CHORDS") {
        s.chords = bar_map(&v, "CHORDS", |x| {
            let a = arr(x, "和音")?;
            if a.len() < 3 {
                return shape("和音は [表示名, [構成音...], ベース音] の3つです");
            }
            let tones = arr(&a[1], "和音の構成音")?;
            let mut out = Vec::new();
            for t in tones.iter() {
                let name = text(t, "構成音")?;
                out.push(note_number(&name).ok_or_else(|| {
                    LoadError::Shape(format!("音名として読めません: {name}"))
                })?);
            }
            let bname = text(&a[2], "ベース音")?;
            Ok(Chord {
                name: text(&a[0], "和音の表示名")?,
                tones: out,
                bass: note_number(&bname)
                    .ok_or_else(|| LoadError::Shape(format!("音名として読めません: {bname}")))?,
            })
        })?;
    }

    // --- 旋律
    if let Some(v) = get(scope, "MELODY") {
        s.melody = bar_map(&v, "MELODY", |x| {
            let a = arr(x, "小節の旋律")?;
            let mut out = Vec::new();
            for it in a.iter() {
                let p = arr(it, "音符")?;
                if p.len() < 2 {
                    return shape("音符は [長さ, 音名] の2つです");
                }
                let len = int(&p[0], "音符の長さ")? as u32;
                let name = text(&p[1], "音符の音名")?;
                let mora = if p.len() > 2 { Some(text(&p[2], "歌詞")?) } else { None };
                out.push((mora, len, name));
            }
            Ok(out)
        })?;
        // 合計が1小節ぶんかを読み込み時に検算する。
        // 何目盛りが1小節かは拍子で変わるので、小節ごとに見る。
        for (bar, notes) in &s.melody {
            if *bar == 0 || *bar > total_bars {
                return shape(format!(
                    "{bar}小節目に旋律がありますが、曲は {total_bars} 小節しかありません"
                ));
            }
            let want = s.bar_steps(*bar);
            let total: u32 = notes.iter().map(|(_, l, _)| l).sum();
            if total != want {
                let m = s.meter_at(*bar);
                return shape(format!(
                    "{bar}小節目の旋律の合計が {total}/{want} です（拍子 {m}）"
                ));
            }
        }
    }

    // --- 編成
    if let Some(v) = get(scope, "ARRANGE") {
        s.arrange = bar_map(&v, "ARRANGE", |x| {
            let a = arr(x, "ARRANGE の中身")?;
            a.iter().map(|p| text(p, "パート名")).collect()
        })?;
    }
    if let Some(v) = get(scope, "TRANSPOSE") {
        s.transpose = bar_map(&v, "TRANSPOSE", |x| Ok(int(x, "TRANSPOSE の値")? as i32))?;
    }
    if let Some(v) = get(scope, "BAR_ACCENT") {
        s.bar_accent = bar_map(&v, "BAR_ACCENT", |x| num(x, "BAR_ACCENT の値"))?;
    }

    // --- 伴奏の型
    if let Some(v) = get(scope, "BASS_PATTERNS") {
        for (k, val) in map(&v, "BASS_PATTERNS")?.iter() {
            let a = arr(val, "ベース型")?;
            let mut out = Vec::new();
            for it in a.iter() {
                let p = arr(it, "ベースの打点")?;
                if p.len() < 3 {
                    return shape("ベースの打点は [位置, 長さ, 半音差] の3つです");
                }
                out.push((
                    int(&p[0], "位置")? as u32,
                    int(&p[1], "長さ")? as u32,
                    int(&p[2], "半音差")? as i32,
                ));
            }
            s.bass_patterns.insert(k.to_string(), out);
        }
    }
    if let Some(v) = get(scope, "ARP_PATTERNS") {
        for (k, val) in map(&v, "ARP_PATTERNS")?.iter() {
            let a = arr(val, "リフ型")?;
            let mut out = Vec::new();
            for it in a.iter() {
                let p = arr(it, "リフの打点")?;
                if p.len() < 4 {
                    return shape("リフの打点は [位置, 長さ, 何番目の音, オクターブ] の4つです");
                }
                out.push((
                    int(&p[0], "位置")? as u32,
                    int(&p[1], "長さ")? as u32,
                    int(&p[2], "何番目の音")?.max(0) as usize,
                    int(&p[3], "オクターブ")? as i32,
                ));
            }
            s.arp_patterns.insert(k.to_string(), out);
        }
    }
    if let Some(v) = get(scope, "CHORD_PATTERN") {
        for it in arr(&v, "CHORD_PATTERN")?.iter() {
            let p = arr(it, "コードの打点")?;
            if p.len() < 3 {
                return shape("コードの打点は [位置, 長さ, 強さ] の3つです");
            }
            s.chord_pattern.push((
                int(&p[0], "位置")? as u32,
                int(&p[1], "長さ")? as u32,
                int(&p[2], "強さ")?.clamp(0, 127) as u8,
            ));
        }
    }

    // --- ドラム
    if let Some(v) = get(scope, "DRUM_KITS") {
        for (kit_name, kit_val) in map(&v, "DRUM_KITS")?.iter() {
            let mut kit = Kit::new();
            for (part, pv) in map(kit_val, "ドラムキット")?.iter() {
                let a = arr(pv, "ドラムのパート")?;
                if a.len() < 2 {
                    return shape("ドラムのパートは [MIDIノート番号, [打点...]] の2つです");
                }
                let note = int(&a[0], "MIDIノート番号")?.clamp(0, 127) as u8;
                let mut hits = Vec::new();
                for h in arr(&a[1], "打点")?.iter() {
                    let p = arr(h, "打点")?;
                    if p.len() < 2 {
                        return shape("打点は [位置, 強さ] の2つです");
                    }
                    hits.push((
                        int(&p[0], "位置")? as u32,
                        int(&p[1], "強さ")?.clamp(0, 127) as u8,
                    ));
                }
                kit.insert(part.to_string(), (note, hits));
            }
            s.drum_kits.insert(kit_name.to_string(), kit);
        }
    }
    if let Some(v) = get(scope, "EXTRA_HITS") {
        for (name, hv) in map(&v, "EXTRA_HITS")?.iter() {
            let a = arr(hv, "一発もの")?;
            if a.len() < 2 {
                return shape("一発ものは [MIDIノート番号, [打点...]] の2つです");
            }
            let note = int(&a[0], "MIDIノート番号")?.clamp(0, 127) as u8;
            let mut hits = Vec::new();
            for h in arr(&a[1], "打点")?.iter() {
                let p = arr(h, "打点")?;
                hits.push((int(&p[0], "位置")? as u32, int(&p[1], "強さ")?.clamp(0, 127) as u8));
            }
            s.extra_hits.insert(name.to_string(), (note, hits));
        }
    }

    // --- 音色
    let voices = map(&need(scope, "VOICES")?, "VOICES")?;
    for (name, vv) in voices.iter() {
        let m = map(vv, "音色")?;
        s.voices.insert(
            name.to_string(),
            Voice {
                ch: int(need_field(&m, "ch", &format!("VOICES.{name}"))?, "ch")?.clamp(0, 15) as u8,
                patch: field(&m, "patch").and_then(|v| v.clone().into_string().ok()),
                program: field(&m, "program")
                    .and_then(|v| v.as_int().ok())
                    .map(|v| v.clamp(0, 127) as u8),
                volume: field(&m, "volume")
                    .and_then(|v| v.as_int().ok())
                    .unwrap_or(100)
                    .clamp(0, 127) as u8,
                label: field(&m, "label")
                    .and_then(|v| v.clone().into_string().ok())
                    .unwrap_or_else(|| name.to_string()),
                color: field(&m, "color")
                    .and_then(|v| v.clone().into_string().ok())
                    .unwrap_or_else(|| "#888888".into()),
            },
        );
    }

    if let Some(v) = get(scope, "EDIT_PARTS") {
        s.edit_parts = arr(&v, "EDIT_PARTS")?
            .iter()
            .map(|p| text(p, "パート名"))
            .collect::<R<Vec<_>>>()?;
    } else {
        s.edit_parts = s.voices.keys().cloned().collect();
        s.edit_parts.sort();
    }

    // --- 自分で作る音色
    if let Some(v) = get(scope, "PATCHES") {
        for (name, pv) in map(&v, "PATCHES")?.iter() {
            let r = read_recipe(pv, name)?;
            // 鳴らす前に数を見る。無音や発振はここで止める
            recipe::check(&r).map_err(|e| LoadError::Shape(format!("PATCHES.{name}: {e}")))?;
            s.patches.insert(name.to_string(), r);
        }
    }

    // --- 音声トラック
    if let Some(v) = get(scope, "AUDIO_TRACKS") {
        for (name, tv) in map(&v, "AUDIO_TRACKS")?.iter() {
            let m = map(tv, "音声トラック")?;
            let path = text(need_field(&m, "path", &format!("AUDIO_TRACKS.{name}"))?, "path")?;
            // 絶対パスは受け付けない。書いた本人の PC でしか動かなくなる。
            if path.starts_with('/') || path.chars().nth(1) == Some(':') {
                return shape(format!(
                    "AUDIO_TRACKS.{name} の path が絶対パスです（{path}）。\
                     TONESCRIPT_ROOT からの相対で書いてください"
                ));
            }
            s.audio_tracks.insert(
                name.to_string(),
                {
                    // 秒で指す値。負や大きすぎるものは読み込みで弾く
                    let secs = |k: &str| -> R<f32> {
                        let v = field(&m, k).map(|v| num(v, k)).transpose()?.unwrap_or(0.0);
                        if !(0.0..=600.0).contains(&v) {
                            return shape(format!(
                                "AUDIO_TRACKS.{name} の {k} が {v} です。0〜600 秒の間に"
                            ));
                        }
                        Ok(v)
                    };
                    let at = field(&m, "at").map(|v| num(v, "at")).transpose()?.unwrap_or(0.0);
                    if at < 0.0 {
                        return shape(format!("AUDIO_TRACKS.{name} の at が負です"));
                    }
                    AudioTrack {
                        path,
                        gain: field(&m, "gain").map(|v| num(v, "gain")).transpose()?.unwrap_or(1.0),
                        label: field(&m, "label")
                            .and_then(|v| v.clone().into_string().ok())
                            .unwrap_or_else(|| name.to_string()),
                        color: field(&m, "color")
                            .and_then(|v| v.clone().into_string().ok())
                            .unwrap_or_else(|| "#ff4d6d".into()),
                        at: at as u32,
                        trim_in: secs("trim_in")?,
                        trim_out: secs("trim_out")?,
                        fade_in: secs("fade_in")?,
                        fade_out: secs("fade_out")?,
                    }
                },
            );
        }
    }

    // --- ミックス
    if let Some(v) = get(scope, "GAINS") {
        for (k, val) in map(&v, "GAINS")?.iter() {
            s.gains.insert(k.to_string(), num(val, "GAINS の値")?);
        }
    }
    if let Some(v) = get(scope, "MIX") {
        for (k, val) in map(&v, "MIX")?.iter() {
            let m = map(val, "MIX の中身")?;
            s.mix.insert(
                k.to_string(),
                MixCfg {
                    width: field(&m, "width").map(|v| num(v, "width")).transpose()?.unwrap_or(0.0),
                    reverb: field(&m, "reverb").map(|v| num(v, "reverb")).transpose()?.unwrap_or(0.0),
                    duck: field(&m, "duck").map(|v| num(v, "duck")).transpose()?.unwrap_or(0.0),
                },
            );
        }
    }
    if let Some(v) = get(scope, "LEAD_PATCH") {
        for (k, val) in map(&v, "LEAD_PATCH")?.iter() {
            s.lead_patch.insert(k.to_string(), text(val, "LEAD_PATCH の値")?);
        }
    }
    if let Some(v) = get(scope, "SECTION_PATCH") {
        for (sec, val) in map(&v, "SECTION_PATCH")?.iter() {
            let m = map(val, "SECTION_PATCH の中身")?;
            let mut inner = HashMap::new();
            for (part, pv) in m.iter() {
                inner.insert(part.to_string(), text(pv, "楽器名")?);
            }
            s.section_patch.insert(sec.to_string(), inner);
        }
    }
    if let Some(v) = get(scope, "SCALE") {
        s.scale = arr(&v, "SCALE")?
            .iter()
            .map(|x| int(x, "SCALE の値").map(|v| v as i32))
            .collect::<R<Vec<_>>>()?;
    }

    // --- オートメーション
    //   AUTOMATION = #{ lead: #{ gain: [[0, 1.0], [16, 0.4]] } }
    // 位置は「小節.拍」ではなく通しの目盛りで書く。小節で書けると
    // 楽だが、拍子が混ざると小節の長さが変わって指しにくくなる。
    if let Some(v) = get(scope, "AUTOMATION") {
        let total = s.total_steps();
        for (part, lanes) in map(&v, "AUTOMATION")?.iter() {
            let lm = map(lanes, &format!("AUTOMATION.{part}"))?;
            let mut out: HashMap<Lane, Curve> = HashMap::new();
            for (lane_name, pv) in lm.iter() {
                let lane = Lane::from_name(lane_name).ok_or_else(|| {
                    LoadError::Shape(format!(
                        "AUTOMATION.{part} の \"{lane_name}\" は知らない名前です。                         gain / pan / reverb / duck のどれかに"
                    ))
                })?;
                let (lo, hi) = lane.range();
                let mut pts = Vec::new();
                for it in arr(pv, &format!("AUTOMATION.{part}.{lane_name}"))?.iter() {
                    let p = arr(it, "オートメーションの節")?;
                    if p.len() < 2 {
                        return shape("節は [位置, 値] の2つです");
                    }
                    let at = int(&p[0], "節の位置")?;
                    if at < 0 {
                        return shape(format!("AUTOMATION.{part}.{lane_name}: 位置が負です"));
                    }
                    if at as u32 > total {
                        return shape(format!(
                            "AUTOMATION.{part}.{lane_name}: 位置 {at} は曲の終わり（{total}）より後です"
                        ));
                    }
                    let val = num(&p[1], "節の値")?;
                    if !(lo..=hi).contains(&val) {
                        return shape(format!(
                            "AUTOMATION.{part}.{lane_name}: 値 {val} が範囲の外です（{lo}〜{hi}）"
                        ));
                    }
                    pts.push((at as u32, val));
                }
                if !pts.is_empty() {
                    out.insert(lane, Curve::new(pts));
                }
            }
            if !out.is_empty() {
                s.automation.insert(part.to_string(), out);
            }
        }
    }

    // --- 仕上げ
    s.sidechain = tuple4(scope, "SIDECHAIN", (0.70, 0.003, 0.020, 0.200))?;
    let rv = tuple4(scope, "REVERB", (1.9, 4.2, 0.0, 0.0))?;
    s.reverb = (rv.0, rv.1);

    if let Some(v) = get(scope, "KICK") {
        let m = map(&v, "KICK")?;
        let mut k = tonescript_dsp::drum::KickCfg::default();
        if let Some(x) = field(&m, "weight") { k.weight = num(x, "KICK.weight")?; }
        if let Some(x) = field(&m, "body") { k.body = num(x, "KICK.body")?; }
        if let Some(x) = field(&m, "click") { k.click = num(x, "KICK.click")?; }
        if let Some(x) = field(&m, "length") { k.length = num(x, "KICK.length")?; }
        if let Some(x) = field(&m, "tail_hz") { k.tail_hz = num(x, "KICK.tail_hz")?; }
        s.kick = k;
    }

    Ok(s)
}

fn tuple4(scope: &Scope, name: &str, dflt: (f32, f32, f32, f32)) -> R<(f32, f32, f32, f32)> {
    let Some(v) = get(scope, name) else { return Ok(dflt) };
    let a = arr(&v, name)?;
    if a.len() < 2 {
        return shape(format!("{name} は数の並びです"));
    }
    let g = |i: usize, d: f32| -> R<f32> {
        match a.get(i) {
            Some(x) => num(x, name),
            None => Ok(d),
        }
    };
    Ok((g(0, dflt.0)?, g(1, dflt.1)?, g(2, dflt.2)?, g(3, dflt.3)?))
}


// ---------------------------------------------------------------- 自分で作る音色

/// `PATCHES` の1つを読む。書いていない所は既定値のまま。
///
/// **書き忘れで止めない。** 音色は「まず鳴らして、直す」で作るものなので、
/// 最小限（`osc` だけ、あるいは何も書かない）でも音が出るようにしてある。
/// おかしいのは値そのものだけで、それは [`recipe::check`] が言う。
fn read_recipe(v: &Dynamic, name: &str) -> R<recipe::Recipe> {
    let m = map(v, &format!("PATCHES.{name}"))?;
    let mut r = recipe::Recipe::default();
    let at = |k: &str| format!("PATCHES.{name}.{k}");

    // 発振器
    if let Some(list) = field(&m, "osc") {
        let items = arr(list, &at("osc"))?;
        r.osc = Vec::with_capacity(items.len());
        for (i, it) in items.iter().enumerate() {
            let om = map(it, &format!("{}[{i}]", at("osc")))?;
            let wave = match field(&om, "wave").and_then(|w| w.clone().into_string().ok()) {
                Some(w) => match recipe::Wave::from_name(&w) {
                    Some(x) => x,
                    None => {
                        return shape(format!(
                            "{}[{i}] の wave が {w} です。{} のどれかに",
                            at("osc"),
                            recipe::Wave::NAMES.join(" / ")
                        ))
                    }
                },
                None => recipe::Wave::Saw,
            };
            let dflt = recipe::Osc::default();
            r.osc.push(recipe::Osc {
                wave,
                mix: mnum(&om, "mix", 1.0)?,
                detune: mnum(&om, "detune", 0.0)?,
                octave: mnum(&om, "octave", 0.0)? as i32,
                // 弦のときだけ効く
                decay: mnum(&om, "decay", dflt.decay)?,
                bright: mnum(&om, "bright", dflt.bright)?,
                pick: mnum(&om, "pick", dflt.pick)?,
                // パルスのときだけ効く
                width: mnum(&om, "width", dflt.width)?,
            });
        }
    }

    // 倍音。整数倍でない倍音を足すと、鐘やガムランになる
    if let Some(list) = field(&m, "partials") {
        for (i, it) in arr(list, &at("partials"))?.iter().enumerate() {
            let t = arr(it, &format!("{}[{i}]", at("partials")))?;
            if t.len() < 3 {
                return shape(format!(
                    "{}[{i}] は [音程の倍率, 大きさ, 落ちる秒数] の3つで書いてください",
                    at("partials")
                ));
            }
            r.partials
                .push((num(&t[0], "倍率")?, num(&t[1], "大きさ")?, num(&t[2], "落ちる秒数")?));
        }
    }

    // 音量のかたち
    if let Some(e) = field(&m, "env") {
        let em = map(e, &at("env"))?;
        r.env = recipe::Env {
            a: mnum(&em, "a", r.env.a)?,
            d: mnum(&em, "d", r.env.d)?,
            s: mnum(&em, "s", r.env.s)?,
            r: mnum(&em, "r", r.env.r)?,
        };
    }

    // フィルタ
    if let Some(f) = field(&m, "filter") {
        let fm = map(f, &at("filter"))?;
        let kind = match field(&fm, "kind").and_then(|k| k.clone().into_string().ok()) {
            Some(k) => match recipe::FilterKind::from_name(&k) {
                Some(x) => x,
                None => {
                    return shape(format!(
                        "{} の kind が {k} です。{} のどれかに",
                        at("filter"),
                        recipe::FilterKind::NAMES.join(" / ")
                    ))
                }
            },
            // kind を書かずに他を書いたなら、掛けたいのだと解する
            None => recipe::FilterKind::Ladder,
        };
        let d = recipe::Filter::default();
        r.filter = recipe::Filter {
            kind,
            base: mnum(&fm, "base", d.base)?,
            sweep: mnum(&fm, "sweep", d.sweep)?,
            res: mnum(&fm, "res", d.res)?,
            track: mnum(&fm, "track", d.track)?,
            vel: mnum(&fm, "vel", d.vel)?,
            env: match field(&fm, "env") {
                Some(e) => {
                    let em = map(e, &at("filter.env"))?;
                    (
                        mnum(&em, "a", d.env.0)?,
                        mnum(&em, "d", d.env.1)?,
                        mnum(&em, "curve", d.env.2)?,
                    )
                }
                None => d.env,
            },
        };
    }

    // 頭の雑音。撥弦の爪や息の立ち上がり
    if let Some(a) = field(&m, "attack") {
        let am = map(a, &at("attack"))?;
        r.attack = recipe::Attack {
            amount: mnum(&am, "amount", 0.0)?,
            hp: mnum(&am, "hp", 2000.0)?,
            a: mnum(&am, "a", 0.0003)?,
            d: mnum(&am, "d", 0.01)?,
        };
    }

    // 揺れ
    if let Some(v) = field(&m, "vibrato") {
        let vm = map(v, &at("vibrato"))?;
        r.vibrato = recipe::Vibrato {
            rate: mnum(&vm, "rate", 5.0)?,
            depth: mnum(&vm, "depth", 0.0)?,
            delay: mnum(&vm, "delay", 0.0)?,
        };
    }

    // 音量の揺れ
    if let Some(v) = field(&m, "tremolo") {
        let tm = map(v, &at("tremolo"))?;
        r.tremolo = recipe::Tremolo {
            rate: mnum(&tm, "rate", 5.0)?,
            depth: mnum(&tm, "depth", 0.0)?,
        };
    }

    // FM
    if let Some(v) = field(&m, "fm") {
        let f2 = map(v, &at("fm"))?;
        r.fm = recipe::Fm {
            ratio: mnum(&f2, "ratio", 2.0)?,
            index: mnum(&f2, "index", 0.0)?,
            decay: mnum(&f2, "decay", 0.3)?,
        };
    }

    // やまびこ
    if let Some(v) = field(&m, "delay") {
        let dm = map(v, &at("delay"))?;
        r.delay = recipe::Delay {
            time: mnum(&dm, "time", 0.15)?,
            feedback: mnum(&dm, "feedback", 0.3)?,
            mix: mnum(&dm, "mix", 0.0)?,
        };
    }

    // 胴鳴り
    if let Some(list) = field(&m, "body") {
        for (i, it) in arr(list, &at("body"))?.iter().enumerate() {
            let t = arr(it, &format!("{}[{i}]", at("body")))?;
            if t.len() < 3 {
                return shape(format!(
                    "{}[{i}] は [中心の高さ, 鋭さ, 混ぜる量] の3つで書いてください",
                    at("body")
                ));
            }
            r.body.push((num(&t[0], "高さ")?, num(&t[1], "鋭さ")?, num(&t[2], "混ぜる量")?));
        }
    }

    r.drive = mnum(&m, "drive", r.drive)?;
    r.gain = mnum(&m, "gain", r.gain)?;
    r.ring = mnum(&m, "ring", r.ring)?;
    Ok(r)
}

/// 表から数を1つ。書いていなければ既定値。
fn mnum(m: &Map, key: &str, dflt: f32) -> R<f32> {
    match field(m, key) {
        Some(v) => num(v, key),
        None => Ok(dflt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
        let TITLE = "テスト";
        let BPM = 128;
        let SECTIONS = [
            ["イントロ", 4, "plain", "light", "main", 0.85],
            ["サビ", 4, "octa", "full", "high", 1.0],
        ];
        let VOICES = #{ lead: #{ ch: 0, patch: "supersaw", volume: 100 } };
    "#;

    #[test]
    fn minimal_song_loads() {
        let s = load_str(MINIMAL).expect("読めるはず");
        assert_eq!(s.title, "テスト");
        assert_eq!(s.bpm, 128.0);
        assert_eq!(s.bars(), 8);
        assert_eq!(s.sections.len(), 2);
        assert_eq!(s.voices["lead"].patch.as_deref(), Some("supersaw"));
        // 書かなかったものは既定値
        assert_eq!(s.tempo_curve, "smooth");
        assert_eq!(s.master_gain, 1.0);
    }

    #[test]
    fn missing_required_value_is_reported() {
        let e = load_str(r#"let TITLE = "x";"#).unwrap_err();
        let m = e.to_string();
        assert!(m.contains("BPM"), "BPM が無いと言うべき: {m}");
    }

    #[test]
    fn bad_bpm_is_rejected() {
        let src = MINIMAL.replace("let BPM = 128;", "let BPM = 5000;");
        let m = load_str(&src).unwrap_err().to_string();
        assert!(m.contains("BPM"), "{m}");
    }

    #[test]
    fn bar_helper_checks_the_sum() {
        // 合計が 16 なら通る
        let ok = format!("{MINIMAL}\nlet MELODY = #{{ \"1\": bar([[8,\"A4\"],[8,\"C5\"]]) }};");
        assert!(load_str(&ok).is_ok());
        // 合計が違えば、その場で止まる
        let ng = format!("{MINIMAL}\nlet MELODY = #{{ \"1\": bar([[8,\"A4\"],[4,\"C5\"]]) }};");
        let m = load_str(&ng).unwrap_err().to_string();
        assert!(m.contains("12/16"), "合計を教えるべき: {m}");
    }

    #[test]
    fn melody_sum_is_checked_even_without_bar_helper() {
        let ng = format!("{MINIMAL}\nlet MELODY = #{{ \"1\": [[8,\"A4\"],[4,\"C5\"]] }};");
        let m = load_str(&ng).unwrap_err().to_string();
        assert!(m.contains("12/16"), "{m}");
    }

    #[test]
    fn melody_past_the_end_is_reported() {
        let ng = format!("{MINIMAL}\nlet MELODY = #{{ \"99\": bar([[16,\"A4\"]]) }};");
        let m = load_str(&ng).unwrap_err().to_string();
        assert!(m.contains("99") && m.contains("8"), "{m}");
    }

    #[test]
    fn absolute_audio_path_is_rejected() {
        let ng = format!(
            "{MINIMAL}\nlet AUDIO_TRACKS = #{{ main: #{{ path: \"D:\\\\x\\\\y.wav\" }} }};"
        );
        let m = load_str(&ng).unwrap_err().to_string();
        assert!(m.contains("絶対パス"), "{m}");
    }

    #[test]
    fn expressions_still_work() {
        // これが Rhai を選んだ理由。データ形式では書けない。
        let src = format!(
            r#"{MINIMAL}
            let BASS_PATTERNS = #{{}};
            let octa = [];
            for i in steps(0, 16, 2) {{
                octa.push([i, 2, 0]);
                octa.push([i, 2, 12]);
            }}
            BASS_PATTERNS.octa = octa;
            "#
        );
        let s = load_str(&src).expect("読めるはず");
        let p = &s.bass_patterns["octa"];
        assert_eq!(p.len(), 16, "2つおき x 2音 = 16");
        assert_eq!(p[0], (0, 2, 0));
        assert_eq!(p[1], (0, 2, 12));
        assert_eq!(p[14], (14, 2, 0));
    }

    #[test]
    fn note_helper_is_available() {
        let src = format!("{MINIMAL}\nlet SCALE = [note(\"A4\") - note(\"A4\")];");
        let s = load_str(&src).unwrap();
        assert_eq!(s.scale, vec![0]);
    }

    #[test]
    fn chords_parse_note_names() {
        let src = format!(
            "{MINIMAL}\nlet CHORDS = #{{ \"1\": [\"Am\", [\"A3\",\"C4\",\"E4\"], \"A2\"] }};"
        );
        let s = load_str(&src).unwrap();
        let c = &s.chords[&1];
        assert_eq!(c.name, "Am");
        assert_eq!(c.tones, vec![57, 60, 64]);
        assert_eq!(c.bass, 45);
    }

    #[test]
    fn bad_note_name_is_reported() {
        let src = format!("{MINIMAL}\nlet CHORDS = #{{ \"1\": [\"X\", [\"Hz9\"], \"A2\"] }};");
        let m = load_str(&src).unwrap_err().to_string();
        assert!(m.contains("音名として読めません"), "{m}");
    }

    #[test]
    fn automation_loads() {
        let src = format!(
            "{MINIMAL}
let AUTOMATION = #{{ lead: #{{ gain: [[0, 1.0], [64, 0.25]],              pan: [[0, -1.0], [128, 1.0]] }} }};"
        );
        let s = load_str(&src).expect("読めるはず");
        let lead = &s.automation["lead"];
        assert_eq!(lead[&Lane::Gain].at(0.0), Some(1.0));
        assert_eq!(lead[&Lane::Gain].at(32.0), Some(0.625));
        assert_eq!(lead[&Lane::Pan].at(64.0), Some(0.0), "真ん中");
    }

    #[test]
    fn unknown_lane_is_reported() {
        let src = format!("{MINIMAL}
let AUTOMATION = #{{ lead: #{{ \"なんとか\": [[0, 1.0]] }} }};");
        let m = load_str(&src).unwrap_err().to_string();
        assert!(m.contains("知らない名前"), "{m}");
    }

    #[test]
    fn out_of_range_automation_is_rejected() {
        let src = format!("{MINIMAL}
let AUTOMATION = #{{ lead: #{{ pan: [[0, 5.0]] }} }};");
        let m = load_str(&src).unwrap_err().to_string();
        assert!(m.contains("範囲の外"), "{m}");
    }

    #[test]
    fn automation_past_the_end_is_rejected() {
        let src = format!("{MINIMAL}
let AUTOMATION = #{{ lead: #{{ gain: [[9999, 1.0]] }} }};");
        let m = load_str(&src).unwrap_err().to_string();
        assert!(m.contains("曲の終わり"), "{m}");
    }

    #[test]
    fn meter_can_be_written() {
        let src = MINIMAL.replace(
            r#"["サビ", 4, "octa", "full", "high", 1.0],"#,
            r#"["サビ", 4, "octa", "full", "high", 1.0, [7, 8]],"#,
        );
        let s = load_str(&src).unwrap();
        assert_eq!(s.sections[1].meter, Meter::new(7, 8));
        assert_eq!(s.bar_steps(5), 14);
        assert_eq!(s.total_steps(), 4 * 16 + 4 * 14);
    }

    #[test]
    fn script_errors_are_passed_through() {
        let m = load_str("let BPM = ;").unwrap_err().to_string();
        assert!(m.contains("エラー"), "{m}");
    }

    #[test]
    fn infinite_loop_is_stopped() {
        // 曲ファイルは信用しない。固まらずにエラーで返ること。
        let m = load_str("let x = 0; while true { x += 1; }").unwrap_err().to_string();
        assert!(!m.is_empty());
    }
}
