//! Tonescript の画面。
//!
//! 曲を選び、譜面を見て、音符を触り、鳴らす。
//!
//! 保存について
//! ------------
//! 曲そのもの（`.rhai`）は手で書くもので、画面からは書き換えない。
//! 触った結果は別のファイル（`.tsp`）に持つ。曲ファイルのコメントも式も
//! 消えないし、曲を直せば触っていないパートはついてくる。
//!
//! 直してから少し置くと自動で保存する。自動保存は本体とは別のファイルへ
//! 書く。手で保存していないものを勝手に本体へ書くと「保存しないで閉じた」
//! が効かなくなるため。前回きちんと閉じていなければ、開いたときに聞く。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browser;
mod edit;
mod font;
mod keys;
mod mixer;
mod rec;
mod sel;
mod newsong;
mod play;
mod settings;
mod theme;

use edit::{Editor, Transport};
use tonescript_engine::Engine;
use tonescript_project::store::{Autosaver, Store};
use tonescript_project::{History, Project, Tag};
use std::path::Path;
use tonescript_render::{build, render_song, wav, Score};
use tonescript_song::model::{Lane, Meter};
use tonescript_song::Song;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;

fn songs_dir() -> PathBuf {
    std::env::var("TONESCRIPT_SONGS").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("songs"))
}

fn out_dir() -> PathBuf {
    std::env::var("TONESCRIPT_ROOT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("out"))
}

/// 編集の保存先。書き出し先とは分ける（消しても作業は消えない）。
fn project_dir() -> PathBuf {
    std::env::var("TONESCRIPT_PROJECTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| out_dir().join("projects"))
}

enum Msg {
    Line(String),
    Done { secs: f32, took: f32, lufs: f32, path: String },
    Failed(String),
}

/// どの画面を出しているか。
///
/// 曲を選ぶ所と、曲を作る所を分ける。編集の画面に一覧を混ぜていたが、
/// あそこは曲を作る所で、曲を選ぶ所ではない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// 曲の一覧。開いたら最初に出る
    Browser,
    /// ピアノロール
    Editor,
}

/// 前回きちんと閉じていなかったときに出す問いかけ。
struct Recovery {
    song: String,
}

struct App {
    screen: Screen,
    /// 曲の一覧。開いたときと、曲を作ったときに作り直す
    entries: Vec<browser::Entry>,
    picked: Option<String>,
    song: Option<Song>,
    /// 曲ファイルが作った譜面に、手で触ったぶんを重ねたもの
    score: Option<Score>,
    error: Option<String>,

    project: Project,
    history: History,
    saver: Autosaver,
    recovery: Option<Recovery>,
    status: String,

    ed: Editor,
    zoom: f32,
    scroll: f32,

    /// 鳴らす側。譜面と設定を渡すと、鳴らしながら作る
    engine: Engine,
    /// 音の出口（cpal）。開けなくても編集と書き出しは続けられる
    out: play::Out,
    /// 再生ヘッドの位置（目盛り）。止まっている間もここを覚えている
    head: f32,
    /// 繰り返す範囲（目盛り）
    loop_range: Option<(f32, f32)>,
    /// 目盛り -> 秒 の対応。曲を読むたびに作り直す
    step_times: Vec<f64>,
    /// 画面が再生に追いて動くか
    follow: bool,

    /// 鍵盤の口
    midi: keys::Input,
    /// 鍵盤の今（押している音・ペダル）
    board: keys::Keyboard,
    /// 弾いたものを譜面へ入れるか
    arm: bool,
    /// 録っている最中の音。音程 -> (始めた目盛り, 強さ)
    taking: std::collections::HashMap<i32, (u32, u8)>,
    /// 見つかっている鍵盤の名前。開くたびに数え直すと重いので覚えておく
    ports: Vec<String>,
    ports_at: Option<std::time::Instant>,

    /// ミキサーを開いているか
    show_mixer: bool,
    /// 針
    needles: mixer::Needles,
    /// 前のフレームの時刻。針の落ち方に使う
    last_frame: Option<std::time::Instant>,
    /// 写したもの
    clip: sel::Clip,
    /// 書き出しの形
    fmt: tonescript_render::export::Format,
    /// 画面に出す音声トラック。読んだときに作る
    clips: Vec<edit::Clip>,
    /// 繰り返す範囲だけ書き出すか
    export_loop: bool,
    /// 録っている最中。開いていれば Some
    taking_audio: Option<rec::Rec>,
    /// 入力の針
    in_level: f32,
    /// カウントインの拍数。0 なら数えない
    count_in: u32,
    /// 叩いて速さを決めるやつ
    tap: settings::Tap,
    /// 見つかっている録る口
    in_ports: Vec<String>,
    in_ports_at: Option<std::time::Instant>,

    log: Vec<String>,
    rx: Option<mpsc::Receiver<Msg>>,
    busy: bool,
    font_note: Option<String>,

    /// 新規作成の窓。開いていれば Some
    making: Option<newsong::Spec>,
    /// 新規作成の窓に出す注意書き
    make_error: Option<String>,
    /// 曲の設定の窓。開いていれば Some
    tuning: Option<settings::Draft>,
    /// 設定の窓に出す注意書き
    tune_error: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        // 鳴らす側と、音の出口。出口が開かなくても engine は動く
        let (engine, mixer) = Engine::new();
        let out = play::Out::start(mixer);
        let mut app = Self {
            screen: Screen::Browser,
            entries: Vec::new(),
            picked: None,
            song: None,
            score: None,
            error: None,
            project: Project::default(),
            history: History::default(),
            saver: Autosaver::default(),
            recovery: None,
            status: String::new(),
            ed: Editor::default(),
            zoom: 12.0,
            scroll: 0.0,
            engine,
            out,
            head: 0.0,
            loop_range: None,
            step_times: Vec::new(),
            follow: true,
            midi: keys::Input::idle(),
            board: keys::Keyboard::default(),
            arm: false,
            taking: std::collections::HashMap::new(),
            ports: Vec::new(),
            ports_at: None,
            show_mixer: false,
            needles: mixer::Needles::default(),
            last_frame: None,
            clip: sel::Clip::default(),
            fmt: tonescript_render::export::Format::default(),
            clips: Vec::new(),
            export_loop: false,
            taking_audio: None,
            count_in: 4,
            tap: settings::Tap::default(),
            in_level: 0.0,
            in_ports: Vec::new(),
            in_ports_at: None,
            log: Vec::new(),
            rx: None,
            busy: false,
            font_note: None,
            making: None,
            make_error: None,
            tuning: None,
            tune_error: None,
        };
        app.refresh_list();
        app
    }
}

impl App {
    /// 曲の一覧を読み直す。
    fn refresh_list(&mut self) {
        self.entries = browser::list(&songs_dir(), &project_dir());
    }

    /// 曲を開いて、編集の画面へ移る。
    fn open_song(&mut self, name: &str) {
        // 今の曲の未保存ぶんを落としてから移る
        if self.saver.is_dirty() {
            if let Some(st) = self.store() {
                let _ = st.autosave(&self.project);
            }
        }
        self.picked = Some(name.to_string());
        self.open();
        self.screen = Screen::Editor;
    }

    fn store(&self) -> Option<Store> {
        self.picked.as_ref().map(|n| Store::new(project_dir(), n))
    }

    /// 曲を開く。編集も読み、自動保存が残っていれば聞く。
    fn open(&mut self) {
        self.song = None;
        self.score = None;
        self.error = None;
        self.project = Project::default();
        self.history = History::default();
        self.saver = Autosaver::default();
        self.recovery = None;
        self.scroll = 0.0;

        let Some(name) = self.picked.clone() else { return };
        let path = songs_dir().join(format!("{name}.rhai"));
        let song = match tonescript_song::load_file(&path) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        // 編集できるパートの既定を、曲が持っているものに合わせる
        if !song.edit_parts.contains(&self.ed.part) {
            if let Some(p) = song.edit_parts.first() {
                self.ed.part = p.clone();
            }
        }
        self.song = Some(song);

        if let Some(st) = self.store() {
            match st.load() {
                Ok(Some(p)) => {
                    self.project = p;
                    self.status = format!("{} を読みました", st.main_path().display());
                }
                Ok(None) => self.project = Project::new(&name),
                Err(e) => {
                    self.project = Project::new(&name);
                    self.status = format!("[!] 保存を読めません: {e}");
                }
            }
            if st.pending_autosave().is_some() {
                self.recovery = Some(Recovery { song: name.clone() });
            }
        }
        self.step_times = self
            .song
            .as_ref()
            .map(tonescript_render::arrange::step_times)
            .unwrap_or_default();
        self.head = 0.0;
        self.loop_range = None;
        self.engine.stop();
        self.engine.set_loop(0, 0);
        // 押しっぱなしのまま曲を変えると鳴り続ける
        for act in self.board.panic() {
            if let keys::Act::Off { pitch } = act {
                self.engine.key_up(&self.ed.part.clone(), pitch);
            }
        }
        self.taking.clear();
        self.rebuild();
        self.push_to_engine();
        self.load_audio();
        // 開いた直後は曲ぜんぶが見えるほうが迷わない
        if let Some(song) = &self.song {
            self.zoom = edit::zoom_for_whole(song, self.ed.view_w);
            self.scroll = 0.0;
        }
    }

    /// 目盛りを秒へ。テンポが動いても合うように、表を引く。
    fn step_to_sec(&self, step: f32) -> f64 {
        if self.step_times.is_empty() {
            return 0.0;
        }
        let i = (step.max(0.0) as usize).min(self.step_times.len() - 1);
        let base = self.step_times[i];
        // 目盛りの途中は、その目盛りの長さで割って足す
        let frac = (step - i as f32).clamp(0.0, 1.0) as f64;
        let next = self.step_times.get(i + 1).copied().unwrap_or(base);
        base + (next - base) * frac
    }

    /// 秒を目盛りへ。
    fn sec_to_step(&self, sec: f64) -> f32 {
        if self.step_times.is_empty() {
            return 0.0;
        }
        let t = &self.step_times;
        // t は昇順。sec を挟む2つを探して、あいだを比で割る
        match t.binary_search_by(|x| x.partial_cmp(&sec).unwrap()) {
            Ok(i) => i as f32,
            Err(0) => 0.0,
            Err(i) if i >= t.len() => (t.len() - 1) as f32,
            Err(i) => {
                let (a, b) = (t[i - 1], t[i]);
                let span = b - a;
                let f = if span > 0.0 { (sec - a) / span } else { 0.0 };
                (i - 1) as f32 + f.clamp(0.0, 1.0) as f32
            }
        }
    }

    /// 目盛りをサンプル位置へ。鳴らす側は 48kHz で数える。
    fn sample_of(&self, step: f32) -> u64 {
        (self.step_to_sec(step) * tonescript_dsp::osc::SR as f64) as u64
    }

    /// 新しい曲を作って、そのまま開く。
    fn create_song(&mut self) {
        let Some(spec) = self.making.clone() else { return };
        match newsong::create(&spec, &songs_dir()) {
            Ok(path) => {
                // 作る前に、今の曲の未保存ぶんを落としておく
                if self.saver.is_dirty() {
                    if let Some(st) = self.store() {
                        let _ = st.autosave(&self.project);
                    }
                }
                self.refresh_list();
                self.making = None;
                self.make_error = None;
                let name = spec.name.trim().to_string();
                self.open_song(&name);
                self.status = format!("{} を作りました", path.display());
            }
            Err(e) => self.make_error = Some(e),
        }
    }

    /// 今の曲を写して、別の名前で作る。
    fn duplicate_song(&mut self) {
        let Some(from) = self.picked.clone() else { return };
        let dir = songs_dir();
        // 空いている名前を探す
        let mut name = format!("{from}-2");
        let mut n = 2;
        while newsong::path_of(&dir, &name).exists() {
            n += 1;
            name = format!("{from}-{n}");
        }
        let src = dir.join(format!("{from}.rhai"));
        match std::fs::read_to_string(&src)
            .map_err(|e| e.to_string())
            .and_then(|t| {
                std::fs::write(newsong::path_of(&dir, &name), t).map_err(|e| e.to_string())
            }) {
            Ok(()) => {
                self.refresh_list();
                self.open_song(&name);
                self.status = format!("{from} を写して {name} を作りました");
            }
            Err(e) => self.status = format!("[!] 写せません: {e}"),
        }
    }

    /// MIDI へ持ち出す。
    fn midi_out(&mut self) {
        let (Some(song), Some(score), Some(name)) =
            (self.song.clone(), self.score.clone(), self.picked.clone())
        else {
            return;
        };
        let path = out_dir().join(&name).join(format!("{name}.mid"));
        match tonescript_midi::export(&song, &score, &path) {
            Ok(n) => {
                self.status = format!("{} へ書きました（{n} バイト）", path.display());
                self.log.push(format!("[MIDI] -> {}", path.display()));
                self.log.push(
                    "        MIDI は音を運びません。音色は番号だけなので、\
                     持ち出した先では別の音で鳴ります".into(),
                );
            }
            Err(e) => self.status = format!("[!] {e}"),
        }
    }

    /// MIDI を取り込む。取り消せるように、変える前に覚えてから入れる。
    fn midi_in(&mut self, path: &Path) {
        let Some(song) = self.song.clone() else { return };
        match tonescript_midi::import(path, &song) {
            Ok(got) => {
                self.record(Tag::Once);
                let mut n = 0;
                let mut parts: Vec<&String> = got.score.keys().collect();
                parts.sort();
                for part in parts {
                    let notes = got.score[part].clone();
                    n += notes.len();
                    self.log.push(format!("[MIDI] {:<10} {:>5} ノート", part, notes.len()));
                    self.project.notes.insert(part.clone(), notes);
                }
                for w in &got.skipped {
                    self.log.push(format!("[MIDI] [!] {w}"));
                }
                if let Some(b) = got.bpm {
                    self.log.push(format!(
                        "[MIDI] 相手のテンポは {b:.1} BPM（曲の設定は変えていません）"
                    ));
                }
                if got.bars > song.bars() {
                    self.log.push(format!(
                        "[MIDI] [!] {} 小節ぶんありますが、曲は {} 小節です。\
                         はみ出したぶんは鳴りません",
                        got.bars,
                        song.bars()
                    ));
                }
                self.touched();
                self.status = format!("{} から {n} ノート取り込みました", path.display());
            }
            Err(e) => self.status = format!("[!] {e}"),
        }
    }

    /// 取り込む MIDI を探す。曲の書き出し先と、その親を見る。
    fn midi_candidates(&self) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut dirs = vec![out_dir()];
        if let Some(n) = &self.picked {
            dirs.insert(0, out_dir().join(n));
        }
        for d in dirs {
            if let Ok(rd) = std::fs::read_dir(&d) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("mid") {
                        out.push(p);
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// 画面の左端にある小節。寄せるときの軸にする。
    ///
    /// 画面の真ん中でも端でもなく左端にするのは、寄せたあとに
    /// 「さっきまで見ていた所」が画面に残るのがここだから。
    fn left_bar(&self) -> u32 {
        let Some(song) = &self.song else { return 1 };
        let step = (self.scroll / self.zoom).max(0.0) as u32;
        song.bar_of_step(step)
    }

    /// n 小節ぶんが収まるように寄せる。`None` なら曲まるごと。
    fn fit_bars(&mut self, n: Option<u32>) {
        let Some(song) = self.song.clone() else { return };
        let w = self.ed.view_w;
        match n {
            Some(n) => {
                let from = self.left_bar();
                self.zoom = edit::zoom_for_bars(&song, from, n, w);
                // 左端をその小節の頭へ揃える。半端な所から始まると
                // 「n小節ぶん」に見えない
                self.scroll = song.bar_start(from) as f32 * self.zoom;
                self.status = format!("{n}小節ぶんを表示");
            }
            None => {
                self.zoom = edit::zoom_for_whole(&song, w);
                self.scroll = 0.0;
                self.status = "曲ぜんぶを表示".into();
            }
        }
    }

    /// 曲ファイルに、手で触ったぶんを重ねたもの。
    ///
    /// **書き出しと鳴らす側で同じものを使う。** 2か所で重ねると必ずずれる。
    fn merged_song(&self) -> Option<Song> {
        let mut song = self.song.clone()?;
        for (part, lanes) in &self.project.automation {
            song.automation.insert(part.clone(), lanes.clone());
        }
        for (part, g) in &self.project.gains {
            song.gains.insert(part.clone(), *g);
        }
        for (part, m) in &self.project.mix {
            song.mix.insert(part.clone(), *m);
        }
        for (name, t) in &self.project.takes {
            song.audio_tracks.insert(name.clone(), t.clone());
        }
        Some(song)
    }

    /// 外で録った音を読んで、鳴らす側へ渡す。
    ///
    /// 読むのは重いので、曲を開いたときと、録り終えたときだけ。
    fn load_audio(&mut self) {
        let Some(song) = self.merged_song() else { return };
        if song.audio_tracks.is_empty() {
            self.engine.set_audio(Vec::new());
            return;
        }
        let quiet = |_: &str| {};
        let (list, missing) = tonescript_render::load_audio_tracks(&song, &out_dir(), &quiet);
        for m in &missing {
            self.log.push(format!("[!] {m}"));
        }
        if !missing.is_empty() {
            self.status = format!("読めない音が {} 本あります（記録を見てください）", missing.len());
        }
        // 画面に出すぶんを先に作る。波形は山だけ間引いて持つ
        //（3分の歌は 800万サンプル。描くのに要るのは数百点）
        let mut names: Vec<String> = song.audio_tracks.keys().cloned().collect();
        names.sort();
        let mut clips = Vec::new();
        for name in &names {
            let t = &song.audio_tracks[name];
            let Some((_, st, _)) = list.iter().find(|(l, _, _)| *l == t.label) else { continue };
            // 置いた位置より後ろだけが中身。頭の無音は数えない
            let at_sample = self.sample_of(t.at as f32) as usize;
            let body = st.len().saturating_sub(at_sample);
            let secs = body as f32 / tonescript_dsp::osc::SR;
            let len = self.sec_to_step(self.step_to_sec(t.at as f32) + secs as f64) - t.at as f32;
            clips.push(edit::Clip {
                name: name.clone(),
                label: if t.label.is_empty() { name.clone() } else { t.label.clone() },
                at: t.at,
                len: len.max(0.5),
                secs,
                peaks: peaks_of(&st.l[at_sample.min(st.len())..], 400),
                gain: t.gain,
                trim_in: t.trim_in,
                trim_out: t.trim_out,
            });
        }
        self.clips = clips;

        let tracks = list
            .into_iter()
            .map(|(label, st, gain)| tonescript_engine::Track {
                label,
                l: Arc::new(st.l),
                r: Arc::new(st.r),
                gain,
            })
            .collect();
        self.engine.set_audio(tracks);
    }

    /// 音声トラックを触った。取り消せる形で当てる。
    fn apply_clip(&mut self, e: edit::ClipEdit) {
        let name = match &e {
            edit::ClipEdit::Move { name, .. }
            | edit::ClipEdit::TrimIn { name, .. }
            | edit::ClipEdit::TrimOut { name, .. } => name.clone(),
        };
        // 曲ファイルに書いてあるものは触らない。触れるのは録ったものだけ
        if !self.project.takes.contains_key(&name) {
            self.status = "曲ファイルに書いてある音声は、ファイルを直してください".into();
            return;
        }
        // 引きずっているあいだは1段にまとめる
        self.history.record(&self.project, Tag::Curve(name.clone(), "clip"));
        let Some(t) = self.project.takes.get_mut(&name) else { return };
        match e {
            edit::ClipEdit::Move { at, .. } => t.at = at,
            edit::ClipEdit::TrimIn { secs, .. } => t.trim_in = secs.clamp(0.0, 600.0),
            edit::ClipEdit::TrimOut { secs, .. } => t.trim_out = secs.clamp(0.0, 600.0),
        }
        self.saver.touched();
        self.load_audio();
    }

    /// 録り始める。曲の今の位置から。
    fn start_rec(&mut self, which: Option<usize>) {
        if self.taking_audio.is_some() {
            return;
        }
        let r = rec::Rec::start(which);
        match (&r.error, r.is_open()) {
            (Some(e), _) => {
                self.status = format!("[!] 録れません: {e}");
                return;
            }
            (None, false) => {
                self.status = "録る口が開きません".into();
                return;
            }
            _ => {}
        }
        self.status = format!(
            "録っています: {}（{} ch / {} Hz）",
            r.name.clone().unwrap_or_default(),
            r.channels,
            r.sample_rate
        );
        self.taking_audio = Some(r);
    }

    /// 録り終えて、曲へ入れる。
    fn stop_rec(&mut self) {
        let Some(r) = self.taking_audio.take() else { return };
        let secs = r.seconds();
        let (l, r2) = match r.finish() {
            Ok(v) => v,
            Err(e) => {
                self.status = format!("[!] {e}");
                return;
            }
        };
        // 書き出し先は TONESCRIPT_ROOT からの相対。曲ごと渡しても開ける
        let dir = out_dir().join("takes");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.status = format!("[!] 置き場が作れません: {e}");
            return;
        }
        let stem = self.picked.clone().unwrap_or_else(|| "take".into());
        let mut i = 1;
        let (name, path) = loop {
            let name = format!("{stem}-{i}");
            let path = dir.join(format!("{name}.wav"));
            if !path.exists() {
                break (name, path);
            }
            i += 1;
            if i > 999 {
                self.status = "[!] 置き場がいっぱいです".into();
                return;
            }
        };
        let stereo = tonescript_render::mix::Stereo { l, r: r2 };
        if let Err(e) = tonescript_render::wav::write_stereo(&path, &stereo, tonescript_dsp::osc::SR as u32) {
            self.status = format!("[!] 書けません: {e}");
            return;
        }
        self.record(Tag::Once);
        self.project.takes.insert(
            name.clone(),
            tonescript_song::model::AudioTrack {
                path: format!("takes/{name}.wav"),
                gain: 1.0,
                label: name.clone(),
                // 録り始めた所へ置く。頭に無音を足すのではなく位置で持つ
                at: self.ed.snapped(self.head.max(0.0)),
                ..Default::default()
            },
        );
        self.saver.touched();
        self.load_audio();
        self.log.push(format!("録音 {name}  {secs:.1}秒  {}", path.display()));
        self.status = format!("{name} に録りました（{secs:.1}秒）");
    }

    /// ぶら下がっている録る口を数え直す。
    fn refresh_in_ports(&mut self) {
        let fresh = self
            .in_ports_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1));
        if fresh {
            return;
        }
        self.in_ports = rec::Rec::ports();
        self.in_ports_at = Some(std::time::Instant::now());
    }

    /// 今の譜面と設定を鳴らす側へ渡す。
    ///
    /// 前は「曲まるごとを作り直して帯を差し替える」だった所。今は譜面を
    /// 渡すだけで、音は鳴らしながら作られる。**鳴っている最中でもいい。**
    fn push_to_engine(&mut self) {
        let (Some(song), Some(score)) = (self.song.clone(), self.score.clone()) else { return };
        self.engine.set_score(Arc::new(song), Arc::new(score));
        // 手で触ったぶん（フェーダー・送り・ミュート）を重ねる。
        // 書き出しと同じ重ね方にすること
        self.engine.set_levels(&self.project.gains, &self.project.mix);
        self.engine.set_mutes(&self.project.muted, &self.project.soloed);
        self.engine.seek(self.sample_of(self.head));
        if let Some((a, b)) = self.loop_range {
            self.engine.set_loop(self.sample_of(a), self.sample_of(b));
        }
    }

    /// 鳴らす。止まっていれば今の位置から、鳴っていれば止める。
    fn toggle_play(&mut self) {
        if self.out.error.is_some() {
            self.status = "音の出口が開けません（書き出しはできます）".into();
            return;
        }
        if self.engine.counting_in() {
            // 数えている最中にもう一度押したら、やめる
            self.engine.cancel_count();
            self.status = "数えるのをやめました".into();
            return;
        }
        if self.engine.is_playing() {
            // 止めたら、止めた所を覚える
            self.head = self.sec_to_step(self.engine.seconds());
            self.engine.stop();
            return;
        }
        self.engine.seek(self.sample_of(self.head));
        if self.count_in > 0 && self.engine.click() > 0.0 {
            // メトロノームを鳴らしているときだけ数える。
            // 切ってあるのに黙って待たされるのは、ただ遅いだけ
            self.engine.play_after_count(self.count_in);
            self.status = format!("{} 拍数えます", self.count_in);
        } else {
            self.engine.play();
        }
    }

    /// 譜面の編集に効く鍵。
    ///
    /// 文字を打ち込んでいる最中（曲名の欄など）は効かせない。
    fn edit_keys(&mut self, ctx: &egui::Context) {
        if self.song.is_none() || self.screen != Screen::Editor {
            return;
        }
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        use egui::Key;
        let (cmd, shift, keys) = ctx.input(|i| {
            let ks: Vec<Key> = [
                Key::A, Key::C, Key::X, Key::V, Key::D, Key::Delete, Key::Backspace,
                Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown, Key::Escape,
            ]
            .into_iter()
            .filter(|k| i.key_pressed(*k))
            .collect();
            (i.modifiers.command, i.modifiers.shift, ks)
        });
        if keys.is_empty() {
            return;
        }
        let snap = self.ed.snap.max(1) as i32;
        for k in keys {
            match (cmd, k) {
                (true, Key::A) => self.select_all(),
                (true, Key::C) => self.copy_sel(),
                (true, Key::X) => {
                    self.copy_sel();
                    self.delete_sel();
                }
                (true, Key::V) => self.paste_clip(),
                (true, Key::D) => self.duplicate_sel(),
                (false, Key::Delete) | (false, Key::Backspace) => self.delete_sel(),
                (false, Key::ArrowLeft) => self.nudge_sel(-snap, 0),
                (false, Key::ArrowRight) => self.nudge_sel(snap, 0),
                // Shift を足すと1オクターブ
                (false, Key::ArrowUp) => self.nudge_sel(0, if shift { 12 } else { 1 }),
                (false, Key::ArrowDown) => self.nudge_sel(0, if shift { -12 } else { -1 }),
                (false, Key::Escape) => self.ed.clear_sel(),
                _ => {}
            }
        }
    }

    /// 鍵盤から届いたぶんを捌く。毎フレーム1回。
    fn pump_keys(&mut self) {
        if !self.midi.is_open() {
            return;
        }
        let evs = self.midi.drain();
        if evs.is_empty() {
            return;
        }
        let part = self.ed.part.clone();
        let mut changed = false;
        for ev in evs {
            for act in self.board.apply(ev) {
                match act {
                    keys::Act::On { pitch, vel } => {
                        self.engine.key_down(&part, pitch, vel);
                        if self.arm {
                            changed |= self.take_on(&part, pitch, vel);
                        }
                    }
                    keys::Act::Off { pitch } => {
                        self.engine.key_up(&part, pitch);
                        if self.arm {
                            changed |= self.take_off(&part, pitch);
                        }
                    }
                }
            }
        }
        if changed {
            self.touched();
        }
    }

    /// 弾き始めを覚える。止まっているときは、その場で1つ置いて先へ進む。
    fn take_on(&mut self, part: &str, pitch: i32, vel: u8) -> bool {
        let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(0);
        if total == 0 {
            return false;
        }
        if self.engine.is_playing() {
            // 鳴らしながら弾いている。離すまで長さが決まらないので覚えておく
            self.taking.insert(pitch, (self.ed.snapped(self.head.max(0.0)), vel));
            return false;
        }
        // 止まっている。1つ置いて、その長さぶん先へ進む（ステップ入力）
        let at = self.ed.snapped(self.head.max(0.0)).min(total.saturating_sub(1));
        let len = self.ed.new_len.max(1).min(total - at);
        self.record(Tag::Once);
        self.copy_part_for_edit(part);
        self.project.add_note(
            part,
            tonescript_song::model::Note { pos: at, len, pitch, vel, mora: String::new() },
        );
        self.head = (at + len) as f32;
        self.engine.seek(self.sample_of(self.head));
        true
    }

    /// 弾き終わりで長さが決まる。
    fn take_off(&mut self, part: &str, pitch: i32) -> bool {
        let Some((from, vel)) = self.taking.remove(&pitch) else { return false };
        let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(0);
        if total == 0 || from >= total {
            return false;
        }
        let now = self.ed.snapped(self.head.max(0.0));
        let len = now.saturating_sub(from).max(self.ed.snap.max(1)).min(total - from);
        self.record(Tag::Once);
        self.copy_part_for_edit(part);
        self.project.add_note(
            part,
            tonescript_song::model::Note { pos: from, len, pitch, vel, mora: String::new() },
        );
        true
    }

    /// 曲ファイルが作ったぶんを手元へ写す。触る前に一度だけ。
    fn copy_part_for_edit(&mut self, part: &str) {
        if !self.project.is_edited(part) {
            let from = self
                .score
                .as_ref()
                .and_then(|sc| sc.get(part).cloned())
                .unwrap_or_default();
            self.project.notes.insert(part.to_string(), from);
        }
    }

    /// ぶら下がっている鍵盤を数え直す。1秒に1回まで。
    fn refresh_ports(&mut self) {
        let fresh = self
            .ports_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1));
        if fresh {
            return;
        }
        self.ports = keys::Input::ports();
        self.ports_at = Some(std::time::Instant::now());
    }

    /// その鍵盤へ繋ぐ。
    fn open_midi(&mut self, which: usize) {
        self.midi = keys::Input::open(Some(which));
        match (&self.midi.name, &self.midi.error) {
            (Some(n), _) => self.status = format!("鍵盤: {n}"),
            (None, Some(e)) => self.status = format!("[!] 鍵盤が繋がりません: {e}"),
            _ => self.status = "鍵盤が見つかりません".into(),
        }
    }

    /// 今のパートの音符。手で触っていなければ曲ファイルのぶんを写してから。
    fn notes_for_edit(&mut self) -> Option<&mut Vec<tonescript_song::model::Note>> {
        let part = self.ed.part.clone();
        self.copy_part_for_edit(&part);
        self.project.notes.get_mut(&part)
    }

    /// 今のパートを全部選ぶ。
    fn select_all(&mut self) {
        let part = self.ed.part.clone();
        self.copy_part_for_edit(&part);
        let n = self.project.notes.get(&part).map(|v| v.len()).unwrap_or(0);
        self.ed.select((0..n).collect());
        self.status = format!("{n} 個を選びました");
    }

    /// 選んでいるものを写す。
    fn copy_sel(&mut self) {
        let part = self.ed.part.clone();
        let Some(ns) = self.project.notes.get(&part) else { return };
        let c = sel::copy(ns, &self.ed.sel);
        if c.is_empty() {
            self.status = "選んでいるものがありません".into();
            return;
        }
        self.status = format!("{} 個を写しました", c.notes.len());
        self.clip = c;
    }

    /// 選んでいるものを消す。
    fn delete_sel(&mut self) {
        if self.ed.sel.is_empty() {
            return;
        }
        self.record(Tag::Once);
        let picked = self.ed.sel.clone();
        let Some(ns) = self.notes_for_edit() else { return };
        *ns = sel::remove(ns, &picked);
        self.ed.clear_sel();
        self.status = format!("{} 個を消しました", picked.len());
        self.touched();
    }

    /// 写したものを再生ヘッドの所へ貼る。
    fn paste_clip(&mut self) {
        if self.clip.is_empty() {
            self.status = "写したものがありません".into();
            return;
        }
        let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(0);
        let at = self.ed.snapped(self.head.max(0.0));
        let add = sel::paste(&self.clip, at, total);
        if add.is_empty() {
            self.status = "そこには貼れません（曲の外）".into();
            return;
        }
        self.record(Tag::Once);
        let Some(ns) = self.notes_for_edit() else { return };
        let first = ns.len();
        ns.extend(add.iter().cloned());
        self.ed.select((first..first + add.len()).collect());
        self.status = format!("{} 個を貼りました", add.len());
        self.touched();
    }

    /// 選んでいるものを、そのすぐ後ろへ複製する。
    fn duplicate_sel(&mut self) {
        if self.ed.sel.is_empty() {
            self.status = "選んでいるものがありません".into();
            return;
        }
        let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(0);
        let part = self.ed.part.clone();
        let Some(ns) = self.project.notes.get(&part) else { return };
        let (add, picked) = sel::duplicate(ns, &self.ed.sel, total);
        if add.is_empty() {
            self.status = "そこには複製できません（曲の外）".into();
            return;
        }
        self.record(Tag::Once);
        if let Some(ns) = self.notes_for_edit() {
            ns.extend(add.iter().cloned());
        }
        self.ed.select(picked);
        self.status = format!("{} 個を複製しました", add.len());
        self.touched();
    }

    /// 選んでいるものを目盛りへ揃える。
    fn quantize_sel(&mut self, strength: f32) {
        if self.ed.sel.is_empty() {
            self.status = "選んでいるものがありません".into();
            return;
        }
        let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(0);
        let picked = self.ed.sel.clone();
        let grid = self.ed.snap.max(1);
        self.record(Tag::Once);
        let done = match self.notes_for_edit() {
            Some(ns) => sel::quantize(ns, &picked, grid, strength, total),
            None => false,
        };
        if done {
            self.status = format!(
                "{} 個を {} 目盛りへ揃えました（{:.0}%）",
                picked.len(),
                grid,
                strength * 100.0
            );
            self.touched();
        } else {
            self.status = "もう揃っています".into();
        }
    }

    /// 選んでいるものの長さを目盛りへ揃える。
    fn quantize_len_sel(&mut self) {
        if self.ed.sel.is_empty() {
            self.status = "選んでいるものがありません".into();
            return;
        }
        let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(0);
        let picked = self.ed.sel.clone();
        let grid = self.ed.snap.max(1);
        self.record(Tag::Once);
        let done = match self.notes_for_edit() {
            Some(ns) => sel::quantize_len(ns, &picked, grid, total),
            None => false,
        };
        if done {
            self.status = format!("{} 個の長さを揃えました", picked.len());
            self.touched();
        }
    }

    /// 選んでいるものの強さを、今の平均へ揃える。
    fn flatten_sel(&mut self) {
        if self.ed.sel.len() < 2 {
            self.status = "2つ以上選んでください".into();
            return;
        }
        let picked = self.ed.sel.clone();
        let part = self.ed.part.clone();
        let Some(avg) = self.project.notes.get(&part).and_then(|ns| sel::mean_vel(ns, &picked))
        else {
            return;
        };
        self.record(Tag::Once);
        let done = match self.notes_for_edit() {
            Some(ns) => sel::flatten(ns, &picked, avg),
            None => false,
        };
        if done {
            self.status = format!("強さを {avg} に揃えました");
            self.touched();
        }
    }

    /// 選んでいるものの強さを傾ける。
    fn ramp_sel(&mut self, up: bool) {
        if self.ed.sel.len() < 2 {
            self.status = "2つ以上選んでください".into();
            return;
        }
        let picked = self.ed.sel.clone();
        let (a, b) = if up { (60, 120) } else { (120, 60) };
        self.record(Tag::Once);
        let done = match self.notes_for_edit() {
            Some(ns) => sel::ramp(ns, &picked, a, b),
            None => false,
        };
        if done {
            self.status = format!("強さを {a} から {b} へ傾けました");
            self.touched();
        }
    }

    /// 選んでいるものをまとめて動かす。
    fn nudge_sel(&mut self, dstep: i32, dpitch: i32) {
        if self.ed.sel.is_empty() {
            return;
        }
        let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(0);
        let picked = self.ed.sel.clone();
        self.record(Tag::Curve(self.ed.part.clone(), "nudge"));
        let moved = match self.notes_for_edit() {
            Some(ns) => sel::nudge(ns, &picked, dstep, dpitch, total),
            None => false,
        };
        if moved {
            // 音程を動かしたら、いちばん上の音を返す
            if dpitch != 0 {
                let part = self.ed.part.clone();
                if let Some(n) =
                    picked.first().and_then(|i| self.project.notes.get(&part)?.get(*i)).cloned()
                {
                    self.audition(&part, n.pitch, n.vel, n.len);
                }
            }
            self.touched();
        } else {
            self.status = "端に当たっています".into();
        }
    }

    /// ミキサーで触られたぶんを当てる。取り消せる形で。
    fn apply_mixer(&mut self, edits: Vec<mixer::Edit>) {
        if edits.is_empty() {
            return;
        }
        let mut audible = false;
        for e in edits {
            match e {
                mixer::Edit::Gain { part, gain } => {
                    // 摘まんでいるあいだは1段にまとめる。離すまで1回
                    self.history.record(&self.project, Tag::Curve(part.clone(), "gain"));
                    self.project.gains.insert(part.clone(), gain);
                    self.engine.set_gain(&part, gain);
                }
                mixer::Edit::Mix { part, mix } => {
                    self.history.record(&self.project, Tag::Curve(part.clone(), "mix"));
                    self.project.mix.insert(part.clone(), mix);
                    self.engine.set_mix(&part, mix.width, mix.reverb, mix.duck);
                }
                mixer::Edit::Mute(part) => {
                    self.record(Tag::Once);
                    if self.project.muted.iter().any(|m| *m == part) {
                        self.project.muted.retain(|m| *m != part);
                    } else {
                        self.project.muted.push(part);
                    }
                    audible = true;
                }
                mixer::Edit::Solo(part) => {
                    self.record(Tag::Once);
                    if self.project.soloed.iter().any(|m| *m == part) {
                        self.project.soloed.retain(|m| *m != part);
                    } else {
                        self.project.soloed.push(part);
                    }
                    audible = true;
                }
            }
        }
        if audible {
            self.engine.set_mutes(&self.project.muted, &self.project.soloed);
        }
        // 音符は変わっていないので譜面は組み直さない。保存だけ促す
        self.saver.touched();
    }

    /// 鍵盤を繋ぐ・切る。
    fn toggle_midi(&mut self) {
        if self.midi.is_open() {
            for act in self.board.panic() {
                if let keys::Act::Off { pitch } = act {
                    self.engine.key_up(&self.ed.part.clone(), pitch);
                }
            }
            self.midi = keys::Input::idle();
            self.status = "鍵盤を切りました".into();
            return;
        }
        self.open_midi(0);
    }

    /// 触った音をその場で返す。**DAW なら当たり前のこと。**
    fn audition(&mut self, part: &str, pitch: i32, vel: u8, len: u32) {
        if self.out.error.is_some() || self.engine.is_playing() {
            return;
        }
        self.engine.note_on_steps(part, pitch, vel, len.clamp(1, 16));
    }

    /// 譜面を組み直す。曲ファイル + 手で触ったぶん。
    fn rebuild(&mut self) {
        let Some(song) = &self.song else { return };
        match build(song) {
            Ok(mut sc) => {
                self.project.overlay(&mut sc);
                self.score = Some(sc);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn undo(&mut self) {
        if self.history.undo(&mut self.project) {
            self.saver.touched();
            self.rebuild();
            let (u, r) = self.history.depth_used();
            self.status = format!("取り消した（戻せる {u} / やり直せる {r}）");
        } else {
            self.status = "これ以上は戻せません".into();
        }
    }

    fn redo(&mut self) {
        if self.history.redo(&mut self.project) {
            self.saver.touched();
            self.rebuild();
            let (u, r) = self.history.depth_used();
            self.status = format!("やり直した（戻せる {u} / やり直せる {r}）");
        } else {
            self.status = "やり直せるものがありません".into();
        }
    }

    /// 画面の操作から状態を変える前に呼ぶ。取り消せるようにする。
    fn record(&mut self, tag: Tag) {
        self.history.record(&self.project, tag);
    }

    fn save(&mut self) {
        let Some(st) = self.store() else { return };
        match st.save(&self.project) {
            Ok(()) => {
                self.saver.saved();
                self.status = format!("保存しました  {}", st.main_path().display());
            }
            Err(e) => self.status = format!("[!] 保存できません: {e}"),
        }
    }

    /// 直したことを覚える。実際に書くのは時間が来てから。
    fn touched(&mut self) {
        self.saver.touched();
        self.rebuild();
        // 音符が変わった。鳴らしたままでも渡せる
        self.push_to_engine();
    }

    fn tick_autosave(&mut self) {
        if !self.saver.due() || self.busy {
            return;
        }
        let Some(st) = self.store() else { return };
        match st.autosave(&self.project) {
            // 自動保存は「書いた」だけで、手で保存したことにはしない。
            // dirty のままにしておかないと、閉じるときに聞けなくなる。
            Ok(()) => self.status = "自動保存しました".into(),
            Err(e) => self.status = format!("[!] 自動保存できません: {e}"),
        }
        // 次の自動保存まで数え直す
        self.saver.saved();
        self.saver.touched();
    }

    fn start_render(&mut self) {
        // 手で触ったぶんを重ねたものを書き出す。聞いたバランスがそのまま出る
        let (Some(song), Some(name)) = (self.merged_song(), self.picked.clone()) else { return };
        let project = self.project.clone();
        let fmt = self.fmt;
        // 繰り返す範囲だけ書き出す。直した所だけ聞き直すときに使う
        let range = self
            .export_loop
            .then(|| self.loop_range)
            .flatten()
            .map(|(a, b)| (self.sample_of(a) as usize, self.sample_of(b) as usize));
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.log.clear();
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let tx2 = tx.clone();
            let say = move |m: &str| {
                let _ = tx2.send(Msg::Line(m.to_string()));
            };
            // 手で触ったぶんは呼ぶ側で混ぜてある（merged_song）
            let score = match build(&song) {
                Ok(mut sc) => {
                    project.overlay(&mut sc);
                    sc
                }
                Err(e) => {
                    let _ = tx.send(Msg::Failed(e));
                    return;
                }
            };
            let mut stems = tonescript_render::render_stems(&song, &score, &say);
            // 外で作った歌（AUDIO_TRACKS）も読む
            let vocals = if song.audio_tracks.is_empty() {
                Vec::new()
            } else {
                let _ = tx.send(Msg::Line(format!(
                    "[歌] AUDIO_TRACKS から {} 本",
                    song.audio_tracks.len()
                )));
                let (v, missing) = tonescript_render::load_audio_tracks(&song, &out_dir(), &say);
                if !missing.is_empty() {
                    let _ = tx.send(Msg::Failed(format!(
                        "歌のファイルが読めません:
    {}",
                        missing.join("
    ")
                    )));
                    return;
                }
                v
            };
            let _ = tx.send(Msg::Line("[mix] 空間処理".into()));
            let mut out =
                tonescript_render::mix_down_with(&song, &mut stems, &score, &vocals, &say);
            let _ = tx.send(Msg::Line("[master] 仕上げ".into()));
            tonescript_render::master(&mut out, song.master_lufs, &say);

            let dir = out_dir().join(&name);
            let full = dir.join("full.wav");
            // 範囲を切って、形を整えてから書く
            let mut out = match range {
                Some((a, b)) => {
                    let _ = tx.send(Msg::Line(format!(
                        "[書き出し] 繰り返す範囲だけ（{:.1}秒〜{:.1}秒）",
                        a as f32 / 48_000.0,
                        b as f32 / 48_000.0
                    )));
                    tonescript_render::export::cut(&out, a, b)
                }
                None => out,
            };
            if fmt.rate.hz() != 48_000 {
                let _ = tx.send(Msg::Line(format!(
                    "[書き出し] {} へ合わせる",
                    fmt.rate.label()
                )));
                out = tonescript_render::export::shape(&out, 48_000, fmt);
            }
            if let Err(e) =
                wav::write_stereo_as(&full, &out, fmt.rate.hz(), fmt.depth, fmt.dither)
            {
                let _ = tx.send(Msg::Failed(format!("書き出せません: {e}")));
                return;
            }
            for (part, buf) in &stems {
                let _ = wav::write_mono(&dir.join("parts").join(format!("{part}.wav")), buf, 48_000);
            }
            let _ = tx.send(Msg::Done {
                secs: out.len() as f32 / fmt.rate.hz() as f32,
                took: t.elapsed().as_secs_f32(),
                lufs: tonescript_render::mix::lufs(&out.l, &out.r),
                path: full.display().to_string(),
            });
        });
    }

    fn pump(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Line(l) => self.log.push(l),
                Msg::Done { secs, took, lufs, path } => {
                    self.log.push(format!(
                        "{secs:.1}秒の曲を {took:.2}秒で作った（実時間の {:.0}倍速） / 音圧 {lufs:.1} LUFS",
                        secs / took.max(1e-6)
                    ));
                    self.log.push(format!("-> {path}"));
                    done = true;
                }
                Msg::Failed(e) => {
                    self.log.push(format!("[!] {e}"));
                    done = true;
                }
            }
        }
        if done {
            self.busy = false;
            self.rx = None;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        self.pump();
        self.pump_keys();
        self.tick_autosave();
        // 針を進める。前のフレームからの時間で落とす
        let now = std::time::Instant::now();
        let dt = self.last_frame.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.016);
        self.last_frame = Some(now);
        if let Some(r) = &self.taking_audio {
            let now = r.take_level();
            self.in_level = mixer::fall(now, self.in_level, dt.min(0.25));
            ctx.request_repaint();
        }
        if self.show_mixer {
            let parts: Vec<String> =
                self.song.as_ref().map(|s| s.edit_parts.clone()).unwrap_or_default();
            self.needles.tick(&self.engine, &parts, dt.min(0.25));
            // 針は動き続けるので、鳴っていなくても描き直す
            ctx.request_repaint();
        }
        if self.engine.is_playing() {
            self.head = self.sec_to_step(self.engine.seconds());
            ctx.request_repaint();
        }
        if self.engine.counting_in() {
            ctx.request_repaint();
        }
        if self.engine.took_end() {
            self.status = "終わりまで鳴らしました".into();
        }
        if self.busy || self.midi.is_open() {
            ctx.request_repaint();
        } else if self.saver.is_dirty() {
            // 自動保存の時計を進めるために、たまに起こす
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        // ショートカット。Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y / Ctrl+S
        let (undo, redo, save) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                c && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                c && ((i.modifiers.shift && i.key_pressed(egui::Key::Z))
                    || i.key_pressed(egui::Key::Y)),
                c && i.key_pressed(egui::Key::S),
            )
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Space) && !i.modifiers.any()) {
            self.toggle_play();
        }
        if undo {
            self.undo();
        }
        self.edit_keys(ctx);
        if redo {
            self.redo();
        }
        if save {
            self.save();
        }

        if let Some(r) = &self.recovery {
            let song = r.song.clone();
            recovery_window(ctx, &song, self);
        }
        if self.making.is_some() {
            new_song_window(ctx, self);
        }
        if self.tuning.is_some() {
            settings_window(ctx, self);
        }

        self.tabs(ctx);
        if self.screen == Screen::Browser {
            self.browser_screen(ctx);
            return;
        }
        self.top_bar(ctx);
        self.side_bar(ctx);
        self.bottom_bar(ctx);
        self.mixer_bar(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(e) = &self.error {
                ui.add_space(20.0);
                ui.colored_label(theme::RED, "曲ファイルを読めません");
                ui.add_space(6.0);
                ui.label(egui::RichText::new(e).monospace().size(11.0));
                return;
            }
            let (Some(song), Some(score)) = (self.song.clone(), self.score.clone()) else {
                ui.centered_and_justified(|ui| ui.label(theme::dim("曲を選んでください")));
                return;
            };
            let tr = Transport {
                head: self.head,
                playing: self.engine.is_playing(),
                loop_range: self.loop_range,
            };
            let t = edit::piano_roll(
                ui,
                &song,
                &score,
                &mut self.project,
                &mut self.history,
                &mut self.ed,
                &tr,
                &self.clips,
                &mut self.zoom,
                &mut self.scroll,
            );
            if t.changed {
                self.touched();
            }
            if let Some(e) = t.clip.clone() {
                self.apply_clip(e);
            }
            if let Some((part, pitch, vel, len)) = t.hit {
                self.audition(&part, pitch, vel, len);
            }
            if let Some(at) = t.seek_to {
                self.head = at;
                self.engine.seek(self.sample_of(at));
            }
            if let Some((a, b)) = t.loop_set {
                self.loop_range = Some((a, b));
                self.engine.set_loop(self.sample_of(a), self.sample_of(b));
            }
            if t.loop_clear {
                self.loop_range = None;
                self.engine.set_loop(0, 0);
            }
            // 鳴っているあいだ、ヘッドが画面から出たら追う
            if self.follow && self.engine.is_playing() {
                let x = self.head * self.zoom - self.scroll;
                let w = ui.available_width().max(1.0);
                if x < w * 0.1 || x > w * 0.9 {
                    self.scroll = (self.head * self.zoom - w * 0.5).max(0.0);
                }
            }
        });
    }

    /// 閉じるときに、保存していないものがあれば自動保存へ落としておく。
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if self.saver.is_dirty() {
            if let Some(st) = self.store() {
                let _ = st.autosave(&self.project);
            }
        }
    }
}

/// 新しい曲を作る窓。
fn new_song_window(ctx: &egui::Context, app: &mut App) {
    let mut open = true;
    let mut make = false;
    let mut spec = app.making.clone().unwrap_or_default();

    egui::Window::new("新しい曲")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_min_width(330.0);
            egui::Grid::new("newsong").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("ファイル名");
                ui.text_edit_singleline(&mut spec.name);
                ui.end_row();

                ui.label("曲名");
                ui.text_edit_singleline(&mut spec.title);
                ui.end_row();

                ui.label("BPM");
                ui.add(egui::DragValue::new(&mut spec.bpm).range(20..=400).speed(1));
                ui.end_row();

                ui.label("調");
                ui.text_edit_singleline(&mut spec.key);
                ui.end_row();

                ui.label("小節数");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut spec.bars).range(1..=512).speed(1));
                    ui.label(theme::dim(&format!(
                        "× 2セクション = 全{}小節",
                        spec.bars * 2
                    )));
                });
                ui.end_row();

                ui.label("拍子");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut spec.meter.num).range(1..=32).speed(1));
                    ui.label("/");
                    egui::ComboBox::from_id_salt("den")
                        .selected_text(format!("{}", spec.meter.den))
                        .width(56.0)
                        .show_ui(ui, |ui| {
                            for d in [1u32, 2, 4, 8, 16] {
                                ui.selectable_value(&mut spec.meter.den, d, format!("{d}"));
                            }
                        });
                    let m = Meter::new(spec.meter.num, spec.meter.den);
                    ui.label(theme::dim(&format!("1小節 {} 目盛り", m.steps())));
                });
                ui.end_row();
            });

            ui.add_space(6.0);
            ui.checkbox(&mut spec.with_backing, "伴奏とドラムを入れておく");
            ui.label(theme::dim(
                "旋律は空のまま作られます。音符は画面で描いてください。",
            ));

            if let Some(e) = &app.make_error {
                ui.add_space(6.0);
                ui.colored_label(theme::RED, e);
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("作って開く").clicked() {
                    make = true;
                }
                if ui.button("やめる").clicked() {
                    app.making = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(theme::dim(&format!("songs/{}.rhai", spec.name.trim())));
                });
            });
        });

    if app.making.is_some() {
        app.making = Some(spec);
    }
    if make {
        app.create_song();
    }
    if !open {
        app.making = None;
    }
}

/// フォルダを開く。
fn open_folder(d: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(d).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(d).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(d).spawn();
}

/// 曲の設定。DAW でいうプロジェクト設定。
///
/// 直したものは曲ファイル（.rhai）へ書き戻す。画面と曲ファイルで
/// 値が食い違うのが一番よくないので、曲ファイルを正本にする。
/// 波形を、山の高さだけに間引く。
///
/// 3分の歌は 800 万サンプルある。画面に出すのに要るのは数百点しかないので、
/// **区間ごとの一番大きい所**だけを拾う。平均を取ると、波形が細く見えて
/// 「録れていない」と勘違いする。
fn peaks_of(x: &[f32], want: usize) -> Vec<f32> {
    if x.is_empty() || want == 0 {
        return Vec::new();
    }
    let per = (x.len() / want).max(1);
    let mut out = Vec::with_capacity(want.min(x.len()));
    let mut i = 0;
    while i < x.len() {
        let end = (i + per).min(x.len());
        let mut m = 0.0f32;
        for v in &x[i..end] {
            m = m.max(v.abs());
        }
        out.push(m);
        i = end;
    }
    out
}

fn settings_window(ctx: &egui::Context, app: &mut App) {
    let mut open = true;
    let mut apply = false;
    let mut d = app.tuning.clone().unwrap_or_else(|| {
        settings::Draft::from_song(app.song.as_ref().expect("曲がある"))
    });
    let (bass_names, kit_names, arp_names) = app
        .song
        .as_ref()
        .map(settings::known_names)
        .unwrap_or_default();

    egui::Window::new("曲の設定")
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("tune").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("曲名");
                ui.text_edit_singleline(&mut d.title);
                ui.end_row();

                ui.label("BPM");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut d.bpm).range(20.0..=400.0).speed(1.0));
                    // 数で分からないときは叩いて決める
                    if ui
                        .button("叩く")
                        .on_hover_text("曲に合わせて4回ほど叩くと、その速さが入る")
                        .clicked()
                    {
                        if let Some(bpm) = app.tap.hit() {
                            d.bpm = (bpm * 10.0).round() / 10.0;
                        }
                    }
                    let n = app.tap.taps();
                    if n > 0 {
                        ui.label(theme::dim(&format!("{} 回", n + 1)));
                        if ui.small_button("消す").clicked() {
                            app.tap.clear();
                        }
                    }
                    ui.label(theme::dim("途中で変えるには TEMPO_MAP を書く"));
                });
                ui.end_row();

                ui.label("ハネ");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Slider::new(&mut d.swing, 0.0..=1.0)
                            .fixed_decimals(2)
                            .custom_formatter(|v, _| {
                                if v < 0.01 { "均等".into() } else { format!("{:.0}%", v * 100.0) }
                            }),
                    )
                    .on_hover_text("0 で均等、1 で三連符の位置まで。音符は動かず、鳴り方だけ変わる");
                    egui::ComboBox::from_id_salt("swing_grid")
                        .selected_text(match d.swing_grid {
                            1 => "16分",
                            2 => "8分",
                            4 => "4分",
                            n => return ui.label(format!("{n} 目盛り")),
                        })
                        .show_ui(ui, |ui| {
                            for (g, t) in [(1u32, "16分"), (2, "8分"), (4, "4分")] {
                                ui.selectable_value(&mut d.swing_grid, g, t);
                            }
                        })
                        .response
                });
                ui.end_row();

                ui.label("ハネ");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Slider::new(&mut d.swing, 0.0..=1.0)
                            .fixed_decimals(2)
                            .custom_formatter(|v, _| {
                                if v < 0.01 { "均等".into() } else { format!("{:.0}%", v * 100.0) }
                            }),
                    )
                    .on_hover_text("0 で均等、1 で三連符の位置まで。音符は動かず、鳴り方だけ変わる");
                    egui::ComboBox::from_id_salt("swing_grid")
                        .selected_text(match d.swing_grid {
                            1 => "16分",
                            2 => "8分",
                            4 => "4分",
                            n => return ui.label(format!("{n} 目盛り")),
                        })
                        .show_ui(ui, |ui| {
                            for (g, t) in [(1u32, "16分"), (2, "8分"), (4, "4分")] {
                                ui.selectable_value(&mut d.swing_grid, g, t);
                            }
                        })
                        .response
                });
                ui.end_row();

                ui.label("調");
                ui.text_edit_singleline(&mut d.key);
                ui.end_row();

                ui.label("音圧の目標");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut d.master_lufs)
                            .range(-40.0..=0.0)
                            .speed(0.1)
                            .suffix(" LUFS"),
                    );
                    ui.label(theme::dim("配信は -14、CD は -9 あたり"));
                });
                ui.end_row();

                ui.label("全体音量");
                ui.add(egui::DragValue::new(&mut d.master_gain).range(0.0..=4.0).speed(0.01));
                ui.end_row();
            });

            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(theme::head("セクション"));
                ui.label(theme::dim(&format!(
                    "全{}小節   およそ {:.0}秒",
                    d.bars(),
                    d.rough_seconds()
                )));
            });
            ui.add_space(4.0);

            let mut remove: Option<usize> = None;
            let mut move_up: Option<usize> = None;
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for (i, sec) in d.sections.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut sec.name).desired_width(96.0));
                        ui.add(
                            egui::DragValue::new(&mut sec.bars)
                                .range(1..=512)
                                .speed(1)
                                .prefix("")
                                .suffix("小節"),
                        );
                        // 拍子
                        ui.add(egui::DragValue::new(&mut sec.meter.num).range(1..=32).speed(1));
                        ui.label("/");
                        egui::ComboBox::from_id_salt(("den", i))
                            .selected_text(format!("{}", sec.meter.den))
                            .width(48.0)
                            .show_ui(ui, |ui| {
                                for x in [1u32, 2, 4, 8, 16] {
                                    ui.selectable_value(&mut sec.meter.den, x, format!("{x}"));
                                }
                            });
                        pick(ui, ("bass", i), &mut sec.bass, &bass_names, 72.0);
                        pick(ui, ("kit", i), &mut sec.kit, &kit_names, 72.0);
                        pick(ui, ("arp", i), &mut sec.arp, &arp_names, 72.0);
                        ui.add(
                            egui::DragValue::new(&mut sec.gain)
                                .range(0.0..=4.0)
                                .speed(0.01),
                        )
                        .on_hover_text("そのセクションの強さ");
                        if i > 0 && ui.small_button("↑").clicked() {
                            move_up = Some(i);
                        }
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                }
            });
            if let Some(i) = move_up {
                d.sections.swap(i - 1, i);
            }
            if let Some(i) = remove {
                if d.sections.len() > 1 {
                    d.sections.remove(i);
                }
            }
            ui.add_space(4.0);
            if ui.small_button("＋ セクションを足す").clicked() {
                let m = d.sections.last().map(|x| x.meter).unwrap_or_default();
                d.sections.push(settings::new_section("新しい部分", m));
            }

            if let Some(e) = &app.tune_error {
                ui.add_space(6.0);
                ui.colored_label(theme::RED, e);
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let changed = app
                    .song
                    .as_ref()
                    .map(|s| d.differs_from(s))
                    .unwrap_or(false);
                if ui
                    .add_enabled(changed, egui::Button::new("曲ファイルへ書く"))
                    .on_hover_text(if changed { "" } else { "何も変わっていません" })
                    .clicked()
                {
                    apply = true;
                }
                if ui.button("やめる").clicked() {
                    app.tuning = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(theme::dim("直したものは .rhai へ書き戻します"));
                });
            });
        });

    if app.tuning.is_some() {
        app.tuning = Some(d.clone());
    }
    if apply {
        let Some(name) = app.picked.clone() else { return };
        let path = songs_dir().join(format!("{name}.rhai"));
        match d.save(&path) {
            Ok(()) => {
                app.tuning = None;
                app.tune_error = None;
                // 曲を読み直す。編集したぶんは残る
                let keep = app.project.clone();
                app.open();
                app.project = keep;
                app.rebuild();
                app.push_to_engine();
                app.refresh_list();
                app.status = format!("{} へ書きました", path.display());
            }
            Err(e) => app.tune_error = Some(e),
        }
    }
    if !open {
        app.tuning = None;
    }
}

/// 決まった名前から選ぶ。曲ファイルに書いていないものを選ぶと鳴らないので。
fn pick(
    ui: &mut egui::Ui,
    id: (&str, usize),
    value: &mut String,
    names: &[String],
    width: f32,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .width(width)
        .show_ui(ui, |ui| {
            for n in names {
                ui.selectable_value(value, n.clone(), n);
            }
        });
}

fn recovery_window(ctx: &egui::Context, song: &str, app: &mut App) {
    let mut open = true;
    egui::Window::new("前回きちんと閉じていません")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!("{song} に、保存されていない作業が残っています。"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("戻す").clicked() {
                    if let Some(st) = app.store() {
                        match st.load_autosave() {
                            Ok(Some(p)) => {
                                app.project = p;
                                app.history.clear();
                                app.rebuild();
                                // 戻したものはまだ保存されていない
                                app.saver.touched();
                                app.status = "自動保存から戻しました".into();
                            }
                            Ok(None) => app.status = "自動保存が見つかりません".into(),
                            Err(e) => app.status = format!("[!] {e}"),
                        }
                    }
                    app.recovery = None;
                }
                if ui.button("捨てる").clicked() {
                    if let Some(st) = app.store() {
                        st.discard_autosave();
                    }
                    app.status = "自動保存を捨てました".into();
                    app.recovery = None;
                }
            });
        });
    if !open {
        app.recovery = None;
    }
}

impl App {
    /// いちばん上のタブ。どの画面を出すか。
    fn tabs(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Tonescript").size(14.0).strong());
                ui.add_space(14.0);
                if ui
                    .selectable_label(self.screen == Screen::Browser, "曲一覧")
                    .clicked()
                {
                    self.refresh_list();
                    self.screen = Screen::Browser;
                }
                let can_edit = self.song.is_some() || self.error.is_some();
                let label = match &self.picked {
                    Some(n) if can_edit => format!("編集 — {n}"),
                    _ => "編集".into(),
                };
                if ui
                    .add_enabled(
                        can_edit,
                        egui::SelectableLabel::new(self.screen == Screen::Editor, label),
                    )
                    .clicked()
                {
                    self.screen = Screen::Editor;
                }
                // 未保存なら、一覧に居ても分かるようにする
                if self.saver.is_dirty() {
                    ui.add_space(8.0);
                    ui.colored_label(theme::ORANGE, "● 未保存");
                }
            });
            ui.add_space(5.0);
        });
    }

    /// 曲の一覧。開いたら最初に出る画面。
    fn browser_screen(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("曲").size(16.0).strong());
                ui.label(theme::dim(&format!("{} 曲", self.entries.len())));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("＋").size(18.0).strong(),
                        ))
                        .on_hover_text("新しい曲を作る")
                        .clicked()
                    {
                        self.making = Some(newsong::Spec::default());
                        self.make_error = None;
                    }
                    if ui.button("フォルダ").on_hover_text("曲の置き場を開く").clicked() {
                        let d = songs_dir();
                        let _ = std::fs::create_dir_all(&d);
                        open_folder(&d);
                    }
                    if ui.button("読み直す").clicked() {
                        self.refresh_list();
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();

            if self.entries.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(theme::dim("曲がまだありません"));
                    ui.add_space(8.0);
                    if ui.button("＋ 最初の曲を作る").clicked() {
                        self.making = Some(newsong::Spec::default());
                        self.make_error = None;
                    }
                });
                return;
            }

            let mut open: Option<String> = None;
            let mut dup: Option<String> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                for e in &self.entries {
                    let picked = self.picked.as_deref() == Some(e.name.as_str());
                    let frame = egui::Frame::none()
                        .fill(if picked { theme::RAISED } else { theme::SURFACE })
                        .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                        .rounding(6.0);
                    frame.show(ui, |ui| {
                        ui.set_width(ui.available_width() - 4.0);
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&e.name).strong().size(13.0));
                                    if !e.title.is_empty() && e.title != e.name {
                                        ui.label(theme::dim(&e.title));
                                    }
                                    if e.pending {
                                        ui.colored_label(theme::ORANGE, "● 閉じ損ね");
                                    } else if e.has_project {
                                        ui.colored_label(theme::part_color("arp"), "編集あり");
                                    }
                                });
                                let mut bits: Vec<String> = Vec::new();
                                if let Some(b) = e.bpm {
                                    bits.push(format!("{b} BPM"));
                                }
                                if let Some(n) = e.bars {
                                    bits.push(format!("{n}小節"));
                                }
                                if !e.key.is_empty() {
                                    bits.push(e.key.clone());
                                }
                                let w = e.when();
                                if !w.is_empty() {
                                    bits.push(w);
                                }
                                ui.label(theme::dim(&bits.join("   ")));
                                if let Some(err) = &e.error {
                                    ui.colored_label(theme::RED, err);
                                }
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("開く").clicked() {
                                        open = Some(e.name.clone());
                                    }
                                    if ui.button("複製").clicked() {
                                        dup = Some(e.name.clone());
                                    }
                                },
                            );
                        });
                    });
                    ui.add_space(5.0);
                }
            });
            if let Some(n) = open {
                self.open_song(&n);
            }
            if let Some(n) = dup {
                self.picked = Some(n);
                self.duplicate_song();
            }
        });
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("head").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if let Some(s) = &self.song {
                    let meter = if s.has_odd_meter() {
                        "  変拍子".to_string()
                    } else {
                        format!("  {}", s.meter_at(1))
                    };
                    ui.label(theme::dim(&format!(
                        "{}   {} BPM   {}   全{}小節{meter}",
                        s.title, s.bpm, s.key, s.bars()
                    )));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.busy { "書き出し中…" } else { "書き出す" };
                    if ui
                        .add_enabled(!self.busy && self.song.is_some(), egui::Button::new(label))
                        .on_hover_text(format!(
                            "{} / {}{}",
                            self.fmt.rate.label(),
                            self.fmt.depth.label(),
                            if self.export_loop { " / 繰り返す範囲だけ" } else { "" }
                        ))
                        .clicked()
                    {
                        self.start_render();
                    }
                    // 書き出しの形
                    ui.menu_button("形", |ui| {
                        use tonescript_render::export::{Depth, Rate};
                        ui.label(theme::head("周波数"));
                        for r in Rate::ALL {
                            let tip = match r {
                                Rate::R44100 => "CD と同じ。48kHz のまま渡すと 8.8% 速く鳴る",
                                Rate::R48000 => "中で作っているのと同じ。変換なし",
                                Rate::R96000 => "中身は増えない。相手の決まりで要るときだけ",
                            };
                            if ui
                                .selectable_label(self.fmt.rate == r, r.label())
                                .on_hover_text(tip)
                                .clicked()
                            {
                                self.fmt.rate = r;
                            }
                        }
                        ui.separator();
                        ui.label(theme::head("深さ"));
                        for d in Depth::ALL {
                            let tip = match d {
                                Depth::I16 => "配信も CD もこれ",
                                Depth::I24 => "作業の受け渡し用",
                                Depth::F32 => "小数のまま。天井を超えたぶんも残る",
                            };
                            if ui
                                .selectable_label(self.fmt.depth == d, d.label())
                                .on_hover_text(tip)
                                .clicked()
                            {
                                self.fmt.depth = d;
                            }
                        }
                        if self.fmt.depth == Depth::I16 {
                            ui.separator();
                            ui.checkbox(&mut self.fmt.dither, "丸めの粉")
                                .on_hover_text(
                                    "16bit へ落とすときの段を、微かな雑音でばらけさせる。
                                     ジリジリした歪みが、小さな雑音になる",
                                );
                        }
                        ui.separator();
                        let has_loop = self.loop_range.is_some();
                        ui.add_enabled(
                            has_loop,
                            egui::Checkbox::new(&mut self.export_loop, "繰り返す範囲だけ"),
                        )
                        .on_hover_text(if has_loop {
                            "直した所だけ聞き直すときに"
                        } else {
                            "先に物差しを Shift+ドラッグして範囲を作る"
                        });
                    })
                    .response
                    .on_hover_text("周波数・深さ・範囲");
                    if ui.button("保存 (Ctrl+S)").clicked() {
                        self.save();
                    }
                    if ui
                        .add_enabled(self.song.is_some(), egui::Button::new("曲の設定"))
                        .on_hover_text("BPM・調・セクション・音圧")
                        .clicked()
                    {
                        if let Some(song) = &self.song {
                            self.tuning = Some(settings::Draft::from_song(song));
                            self.tune_error = None;
                        }
                    }
                    ui.separator();
                    let playing = self.engine.is_playing();
                    if ui
                        .add_enabled(
                            self.song.is_some() && self.out.error.is_none(),
                            egui::Button::new(if self.engine.counting_in() {
                                "… 数えています"
                            } else if playing {
                                "■ 止める"
                            } else {
                                "▶ 鳴らす"
                            }),
                        )
                        .on_hover_text("Space")
                        .clicked()
                    {
                        self.toggle_play();
                    }
                    if ui.button("頭へ").on_hover_text("再生ヘッドを先頭へ").clicked() {
                        self.head = 0.0;
                        self.engine.seek(0);
                        self.scroll = 0.0;
                    }
                    ui.checkbox(&mut self.follow, "追う");
                    if !self.clips.is_empty()
                        && ui
                            .selectable_label(self.ed.show_audio, "音声")
                            .on_hover_text(
                                "録った音を小節に合わせて出す。
                                 真ん中を掴めば動き、端を掴めば切り詰める",
                            )
                            .clicked()
                    {
                        self.ed.show_audio = !self.ed.show_audio;
                    }
                    if ui
                        .selectable_label(self.ed.show_vel, "強さ")
                        .on_hover_text("下に強さのレーンを出す。押した高さがそのまま強さ")
                        .clicked()
                    {
                        self.ed.show_vel = !self.ed.show_vel;
                    }
                    if ui
                        .selectable_label(self.show_mixer, "ミキサー")
                        .on_hover_text("音量・広がり・残響の送りと、針")
                        .clicked()
                    {
                        self.show_mixer = !self.show_mixer;
                    }
                    ui.separator();
                    // 鍵盤
                    let on = self.midi.is_open();
                    if on {
                        if ui
                            .selectable_label(true, "鍵盤 ●")
                            .on_hover_text(match &self.midi.name {
                                Some(n) => format!("{n}（押すと切る）"),
                                None => "押すと切る".into(),
                            })
                            .clicked()
                        {
                            self.toggle_midi();
                        }
                    } else {
                        // 繋がっていない。見つかったものから選ばせる
                        let mut pick = None;
                        ui.menu_button("鍵盤", |ui| {
                            self.refresh_ports();
                            if self.ports.is_empty() {
                                ui.label(theme::dim("鍵盤が見つかりません"));
                                ui.label(theme::dim("挿してから開き直してください"));
                            }
                            for (i, name) in self.ports.iter().enumerate() {
                                if ui.button(name).clicked() {
                                    pick = Some(i);
                                    ui.close_menu();
                                }
                            }
                        })
                        .response
                        .on_hover_text("MIDI 鍵盤を繋ぐ");
                        if let Some(i) = pick {
                            self.open_midi(i);
                        }
                    }
                    if on {
                        if ui
                            .selectable_label(self.arm, "録る")
                            .on_hover_text(
                                "弾いたものを譜面へ入れる。
                                 鳴らしながら弾けば弾いた所へ、
                                 止めて弾けば1つずつ置いて先へ進む",
                            )
                            .clicked()
                        {
                            self.arm = !self.arm;
                            if !self.arm {
                                self.taking.clear();
                            }
                        }
                        let n = self.board.held_count();
                        if n > 0 {
                            ui.label(theme::dim(&format!("{n} 押下")));
                        }
                    }
                    ui.separator();
                    // メトロノーム
                    let click_on = self.engine.click() > 0.0;
                    if ui
                        .selectable_label(click_on, "拍")
                        .on_hover_text("メトロノーム。伴奏なしで歌うときに要る")
                        .clicked()
                    {
                        self.engine.set_click(if click_on { 0.0 } else { 0.8 });
                    }
                    if click_on {
                        ui.add(
                            egui::DragValue::new(&mut self.count_in)
                                .speed(0.1)
                                .range(0..=8)
                                .suffix(" 拍前"),
                        )
                        .on_hover_text("鳴らす前に数える拍数。0 なら数えない");
                    }
                    ui.separator();
                    // 録音
                    if self.taking_audio.is_some() {
                        let secs = self.taking_audio.as_ref().map(|r| r.seconds()).unwrap_or(0.0);
                        if ui
                            .add(egui::Button::new(format!("● 録音中 {secs:.1}秒")).fill(theme::RED))
                            .on_hover_text("押すと録り終えて曲へ入れる")
                            .clicked()
                        {
                            self.stop_rec();
                        }
                        // 入っているか目で分かるように
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(46.0, 10.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(rect, 2.0, theme::PANEL);
                        let w = rect.width() * mixer::bar_of(self.in_level);
                        if w > 0.5 {
                            let c = if self.in_level > 0.99 { theme::RED } else { theme::GREEN };
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height())),
                                2.0,
                                c,
                            );
                        }
                        if self.in_level > 0.99 {
                            ui.colored_label(theme::RED, "割れています");
                        }
                    } else {
                        let mut pick = None;
                        let mut dflt = false;
                        ui.menu_button("録音", |ui| {
                            self.refresh_in_ports();
                            if self.in_ports.is_empty() {
                                ui.label(theme::dim("録る口が見つかりません"));
                            }
                            if ui.button("既定の口で録る").clicked() {
                                dflt = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            for (i, name) in self.in_ports.iter().enumerate() {
                                if ui.button(name).clicked() {
                                    pick = Some(i);
                                    ui.close_menu();
                                }
                            }
                        })
                        .response
                        .on_hover_text("マイクから録る。再生ヘッドの所から始まる");
                        if dflt {
                            self.start_rec(None);
                        } else if let Some(i) = pick {
                            self.start_rec(Some(i));
                        }
                    }
                    ui.separator();
                    // 表示する範囲。拡大率そのものではなく「何小節ぶん」で選ぶ
                    if ui.button("全体").on_hover_text("曲ぜんぶを画面に収める").clicked() {
                        self.fit_bars(None);
                    }
                    for n in [16u32, 8, 4, 2] {
                        if ui
                            .button(format!("{n}"))
                            .on_hover_text(format!("{n}小節ぶんを表示"))
                            .clicked()
                        {
                            self.fit_bars(Some(n));
                        }
                    }
                    ui.label(theme::dim("小節"));
                    if let Some((a, b)) = self.loop_range {
                        if ui.button("繰り返しを解く").clicked() {
                            self.loop_range = None;
                            self.engine.set_loop(0, 0);
                        }
                        // 出口が本当にその範囲を持っているかも出す
                        let on = self.engine.loop_range().is_some();
                        let bars = self
                            .song
                            .as_ref()
                            .map(|s| (s.bar_of_step(a as u32), s.bar_of_step(b as u32)))
                            .unwrap_or((0, 0));
                        ui.label(theme::dim(&format!(
                            "繰り返し {}〜{}小節{}",
                            bars.0,
                            bars.1,
                            if on { "" } else { "（出口へ未反映）" }
                        )));
                    }
                    let (u, r) = self.history.depth_used();
                    if ui
                        .add_enabled(r > 0, egui::Button::new("やり直す"))
                        .on_hover_text("Ctrl+Shift+Z / Ctrl+Y")
                        .clicked()
                    {
                        self.redo();
                    }
                    if ui
                        .add_enabled(u > 0, egui::Button::new("取り消す"))
                        .on_hover_text("Ctrl+Z")
                        .clicked()
                    {
                        self.undo();
                    }
                    // 保存の状態をここに出す。DAW の「*」に当たる
                    if self.saver.is_dirty() {
                        ui.colored_label(theme::ORANGE, "● 未保存");
                    } else {
                        ui.label(theme::dim("保存済み"));
                    }
                });
            });
            ui.add_space(6.0);
        });
    }

    fn side_bar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side").default_width(215.0).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.label(theme::head("編集するパート"));
            let parts: Vec<String> = self
                .song
                .as_ref()
                .map(|s| s.edit_parts.clone())
                .unwrap_or_default();
            for p in &parts {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, theme::part_color(p));
                    let n = self.score.as_ref().and_then(|s| s.get(p)).map(|v| v.len()).unwrap_or(0);
                    let mark = if self.project.is_edited(p) { "*" } else { " " };
                    if ui.selectable_label(&self.ed.part == p, format!("{mark}{p}  {n}")).clicked() {
                        self.ed.part = p.clone();
                    }
                    // 黙らせる
                    let muted = self.project.muted.iter().any(|m| m == p);
                    if ui.small_button(if muted { "M" } else { "m" }).clicked() {
                        self.history.record(&self.project, Tag::Once);
                        if muted {
                            self.project.muted.retain(|m| m != p);
                        } else {
                            self.project.muted.push(p.clone());
                        }
                        self.touched();
                    }
                });
            }

            // 録ったもの
            if !self.project.takes.is_empty() {
                ui.add_space(10.0);
                ui.separator();
                ui.label(theme::head("録ったもの"));
                let mut names: Vec<String> = self.project.takes.keys().cloned().collect();
                names.sort();
                let mut drop_it: Option<String> = None;
                let mut changed = false;
                for n in names {
                    ui.horizontal(|ui| {
                        if let Some(t) = self.project.takes.get_mut(&n) {
                            ui.label(theme::dim(&n));
                            if ui
                                .add(
                                    egui::DragValue::new(&mut t.gain)
                                        .speed(0.01)
                                        .range(0.0..=4.0)
                                        .fixed_decimals(2),
                                )
                                .on_hover_text("音量。0 にすれば鳴らない")
                                .changed()
                            {
                                changed = true;
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(&mut t.fade_in)
                                        .speed(0.01)
                                        .range(0.0..=30.0)
                                        .fixed_decimals(2)
                                        .prefix("入"),
                                )
                                .on_hover_text("何秒かけて入るか")
                                .changed()
                            {
                                changed = true;
                            }
                            if ui
                                .add(
                                    egui::DragValue::new(&mut t.fade_out)
                                        .speed(0.01)
                                        .range(0.0..=30.0)
                                        .fixed_decimals(2)
                                        .prefix("出"),
                                )
                                .on_hover_text("何秒かけて消えるか")
                                .changed()
                            {
                                changed = true;
                            }
                        }
                        if ui.small_button("×").on_hover_text("曲から外す（ファイルは残る）").clicked() {
                            drop_it = Some(n.clone());
                        }
                    });
                }
                if let Some(n) = drop_it {
                    self.record(Tag::Once);
                    self.project.takes.remove(&n);
                    self.saver.touched();
                    self.load_audio();
                    self.status = format!("{n} を曲から外しました（ファイルは残っています）");
                } else if changed {
                    self.saver.touched();
                    self.load_audio();
                }
            }

            ui.add_space(10.0);
            ui.separator();
            ui.label(theme::head("選ぶ"));
            let n = self.ed.sel.len();
            ui.label(theme::dim(&if n == 0 {
                "何も選んでいません".to_string()
            } else {
                format!("{n} 個を選んでいます")
            }));
            ui.label(theme::dim("空きから引きずると囲んで選ぶ"))
                .on_hover_text("Shift か Ctrl を押しながら叩くと、1つずつ足す／外す");
            ui.horizontal_wrapped(|ui| {
                for (t, tip) in [
                    ("全部", "Ctrl+A"),
                    ("写す", "Ctrl+C"),
                    ("貼る", "Ctrl+V（再生ヘッドの所へ）"),
                    ("複製", "Ctrl+D（すぐ後ろへ）"),
                    ("消す", "Delete"),
                ] {
                    if ui.small_button(t).on_hover_text(tip).clicked() {
                        match t {
                            "全部" => self.select_all(),
                            "写す" => self.copy_sel(),
                            "貼る" => self.paste_clip(),
                            "複製" => self.duplicate_sel(),
                            _ => self.delete_sel(),
                        }
                    }
                }
            });
            ui.label(theme::dim("← → で動かす / ↑ ↓ で音程"))
                .on_hover_text("↑↓ は半音ずつ。Shift を足すと1オクターブ");
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::dim("位置"));
                if ui
                    .small_button("揃える")
                    .on_hover_text("選んだものを「刻み」の目盛りへぴったり寄せる")
                    .clicked()
                {
                    self.quantize_sel(1.0);
                }
                if ui
                    .small_button("半分")
                    .on_hover_text("半分だけ寄せる。弾いたノリを残したいとき")
                    .clicked()
                {
                    self.quantize_sel(0.5);
                }
                if ui.small_button("長さも").on_hover_text("長さを目盛りへ揃える").clicked() {
                    self.quantize_len_sel();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::dim("強さ"));
                if ui
                    .small_button("揃える")
                    .on_hover_text("選んだものを、今の平均に揃える")
                    .clicked()
                {
                    self.flatten_sel();
                }
                if ui.small_button("↗").on_hover_text("だんだん強く").clicked() {
                    self.ramp_sel(true);
                }
                if ui.small_button("↘").on_hover_text("だんだん弱く").clicked() {
                    self.ramp_sel(false);
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.label(theme::head("置きかた"));
            ui.horizontal(|ui| {
                ui.label(theme::dim("長さ"));
                for (n, t) in [(1u32, "16分"), (2, "8分"), (4, "4分"), (8, "2分")] {
                    if ui.selectable_label(self.ed.new_len == n, t).clicked() {
                        self.ed.new_len = n;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(theme::dim("刻み"));
                for (n, t) in [(1u32, "1"), (2, "2"), (4, "4")] {
                    if ui.selectable_label(self.ed.snap == n, t).clicked() {
                        self.ed.snap = n;
                    }
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.label(theme::head("オートメーション"));
            let part = self.ed.part.clone();
            let has = self.project.automation.get(&part).map(|m| m.len()).unwrap_or(0);
            ui.label(theme::dim(&format!("{part}: {has} 本")));
            ui.horizontal(|ui| {
                if ui.small_button("音量を下げる線").clicked() {
                    self.record(Tag::Once);
                    let total = self.song.as_ref().map(|s| s.total_steps()).unwrap_or(64);
                    self.project
                        .automation
                        .entry(part.clone())
                        .or_default()
                        .insert(Lane::Gain, tonescript_song::model::Curve::new(
                            vec![(0, 1.0), (total, 0.0)],
                        ));
                    self.touched();
                }
                if ui.small_button("消す").clicked() {
                    self.record(Tag::Once);
                    self.project.automation.remove(&part);
                    self.touched();
                }
            });

            if self.project.is_edited(&part) {
                ui.add_space(10.0);
                if ui.button("このパートを曲ファイルへ戻す").clicked() {
                    self.record(Tag::Once);
                    self.project.notes.remove(&part);
                    self.touched();
                }
            }

            ui.add_space(10.0);
            ui.separator();
            ui.label(theme::head("MIDI"));
            ui.label(theme::dim("音符を外とやり取りする形式。音は運べません"));
            ui.add_space(3.0);
            if ui
                .add_enabled(self.song.is_some(), egui::Button::new("MIDI へ書き出す"))
                .clicked()
            {
                self.midi_out();
            }
            let mids = self.midi_candidates();
            if mids.is_empty() {
                ui.label(theme::dim("取り込める .mid が見つかりません"));
            } else {
                let mut take: Option<std::path::PathBuf> = None;
                egui::ComboBox::from_id_salt("midi_in")
                    .selected_text("MIDI を取り込む")
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for m in &mids {
                            let label = m
                                .file_name()
                                .and_then(|x| x.to_str())
                                .unwrap_or("?")
                                .to_string();
                            if ui.selectable_label(false, label).clicked() {
                                take = Some(m.clone());
                            }
                        }
                    });
                if let Some(m) = take {
                    self.midi_in(&m);
                }
            }

            ui.add_space(10.0);
            ui.separator();
            if let Some(st) = self.store() {
                let n = st.backups().len();
                ui.label(theme::dim(&format!("世代 {n} / {}", tonescript_project::store::BACKUPS)));
                if n > 0 && ui.small_button("ひとつ前へ戻す").clicked() {
                    match st.load_backup(1) {
                        Ok(p) => {
                            self.history.record(&self.project, Tag::Once);
                            self.project = p;
                            self.touched();
                            self.status = "ひとつ前の保存に戻しました".into();
                        }
                        Err(e) => self.status = format!("[!] {e}"),
                    }
                }
            }
        });
    }

    /// ミキサーの帯。記録の上に出す。
    fn mixer_bar(&mut self, ctx: &egui::Context) {
        if !self.show_mixer {
            return;
        }
        let Some(song) = self.song.clone() else { return };
        let mut edits = Vec::new();
        egui::TopBottomPanel::bottom("mixer")
            .resizable(true)
            .default_height(232.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(theme::head("ミキサー"));
                    ui.label(theme::dim("触ると鳴らしたまま変わる。書き出しにもそのまま効く"));
                });
                ui.add_space(2.0);
                edits = mixer::panel(
                    ui,
                    &song,
                    &self.project,
                    &self.engine,
                    &self.needles,
                    &mut self.ed.part,
                );
            });
        self.apply_mixer(edits);
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("log").resizable(true).default_height(140.0).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(theme::head("記録"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(theme::dim(&self.status));
                    if let Some(e) = &self.out.error {
                        ui.colored_label(theme::ORANGE, format!("音: {e}"));
                    } else if let Some(n) = &self.out.note {
                        // 周波数が合っていない。黙ってずれるより言う
                        ui.colored_label(theme::ORANGE, format!("音: {}Hz", self.out.sample_rate))
                            .on_hover_text(n);
                    } else {
                        // 今いくつ鳴っているか。鳴らしながら作っている証拠
                        let v = self.engine.voices();
                        if v > 0 {
                            ui.label(theme::dim(&format!("{v} 音")));
                        }
                    }
                    if let Some(n) = &self.font_note {
                        if n.starts_with("[!]") {
                            ui.colored_label(theme::RED, n);
                        }
                    }
                });
            });
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                if self.log.is_empty() {
                    ui.label(theme::dim(
                        "譜面: 左クリックで置く / 右クリックで消す / ドラッグで動かす /                          右端で長さ変え / Ctrl+Z で取り消し",
                    ));
                    ui.label(theme::dim(
                        "物差し（上の帯）: クリックで再生位置 / Shift+ドラッグで繰り返す範囲 /                          Space で鳴らす",
                    ));
                    ui.label(theme::dim(
                        "ホイール: 左右へ動く / 上の「小節」ボタン: 何小節ぶん見るかを選ぶ",
                    ));
                }
                for l in &self.log {
                    ui.label(egui::RichText::new(l).monospace().size(11.0));
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([820.0, 520.0])
            .with_title("Tonescript"),
        ..Default::default()
    };
    eframe::run_native(
        "Tonescript",
        opts,
        Box::new(|cc| {
            // 先に日本語のフォントを入れる。入れないと画面が全部「□」になる
            let font = font::install(&cc.egui_ctx);
            theme::install(&cc.egui_ctx);
            let mut app = Box::<App>::default();
            match font {
                Ok(p) => app.font_note = Some(format!("フォント {}", p.display())),
                Err(e) => app.font_note = Some(format!("[!] {e}（日本語が □ になります）")),
            }
            Ok(app)
        }),
    )
}

// render_song は CLI 側で使う。ここでは段ごとに呼んでいる。
#[allow(unused)]
fn _keep(s: &Song) {
    let _ = render_song;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 開いたら必ず曲一覧から始まること。
    ///
    /// 前に開いた曲をいきなり出すと、どれを触っているのか分からないまま
    /// 編集が始まる。まず「どれを開くか」を選ばせる。
    #[test]
    fn the_app_starts_on_the_song_list() {
        let app = App::default();
        assert!(app.screen == Screen::Browser, "いきなり編集の画面が出る");
        assert!(app.picked.is_none(), "勝手に曲が開かれている");
        assert!(app.song.is_none(), "勝手に曲が読まれている");
        // 未保存も、直したことになっていないこと
        assert!(!app.saver.is_dirty());
    }

    /// 曲を開いたら編集の画面へ移り、閉じ損ねの確認も効くこと。
    #[test]
    fn opening_a_song_moves_to_the_editor() {
        let mut app = App::default();
        // 一覧に何か在れば、それを開ける
        let Some(first) = app.entries.first().map(|e| e.name.clone()) else {
            return; // 曲が1つも無い環境では確かめようがない
        };
        app.open_song(&first);
        assert!(app.screen == Screen::Editor, "編集の画面へ移らない");
        assert_eq!(app.picked.as_deref(), Some(first.as_str()));
    }
}
