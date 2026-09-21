//! 保存。
//!
//! 守りたいのは3つ。
//!
//!   1. **書いている途中で落ちても壊れない**
//!      直接上書きすると、書き終える前に落ちたときに中途半端なファイルが
//!      残って、次に開けなくなる。別名で書き切ってから差し替える。
//!
//!   2. **間違えても戻せる**
//!      上書きする前のものを世代で残す。DAW の「バックアップ」に当たる。
//!
//!   3. **押し忘れても消えない**
//!      直してから少し待って、勝手に保存する。保存は別ファイル（`.autosave`）
//!      に書いて、本体とは分ける。手で保存していないものを勝手に本体へ
//!      書き込むと、「保存しないで閉じた」が効かなくなる。

use crate::json::{self, Value};
use crate::{Project, FORMAT};
use tonescript_song::model::{AudioTrack, Curve, Lane, MixCfg, Note};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 残す世代の数。
pub const BACKUPS: usize = 10;

/// 直してから自動保存までの待ち時間。
///
/// 短すぎるとタイプするたびに書きに行く。長すぎると守れる範囲が減る。
/// DAW の既定はだいたい数分だが、ここは書くものが小さいので短くできる。
pub const AUTOSAVE_AFTER: Duration = Duration::from_secs(20);

/// 保存の置き場。
///
///   <root>/<曲名>.tsp           本体
///   <root>/<曲名>.autosave.tsp  自動保存
///   <root>/backup/<曲名>-NN.tsp 世代
pub struct Store {
    root: PathBuf,
    song: String,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, song: &str) -> Self {
        Self { root: root.into(), song: song.to_string() }
    }

    pub fn main_path(&self) -> PathBuf {
        self.root.join(format!("{}.tsp", self.song))
    }

    pub fn autosave_path(&self) -> PathBuf {
        self.root.join(format!("{}.autosave.tsp", self.song))
    }

    fn backup_dir(&self) -> PathBuf {
        self.root.join("backup")
    }

    fn backup_path(&self, n: usize) -> PathBuf {
        self.backup_dir().join(format!("{}-{:02}.tsp", self.song, n))
    }

    /// 手で保存する。世代を1つ送ってから、本体を差し替える。
    pub fn save(&self, p: &Project) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        self.rotate_backups()?;
        write_atomic(&self.main_path(), &json::to_string(&encode(p)))?;
        // 本体を保存したら、自動保存は役目を終える。
        // 残しておくと、次に開いたときに「古い作業が残っている」と
        // 誤って知らせてしまう。
        let _ = std::fs::remove_file(self.autosave_path());
        Ok(())
    }

    /// 自動で保存する。本体には触らない。
    pub fn autosave(&self, p: &Project) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        write_atomic(&self.autosave_path(), &json::to_string(&encode(p)))
    }

    /// 読む。無ければ None。
    pub fn load(&self) -> Result<Option<Project>, String> {
        let p = self.main_path();
        if !p.exists() {
            return Ok(None);
        }
        let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        decode(&json::parse(&src).map_err(|e| format!("{}: {e}", p.display()))?).map(Some)
    }

    /// 自動保存が残っているか。残っていれば「前回きちんと閉じていない」。
    ///
    /// **在るかどうかだけで決める。** 手で保存すると `save` が自動保存を
    /// 消すので、残っているということは「保存したあとに何か直した」
    /// または「一度も保存していない」のどちらかになる。
    ///
    /// はじめは更新時刻を比べていたが、保存と自動保存が同じ時刻に収まると
    /// 「無い」と判断してしまい、落ちたのに知らせない事故が起きた
    /// （ファイルの時刻の細かさは環境によって違う）。時刻に頼らない。
    pub fn pending_autosave(&self) -> Option<PathBuf> {
        let a = self.autosave_path();
        a.exists().then_some(a)
    }

    /// 自動保存から戻す。
    pub fn load_autosave(&self) -> Result<Option<Project>, String> {
        let a = self.autosave_path();
        if !a.exists() {
            return Ok(None);
        }
        let src = std::fs::read_to_string(&a).map_err(|e| format!("{}: {e}", a.display()))?;
        decode(&json::parse(&src).map_err(|e| format!("{}: {e}", a.display()))?).map(Some)
    }

    /// 自動保存を捨てる。「戻さない」を選んだとき。
    pub fn discard_autosave(&self) {
        let _ = std::fs::remove_file(self.autosave_path());
    }

    /// 残っている世代。新しい順。
    pub fn backups(&self) -> Vec<PathBuf> {
        (1..=BACKUPS).map(|n| self.backup_path(n)).filter(|p| p.exists()).collect()
    }

    /// 世代から戻す。
    pub fn load_backup(&self, n: usize) -> Result<Project, String> {
        let p = self.backup_path(n);
        let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        decode(&json::parse(&src).map_err(|e| format!("{}: {e}", p.display()))?)
    }

    /// 今の本体を 01 へ送り、古いものを1つずつ後ろへずらす。
    fn rotate_backups(&self) -> Result<(), String> {
        let main = self.main_path();
        if !main.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(self.backup_dir()).map_err(|e| e.to_string())?;
        // いちばん古いものは落ちる
        let _ = std::fs::remove_file(self.backup_path(BACKUPS));
        for n in (1..BACKUPS).rev() {
            let from = self.backup_path(n);
            if from.exists() {
                let _ = std::fs::rename(&from, self.backup_path(n + 1));
            }
        }
        std::fs::copy(&main, self.backup_path(1)).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// 別名で書き切ってから差し替える。
///
/// 直接上書きすると、書いている途中で落ちたときに壊れたファイルが残る。
/// 一時ファイルへ書いて、ディスクへ送り出してから、名前を差し替える。
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        // ここまでやってから差し替える。書き終えていないものが本体に
        // なることを防ぐ
        f.sync_all().map_err(|e| e.to_string())?;
    }
    // Windows の rename は上書きできないので、先に消す。
    // ここで落ちると本体が無い一瞬ができるが、tmp は残っているので
    // 手で戻せる。世代も残っている。
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------- 変換

fn encode(p: &Project) -> Value {
    let mut root = Value::obj();
    root.insert("format", FORMAT.into());
    root.insert("song", p.song.as_str().into());

    let mut notes = Value::obj();
    for (part, ns) in &p.notes {
        let arr: Vec<Value> = ns
            .iter()
            .map(|n| {
                let mut o = Value::obj();
                o.insert("pos", n.pos.into());
                o.insert("len", n.len.into());
                o.insert("pitch", n.pitch.into());
                o.insert("vel", n.vel.into());
                if !n.mora.is_empty() {
                    o.insert("mora", n.mora.as_str().into());
                }
                o
            })
            .collect();
        notes.insert(part, Value::Arr(arr));
    }
    root.insert("notes", notes);

    let mut auto = Value::obj();
    for (part, lanes) in &p.automation {
        let mut m = Value::obj();
        for (lane, curve) in lanes {
            let pts: Vec<Value> = curve
                .points
                .iter()
                .map(|(s, v)| Value::Arr(vec![(*s).into(), (*v).into()]))
                .collect();
            m.insert(lane.name(), Value::Arr(pts));
        }
        auto.insert(part, m);
    }
    root.insert("automation", auto);

    let mut gains = Value::obj();
    for (part, g) in &p.gains {
        gains.insert(part, (*g).into());
    }
    root.insert("gains", gains);

    let mut mix = Value::obj();
    for (part, m) in &p.mix {
        let mut one = Value::obj();
        one.insert("width", m.width.into());
        one.insert("pan", m.pan.into());
        one.insert("reverb", m.reverb.into());
        one.insert("duck", m.duck.into());
        let mut e = Value::obj();
        e.insert("low", m.eq.low.into());
        e.insert("mid", m.eq.mid.into());
        e.insert("high", m.eq.high.into());
        e.insert("low_hz", m.eq.low_hz.into());
        e.insert("mid_hz", m.eq.mid_hz.into());
        e.insert("mid_q", m.eq.mid_q.into());
        e.insert("high_hz", m.eq.high_hz.into());
        one.insert("eq", e);
        mix.insert(part, one);
    }
    root.insert("mix", mix);

    let mut takes = Value::obj();
    for (name, t) in &p.takes {
        let mut one = Value::obj();
        one.insert("path", t.path.as_str().into());
        one.insert("gain", t.gain.into());
        one.insert("label", t.label.as_str().into());
        one.insert("color", t.color.as_str().into());
        one.insert("at", t.at.into());
        one.insert("trim_in", t.trim_in.into());
        one.insert("trim_out", t.trim_out.into());
        one.insert("fade_in", t.fade_in.into());
        one.insert("fade_out", t.fade_out.into());
        takes.insert(name, one);
    }
    root.insert("takes", takes);
    root.insert("muted", Value::Arr(p.muted.iter().map(|s| s.as_str().into()).collect()));
    root.insert("soloed", Value::Arr(p.soloed.iter().map(|s| s.as_str().into()).collect()));
    root
}

/// 保存した音の整えを読む。範囲外は引き戻す。
fn read_eq(one: &Value) -> tonescript_dsp::eq::EqCfg {
    let d = tonescript_dsp::eq::EqCfg::default();
    let Some(e) = one.get("eq").and_then(|x| x.as_obj()) else { return d };
    let num = |k: &str, lo: f32, hi: f32, dflt: f32| -> f32 {
        e.get(k).and_then(|x| x.as_f64()).map(|v| (v as f32).clamp(lo, hi)).unwrap_or(dflt)
    };
    tonescript_dsp::eq::EqCfg {
        low: num("low", -24.0, 24.0, 0.0),
        mid: num("mid", -24.0, 24.0, 0.0),
        high: num("high", -24.0, 24.0, 0.0),
        low_hz: num("low_hz", 20.0, 18_000.0, d.low_hz),
        mid_hz: num("mid_hz", 20.0, 18_000.0, d.mid_hz),
        mid_q: num("mid_q", 0.2, 12.0, d.mid_q),
        high_hz: num("high_hz", 20.0, 18_000.0, d.high_hz),
    }
}

fn decode(v: &Value) -> Result<Project, String> {
    let format = v.get("format").and_then(|x| x.as_u32()).unwrap_or(0);
    if format > FORMAT {
        return Err(format!(
            "この保存は新しい Tonescript で作られています（形式 {format}、こちらは {FORMAT}）"
        ));
    }
    let mut p = Project {
        song: v.get("song").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
        ..Default::default()
    };

    if let Some(m) = v.get("notes").and_then(|x| x.as_obj()) {
        for (part, arr) in m {
            let mut ns = Vec::new();
            for it in arr.as_arr().unwrap_or(&[]) {
                let pos = it.get("pos").and_then(|x| x.as_u32());
                let pitch = it.get("pitch").and_then(|x| x.as_i32());
                let (Some(pos), Some(pitch)) = (pos, pitch) else {
                    return Err(format!("{part}: 音符に pos か pitch がありません"));
                };
                ns.push(Note {
                    pos,
                    len: it.get("len").and_then(|x| x.as_u32()).unwrap_or(1).max(1),
                    pitch: pitch.clamp(0, 127),
                    vel: it.get("vel").and_then(|x| x.as_u32()).unwrap_or(100).clamp(1, 127) as u8,
                    mora: it.get("mora").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                });
            }
            ns.sort_by_key(|n| (n.pos, n.pitch));
            p.notes.insert(part.clone(), ns);
        }
    }

    if let Some(m) = v.get("automation").and_then(|x| x.as_obj()) {
        for (part, lanes) in m {
            let mut out: HashMap<Lane, Curve> = HashMap::new();
            for (name, pts) in lanes.as_obj().into_iter().flatten() {
                // 知らないレーンは黙って捨てる。古い保存を開けなくしない
                let Some(lane) = Lane::from_name(name) else { continue };
                let (lo, hi) = lane.range();
                let mut points = Vec::new();
                for q in pts.as_arr().unwrap_or(&[]) {
                    let a = q.as_arr().unwrap_or(&[]);
                    if a.len() < 2 {
                        continue;
                    }
                    if let (Some(s), Some(val)) = (a[0].as_u32(), a[1].as_f64()) {
                        points.push((s, (val as f32).clamp(lo, hi)));
                    }
                }
                if !points.is_empty() {
                    out.insert(lane, Curve::new(points));
                }
            }
            if !out.is_empty() {
                p.automation.insert(part.clone(), out);
            }
        }
    }

    if let Some(m) = v.get("gains").and_then(|x| x.as_obj()) {
        for (part, g) in m {
            if let Some(g) = g.as_f64() {
                p.gains.insert(part.clone(), (g as f32).clamp(0.0, 8.0));
            }
        }
    }
    if let Some(m) = v.get("mix").and_then(|x| x.as_obj()) {
        for (part, one) in m {
            let num = |k: &str, lo: f32, hi: f32, dflt: f32| -> f32 {
                one.get(k).and_then(|x| x.as_f64()).map(|v| (v as f32).clamp(lo, hi)).unwrap_or(dflt)
            };
            p.mix.insert(
                part.clone(),
                MixCfg {
                    width: num("width", 0.0, 4.0, 0.0),
                    pan: num("pan", -1.0, 1.0, 0.0),
                    eq: read_eq(one),
                    reverb: num("reverb", 0.0, 2.0, 0.0),
                    duck: num("duck", 0.0, 2.0, 0.0),
                },
            );
        }
    }
    if let Some(m) = v.get("takes").and_then(|x| x.as_obj()) {
        for (name, one) in m {
            let text = |k: &str| {
                one.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string()
            };
            let path = text("path");
            // 絶対パスは読まない。曲ごと渡したときに他人の機械で開けなくなる
            if path.is_empty() || path.contains(':') || path.starts_with('/') {
                continue;
            }
            let secs = |k: &str| {
                one.get(k)
                    .and_then(|x| x.as_f64())
                    .map(|v| (v as f32).clamp(0.0, 600.0))
                    .unwrap_or(0.0)
            };
            p.takes.insert(
                name.clone(),
                AudioTrack {
                    path,
                    gain: one
                        .get("gain")
                        .and_then(|x| x.as_f64())
                        .map(|v| (v as f32).clamp(0.0, 8.0))
                        .unwrap_or(1.0),
                    label: text("label"),
                    color: text("color"),
                    at: one.get("at").and_then(|x| x.as_u32()).unwrap_or(0),
                    trim_in: secs("trim_in"),
                    trim_out: secs("trim_out"),
                    fade_in: secs("fade_in"),
                    fade_out: secs("fade_out"),
                },
            );
        }
    }
    let names = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_arr())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    p.muted = names("muted");
    p.soloed = names("soloed");
    Ok(p)
}

// ---------------------------------------------------------------- 自動保存の見張り

/// 「直したら少し待って保存する」を数えるだけのもの。
///
/// 時刻を持つだけで、自分では保存しない。いつ書くかは画面側が決める。
/// ここで勝手にスレッドを起こすと、試すのが難しくなる。
pub struct Autosaver {
    dirty_since: Option<Instant>,
    last_saved: Option<Instant>,
    after: Duration,
}

impl Default for Autosaver {
    fn default() -> Self {
        Self::new(AUTOSAVE_AFTER)
    }
}

impl Autosaver {
    pub fn new(after: Duration) -> Self {
        Self { dirty_since: None, last_saved: None, after }
    }

    /// 何か直した。
    pub fn touched(&mut self) {
        self.touched_at(Instant::now());
    }

    /// 時刻を渡す版。境目を厳密に試すために分けてある。
    pub fn touched_at(&mut self, now: Instant) {
        if self.dirty_since.is_none() {
            self.dirty_since = Some(now);
        }
    }

    /// 保存されていない変更があるか。
    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some()
    }

    /// 保存し終えた。
    pub fn saved(&mut self) {
        self.dirty_since = None;
        self.last_saved = Some(Instant::now());
    }

    /// そろそろ書くべきか。
    pub fn due(&self) -> bool {
        self.due_at(Instant::now())
    }

    /// 時刻を渡す版。試すために分けてある。
    pub fn due_at(&self, now: Instant) -> bool {
        match self.dirty_since {
            Some(t) => now.duration_since(t) >= self.after,
            None => false,
        }
    }

    /// 最後に保存してから何秒経ったか。
    pub fn since_saved(&self) -> Option<Duration> {
        self.last_saved.map(|t| t.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tonescript_store_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn note(pos: u32, pitch: i32) -> Note {
        Note { pos, len: 4, pitch, vel: 100, mora: String::new() }
    }

    fn sample() -> Project {
        let mut p = Project::new("example");
        p.add_note("lead", note(0, 60));
        p.add_note("lead", note(8, 64));
        p.automation.insert(
            "lead".into(),
            HashMap::from([(Lane::Gain, Curve::new(vec![(0, 1.0), (32, 0.25)]))]),
        );
        p.gains.insert("bass".into(), 0.8);
        p.mix.insert("lead".into(), MixCfg { width: 1.35, reverb: 0.26, duck: 0.55, ..Default::default() });
        p.takes.insert(
            "take1".into(),
            AudioTrack {
                path: "takes/a.wav".into(),
                gain: 0.9,
                label: "テイク1".into(),
                color: String::new(),
                at: 32,
                trim_in: 0.25,
                fade_out: 0.5,
                ..Default::default()
            },
        );
        p.muted.push("perc".into());
        p
    }

    #[test]
    fn the_mixer_settings_survive_a_save() {
        let d = tmpdir("mix");
        let s = Store::new(&d, "example");
        let mut p = sample();
        p.mix.insert("bass".into(), MixCfg { width: 0.0, reverb: 0.02, duck: 1.0, ..Default::default() });
        s.save(&p).unwrap();
        let back = s.load().unwrap().expect("あるはず");
        assert_eq!(back.mix.len(), 2);
        let lead = back.mix["lead"];
        assert!((lead.width - 1.35).abs() < 1e-6, "広がりが {}", lead.width);
        assert!((lead.reverb - 0.26).abs() < 1e-6);
        assert!((lead.duck - 0.55).abs() < 1e-6);
        assert_eq!(back.mix["bass"].width, 0.0);
        assert_eq!(back, p);
    }

    #[test]
    fn a_take_survives_a_save() {
        let d = tmpdir("take");
        let s = Store::new(&d, "example");
        let p = sample();
        s.save(&p).unwrap();
        let back = s.load().unwrap().expect("あるはず");
        assert_eq!(back.takes["take1"].path, "takes/a.wav");
        assert!((back.takes["take1"].gain - 0.9).abs() < 1e-6);
        assert_eq!(back.takes["take1"].label, "テイク1");
        assert_eq!(back.takes["take1"].at, 32, "置き場所が消えた");
        assert!((back.takes["take1"].trim_in - 0.25).abs() < 1e-6, "切り詰めが消えた");
        assert!((back.takes["take1"].fade_out - 0.5).abs() < 1e-6, "出が消えた");
    }

    #[test]
    fn an_absolute_take_path_is_refused() {
        // 他人の機械で開けなくなる。保存はできても読まない
        let d = tmpdir("takeabs");
        let s = Store::new(&d, "example");
        let mut p = Project::new("example");
        for bad in ["C:/tmp/a.wav", "/tmp/a.wav"] {
            p.takes.insert(
                bad.into(),
                AudioTrack { path: bad.into(), ..Default::default() },
            );
        }
        s.save(&p).unwrap();
        let back = s.load().unwrap().unwrap();
        assert!(back.takes.is_empty(), "絶対パスを読んでしまった");
    }

    #[test]
    fn an_out_of_range_mix_is_pulled_back_not_trusted() {
        let d = tmpdir("mixbad");
        let s = Store::new(&d, "example");
        let mut p = Project::new("example");
        p.mix.insert("lead".into(), MixCfg { width: 99.0, reverb: -5.0, duck: 50.0, ..Default::default() });
        s.save(&p).unwrap();
        let back = s.load().unwrap().unwrap();
        let m = back.mix["lead"];
        assert_eq!(m.width, 4.0, "広がりが範囲外のまま");
        assert_eq!(m.reverb, 0.0);
        assert_eq!(m.duck, 2.0);
    }

    #[test]
    fn save_and_load_round_trip() {
        let d = tmpdir("round");
        let s = Store::new(&d, "example");
        let p = sample();
        s.save(&p).unwrap();
        let back = s.load().unwrap().expect("あるはず");
        assert_eq!(back, p);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn missing_file_is_none_not_an_error() {
        let d = tmpdir("missing");
        let s = Store::new(&d, "nope");
        assert_eq!(s.load().unwrap(), None);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn backups_rotate_and_keep_the_old_content() {
        let d = tmpdir("rotate");
        let s = Store::new(&d, "example");
        // 3回保存する。そのたびに中身を変える
        for i in 0..3 {
            let mut p = Project::new("example");
            p.add_note("lead", note(i * 16, 60 + i as i32));
            s.save(&p).unwrap();
        }
        // 世代は2つ（1回目の保存では送るものが無い）
        assert_eq!(s.backups().len(), 2);
        // 01 が「ひとつ前」= 2回目の中身
        let b1 = s.load_backup(1).unwrap();
        assert_eq!(b1.notes["lead"][0].pitch, 61);
        let b2 = s.load_backup(2).unwrap();
        assert_eq!(b2.notes["lead"][0].pitch, 60);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn backups_are_capped() {
        let d = tmpdir("cap");
        let s = Store::new(&d, "example");
        for i in 0..(BACKUPS + 5) {
            let mut p = Project::new("example");
            p.add_note("lead", note(i as u32, 60));
            s.save(&p).unwrap();
        }
        assert_eq!(s.backups().len(), BACKUPS, "世代が増え続けている");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn autosave_is_separate_from_the_main_file() {
        let d = tmpdir("auto");
        let s = Store::new(&d, "example");
        let mut p = sample();
        s.save(&p).unwrap();

        p.add_note("lead", note(32, 67));
        s.autosave(&p).unwrap();

        // 本体は保存したときのまま
        assert_eq!(s.load().unwrap().unwrap().notes["lead"].len(), 2);
        // 自動保存のほうには新しいのが入っている
        assert_eq!(s.load_autosave().unwrap().unwrap().notes["lead"].len(), 3);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn pending_autosave_is_seen_then_cleared_by_save() {
        let d = tmpdir("pending");
        let s = Store::new(&d, "example");
        let p = sample();
        s.autosave(&p).unwrap();
        // 本体が無いのに自動保存がある = 前回きちんと閉じていない
        assert!(s.pending_autosave().is_some());
        // 手で保存すると片付く
        s.save(&p).unwrap();
        assert!(s.pending_autosave().is_none());
        assert!(!s.autosave_path().exists());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn pending_does_not_depend_on_the_clock() {
        // 保存と自動保存が同じ時刻に収まっても、ちゃんと気づくこと。
        // 時刻を比べる作りだと、ここが環境によって落ちる。
        let d = tmpdir("clock");
        let s = Store::new(&d, "example");
        let p = sample();
        for _ in 0..50 {
            s.save(&p).unwrap();
            s.autosave(&p).unwrap();
            assert!(
                s.pending_autosave().is_some(),
                "保存の直後に自動保存したのに気づかない"
            );
            s.discard_autosave();
            assert!(s.pending_autosave().is_none());
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn discard_autosave_removes_it() {
        let d = tmpdir("discard");
        let s = Store::new(&d, "example");
        s.autosave(&sample()).unwrap();
        assert!(s.pending_autosave().is_some());
        s.discard_autosave();
        assert!(s.pending_autosave().is_none());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn broken_file_is_reported_not_silently_empty() {
        let d = tmpdir("broken");
        let s = Store::new(&d, "example");
        std::fs::write(s.main_path(), "{ これは JSON では").unwrap();
        let e = s.load().unwrap_err();
        assert!(e.contains("example.tsp"), "どのファイルか分からない: {e}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn newer_format_is_refused() {
        let d = tmpdir("newer");
        let s = Store::new(&d, "example");
        std::fs::write(s.main_path(), r#"{"format": 999, "song": "x"}"#).unwrap();
        let e = s.load().unwrap_err();
        assert!(e.contains("新しい"), "{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn unknown_lane_in_file_is_ignored() {
        // 古い保存を開けなくしないこと
        let d = tmpdir("lane");
        let s = Store::new(&d, "example");
        std::fs::write(
            s.main_path(),
            r#"{"format":1,"song":"x","automation":{"lead":{"gain":[[0,1]],"未来のレーン":[[0,1]]}}}"#,
        )
        .unwrap();
        let p = s.load().unwrap().unwrap();
        assert_eq!(p.automation["lead"].len(), 1);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn no_tmp_file_is_left_behind() {
        let d = tmpdir("tmp");
        let s = Store::new(&d, "example");
        s.save(&sample()).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "一時ファイルが残っている");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn autosaver_waits_then_fires() {
        let mut a = Autosaver::new(Duration::from_secs(20));
        assert!(!a.is_dirty());
        assert!(!a.due(), "何もしていないのに保存しようとした");

        a.touched();
        assert!(a.is_dirty());
        assert!(!a.due(), "直した直後に保存してしまう");
        // 20秒経ったことにする
        let later = Instant::now() + Duration::from_secs(21);
        assert!(a.due_at(later));

        a.saved();
        assert!(!a.is_dirty());
        assert!(!a.due_at(later + Duration::from_secs(60)), "保存後も保存し続ける");
    }

    #[test]
    fn autosaver_counts_from_the_first_edit() {
        let mut a = Autosaver::new(Duration::from_secs(20));
        a.touched();
        let first = Instant::now();
        // 続けて直しても、待ち時間は最初の1回から数える。
        // そうしないと、ずっと触っている間いつまでも保存されない。
        a.touched();
        a.touched();
        assert!(a.due_at(first + Duration::from_secs(21)));
    }
}
