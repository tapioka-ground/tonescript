//! ピアノロールの中身。見る所と、触る所。
//!
//! Python 版（editor.py）にあった操作のうち、置く・動かす・長さを変える・
//! 消す、までをここに入れてある。手描きの楽器アイコンや細かい表示は
//! まだ移していない。
//!
//! 画面の座標と譜面の位置を行き来する所を1つにまとめてある。
//! ここが散らばると、拍子が混ざったときに必ずどこかがずれる。

use crate::theme;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use tonescript_project::{History, Project, Tag};
use tonescript_render::Score;
use tonescript_song::model::Note;
use tonescript_song::Song;

/// 画面と譜面の対応。
pub struct View {
    /// 1目盛りを何ピクセルで描くか
    pub zoom: f32,
    /// 左端が譜面のどこか（ピクセル）
    pub scroll: f32,
    /// いちばん上に描く音程
    pub top_pitch: i32,
    pub row_h: f32,
    pub rect: Rect,
}

impl View {
    pub fn x_of(&self, step: f32) -> f32 {
        self.rect.left() + step * self.zoom - self.scroll
    }
    pub fn y_of(&self, pitch: i32) -> f32 {
        self.rect.top() + (self.top_pitch - pitch) as f32 * self.row_h
    }
    /// 画面の位置を譜面の (目盛り, 音程) へ。
    pub fn step_at(&self, x: f32) -> f32 {
        (x - self.rect.left() + self.scroll) / self.zoom
    }
    pub fn pitch_at(&self, y: f32) -> i32 {
        self.top_pitch - ((y - self.rect.top()) / self.row_h).floor() as i32
    }
}

/// 掴んでいるもの。
#[derive(Clone, Debug, PartialEq)]
pub enum Grab {
    None,
    /// 動かしている（掴んだ位置からのずれを覚えておく）
    Move { part: String, index: usize, offset: f32 },
    /// 右端を掴んで長さを変えている
    Resize { part: String, index: usize },
}

/// 編集の設定と途中の状態。
pub struct Editor {
    pub part: String,
    /// 新しく置く音符の長さ（目盛り）
    pub new_len: u32,
    /// 置くときに合わせる刻み
    pub snap: u32,
    pub grab: Grab,
    /// 直前に触った音符。色を変えて分かるようにする
    pub selected: Option<(String, usize)>,
    /// 譜面の見えている幅（ピクセル）。ボタンが拡大率を出すのに使う
    pub view_w: f32,
    /// 物差しを引きずっている最中か
    pub scrubbing: bool,
    /// 繰り返す範囲を作っている最中の始点（目盛り）
    pub loop_from: Option<f32>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            part: "lead".into(),
            new_len: 4,
            snap: 1,
            grab: Grab::None,
            selected: None,
            view_w: 800.0,
            scrubbing: false,
            loop_from: None,
        }
    }
}

impl Editor {
    fn snapped(&self, step: f32) -> u32 {
        let s = self.snap.max(1) as f32;
        ((step / s).floor() * s).max(0.0) as u32
    }
}

/// ホイール1目盛りで何ピクセル動かすか。
///
/// マウスのホイールは1回で 50〜120 くらい送ってくる。等倍だと飛びすぎるので
/// 落とす。タッチパッドは小さい値を連続で送ってくるので、そのまま効く。
const WHEEL_SPEED: f32 = 1.4;

/// 1目盛りを何ピクセルまで許すか。
///
/// 下限は「16分が 1px」。これより縮めると音符が線にもならない。
/// 上限は「16分が 80px」。1小節で画面が埋まるくらい。
pub const MIN_ZOOM: f32 = 1.0;
pub const MAX_ZOOM: f32 = 80.0;

/// ホイールの扱い。左右へ動かすだけ。
///
/// 画面を触らずに確かめられるよう、状態を持たない形にしてある。
///
/// 拡大縮小はここではやらない。「何小節ぶん見るか」を選ぶほうが、
/// 見たい形に一発で合う（`zoom_for_bars` を見ること）。
pub fn wheel(scroll: &mut f32, dy: f32, dx: f32) {
    // 手前へ回したら先へ進む。上下の送りも左右へ回す
    //（縦のホイールしか無いマウスでも横へ動かせるように）
    *scroll += (-dy - dx) * WHEEL_SPEED;
}

/// 「n 小節ぶんが画面に収まる」拡大率。
///
/// 拡大率そのものを触るのではなく、見たい単位で決める。
/// 拍子が混ざっていても、実際にその小節たちが占める目盛り数から出す
/// （3/4 と 7/8 が混ざれば、同じ「4小節」でも幅が違う）。
///
/// `from_bar` は画面の左端にある小節。そこから n 小節ぶんを測る。
pub fn zoom_for_bars(song: &Song, from_bar: u32, n: u32, width: f32) -> f32 {
    let bars = song.bars().max(1);
    let from = from_bar.clamp(1, bars);
    let to = from.saturating_add(n).min(bars + 1);
    let a = song.bar_start(from);
    let b = if to > bars { song.total_steps() } else { song.bar_start(to) };
    let steps = b.saturating_sub(a).max(1) as f32;
    (width.max(1.0) / steps).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// 曲まるごとが収まる拡大率。
pub fn zoom_for_whole(song: &Song, width: f32) -> f32 {
    let steps = song.total_steps().max(1) as f32;
    (width.max(1.0) / steps).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// 触った結果。画面側が「直った」と分かるように返す。
#[derive(Default)]
pub struct Touched {
    pub changed: bool,
    /// 再生ヘッドをここへ動かしてほしい（目盛り）
    pub seek_to: Option<f32>,
    /// 繰り返す範囲が決まった（目盛り）
    pub loop_set: Option<(f32, f32)>,
    /// 繰り返しを解いてほしい
    pub loop_clear: bool,
}

/// 再生の様子。画面から渡してもらう。
pub struct Transport {
    /// 今どこを鳴らしているか（目盛り）。止まっていても位置は持つ
    pub head: f32,
    pub playing: bool,
    /// 繰り返す範囲（目盛り）
    pub loop_range: Option<(f32, f32)>,
}

/// ピアノロールを描いて、触りを受け取る。
#[allow(clippy::too_many_arguments)]
pub fn piano_roll(
    ui: &mut egui::Ui,
    song: &Song,
    score: &Score,
    project: &mut Project,
    history: &mut History,
    ed: &mut Editor,
    tr: &Transport,
    zoom: &mut f32,
    scroll: &mut f32,
) -> Touched {
    let mut out = Touched::default();
    let total_steps = song.total_steps() as f32;
    let (resp, p) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
    let whole = resp.rect;
    p.rect_filled(whole, 0.0, theme::BASE);
    // 上を物差しに使い、その下を譜面にする
    let ruler = Rect::from_min_max(whole.min, Pos2::new(whole.max.x, whole.min.y + RULER_H));
    let rect = Rect::from_min_max(Pos2::new(whole.min.x, ruler.max.y), whole.max);

    // 音程の範囲。手で置いたぶんも含める
    let (mut lo, mut hi) = (127i32, 0i32);
    for notes in score.values() {
        for n in notes {
            lo = lo.min(n.pitch);
            hi = hi.max(n.pitch);
        }
    }
    if lo > hi {
        lo = 48;
        hi = 84;
    }
    lo -= 3;
    hi += 3;
    let rows = (hi - lo + 1) as f32;
    let row_h = (rect.height() / rows).clamp(3.0, 20.0);

    // ホイールで左右へ動かす。拡大は「何小節ぶん見るか」で決める。
    if resp.hovered() {
        let (dy, dx) = ui.input(|i| (i.raw_scroll_delta.y, i.raw_scroll_delta.x));
        wheel(scroll, dy, dx);
    }
    let max_scroll = (total_steps * *zoom - rect.width()).max(0.0);
    *scroll = scroll.clamp(0.0, max_scroll);

    ed.view_w = rect.width();
    let view = View { zoom: *zoom, scroll: *scroll, top_pitch: hi, row_h, rect };

    draw_loop(&p, tr, &view, &ruler);
    draw_grid(&p, song, &view);
    draw_notes(&p, score, ed, &view);
    draw_ruler(&p, song, &view, &ruler);
    draw_head(&p, tr, &view, &ruler);

    // ---- 触り
    let pointer = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
    if let Some(pos) = pointer {
        if ruler.contains(pos) || ed.scrubbing {
            // 物差しの上。ここでは音符を触らず、鳴らす位置だけ動かす
            handle_ruler(&resp, ui, pos, &view, total_steps, ed, &mut out);
        } else if rect.contains(pos) {
            handle_input(&resp, ui, pos, song, score, project, history, ed, &view, &mut out);
        }
    }
    if resp.drag_stopped() {
        // 指を離したら、そこで1段の区切り。呼ばないと、離してもう一度
        // 同じ音符を掴んだときに前の段へまとめられてしまう
        history.end_group();
        ed.grab = Grab::None;
        ed.scrubbing = false;
        ed.loop_from = None;
    }
    out
}

/// 物差しの触り。
///
/// 押した所へ再生ヘッドを置き、引きずれば追いてくる。
/// Shift を押しながら引きずると、繰り返す範囲になる。
fn handle_ruler(
    resp: &egui::Response,
    ui: &egui::Ui,
    pos: Pos2,
    v: &View,
    total: f32,
    ed: &mut Editor,
    out: &mut Touched,
) {
    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    let at = v.step_at(pos.x).clamp(0.0, total);
    let shift = ui.input(|i| i.modifiers.shift);

    if resp.drag_started() {
        ed.scrubbing = true;
        ed.loop_from = if shift { Some(at) } else { None };
        if !shift {
            out.seek_to = Some(at);
        }
    }
    if resp.dragged() && ed.scrubbing {
        match ed.loop_from {
            Some(from) => {
                let (a, b) = if at < from { (at, from) } else { (from, at) };
                // 幅が無いうちは範囲にしない。押しただけで繰り返しが付くと驚く
                if b - a > 0.5 {
                    out.loop_set = Some((a, b));
                }
            }
            None => out.seek_to = Some(at),
        }
    }
    // 素で叩いたら、そこへ置いて繰り返しを解く
    if resp.clicked() && !shift {
        out.seek_to = Some(at);
        out.loop_clear = true;
    }
}

/// 物差し。小節番号を出す。
fn draw_ruler(p: &egui::Painter, song: &Song, v: &View, ruler: &Rect) {
    p.rect_filled(*ruler, 0.0, theme::SURFACE);
    p.line_segment(
        [Pos2::new(ruler.left(), ruler.bottom()), Pos2::new(ruler.right(), ruler.bottom())],
        Stroke::new(1.0, theme::LINE_STRONG),
    );
    let bars = song.bars();
    // 狭いときは番号を間引く。重なって読めなくなるより飛ばすほうがいい
    let per_bar = song.bar_steps(1) as f32 * v.zoom;
    let every = ((46.0 / per_bar.max(1.0)).ceil() as u32).max(1);
    for b in 0..bars {
        let x = v.x_of(song.bar_start(b + 1) as f32);
        if x < ruler.left() - 1.0 || x > ruler.right() + 1.0 {
            continue;
        }
        let show = b % every == 0;
        p.line_segment(
            [
                Pos2::new(x, ruler.bottom() - if show { 8.0 } else { 4.0 }),
                Pos2::new(x, ruler.bottom()),
            ],
            Stroke::new(1.0, if show { theme::LINE_STRONG } else { theme::LINE }),
        );
        if show {
            p.text(
                Pos2::new(x + 3.0, ruler.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{}", b + 1),
                egui::FontId::monospace(10.0),
                theme::DIM,
            );
        }
    }
}

/// 繰り返す範囲。譜面の地に薄く敷く。
fn draw_loop(p: &egui::Painter, tr: &Transport, v: &View, ruler: &Rect) {
    let Some((a, b)) = tr.loop_range else { return };
    let (x0, x1) = (v.x_of(a), v.x_of(b));
    if x1 < v.rect.left() || x0 > v.rect.right() {
        return;
    }
    let band = Rect::from_min_max(
        Pos2::new(x0.max(v.rect.left()), ruler.top()),
        Pos2::new(x1.min(v.rect.right()), v.rect.bottom()),
    );
    p.rect_filled(band, 0.0, theme::BLUE.gamma_multiply(0.12));
    // 物差しの所だけ帯を濃くして、どこが繰り返しか分かるようにする
    let tab = Rect::from_min_max(
        Pos2::new(band.left(), ruler.bottom() - 3.0),
        Pos2::new(band.right(), ruler.bottom()),
    );
    p.rect_filled(tab, 0.0, theme::BLUE);
}

/// 再生ヘッド。いちばん上に描く。
fn draw_head(p: &egui::Painter, tr: &Transport, v: &View, ruler: &Rect) {
    let x = v.x_of(tr.head);
    if x < v.rect.left() - 1.0 || x > v.rect.right() + 1.0 {
        return;
    }
    let c = if tr.playing { theme::HEAD_ON } else { theme::HEAD_OFF };
    p.line_segment(
        [Pos2::new(x, ruler.top()), Pos2::new(x, v.rect.bottom())],
        Stroke::new(1.0, c),
    );
    // 物差しの所に三角を出す。線だけだと小節線に紛れる
    p.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x - 5.0, ruler.top()),
            Pos2::new(x + 5.0, ruler.top()),
            Pos2::new(x, ruler.top() + 8.0),
        ],
        c,
        Stroke::NONE,
    ));
}

fn draw_grid(p: &egui::Painter, song: &Song, v: &View) {
    let bars = song.bars();
    for b in 0..=bars {
        let step = if b == bars { song.total_steps() } else { song.bar_start(b + 1) };
        let x = v.x_of(step as f32);
        if x < v.rect.left() - 1.0 || x > v.rect.right() + 1.0 {
            continue;
        }
        let strong = b % 4 == 0;
        p.line_segment(
            [Pos2::new(x, v.rect.top()), Pos2::new(x, v.rect.bottom())],
            Stroke::new(1.0, if strong { theme::LINE_STRONG } else { theme::LINE }),
        );
        if b < bars {
            let meter = song.meter_at(b + 1);
            // 拍の目印。拍子が 4/4 でないときに効く
            let per = meter.steps_per_beat();
            for k in (per..meter.steps()).step_by(per as usize) {
                let bx = v.x_of((step + k) as f32);
                if bx > v.rect.left() && bx < v.rect.right() {
                    p.line_segment(
                        [Pos2::new(bx, v.rect.top()), Pos2::new(bx, v.rect.bottom())],
                        Stroke::new(1.0, theme::LINE.gamma_multiply(0.5)),
                    );
                }
            }
            if strong {
                p.text(
                    Pos2::new(x + 4.0, v.rect.top() + 2.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}", b + 1),
                    egui::FontId::monospace(10.0),
                    theme::DIM,
                );
            }
        }
    }
    // セクション名と拍子
    for (name, from, to) in song.section_ranges() {
        let x0 = v.x_of(song.bar_start(from) as f32);
        let x1 = v.x_of((song.bar_start(to) + song.bar_steps(to)) as f32);
        if x1 < v.rect.left() || x0 > v.rect.right() {
            continue;
        }
        let m = song.meter_at(from);
        let label = if song.has_odd_meter() { format!("{name}  {m}") } else { name };
        p.text(
            Pos2::new(x0 + 6.0, v.rect.bottom() - 15.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(11.0),
            theme::DIM,
        );
    }
}

fn draw_notes(p: &egui::Painter, score: &Score, ed: &Editor, v: &View) {
    let mut parts: Vec<&String> = score.keys().collect();
    parts.sort();
    // 編集中のパートは最後に描いて前に出す
    parts.sort_by_key(|n| *n == &ed.part);
    for part in parts {
        let base = theme::part_color(part);
        let front = part == &ed.part;
        for (i, n) in score[part].iter().enumerate() {
            let x0 = v.x_of(n.pos as f32);
            let x1 = v.x_of((n.pos + n.len.max(1)) as f32);
            if x1 < v.rect.left() || x0 > v.rect.right() {
                continue;
            }
            let y = v.y_of(n.pitch);
            if y < v.rect.top() - v.row_h || y > v.rect.bottom() {
                continue;
            }
            let r = Rect::from_min_max(
                Pos2::new(x0, y),
                Pos2::new((x1 - 1.0).max(x0 + 2.0), y + v.row_h - 1.0),
            );
            let picked = ed.selected.as_ref().is_some_and(|(sp, si)| sp == part && *si == i);
            let a = if front { 80 + n.vel } else { 40 + n.vel / 2 };
            let c = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), a);
            p.rect_filled(r, 2.0, c);
            if picked {
                p.rect_stroke(r, 2.0, Stroke::new(1.5, theme::LABEL));
            }
        }
    }
}

/// 音符の右端をつかむ幅（ピクセル）。
const EDGE: f32 = 5.0;

/// 上に置く物差しの高さ。ここを触ると再生ヘッドが動く。
///
/// 音符を置く所と分けてある。分けないと、鳴らす位置を変えたいだけなのに
/// 音符が置かれてしまう。DAW も動画編集ソフトも、たいていこの形。
pub const RULER_H: f32 = 20.0;

#[allow(clippy::too_many_arguments)]
fn handle_input(
    resp: &egui::Response,
    ui: &egui::Ui,
    pos: Pos2,
    song: &Song,
    score: &Score,
    project: &mut Project,
    history: &mut History,
    ed: &mut Editor,
    v: &View,
    out: &mut Touched,
) {
    let step = v.step_at(pos.x);
    let pitch = v.pitch_at(pos.y);
    let total = song.total_steps();

    // 編集は「今のパート」に対してだけ行う。
    // 曲ファイルが作ったぶんを触るときは、まずそのパートを手元へ写す。
    let ensure = |project: &mut Project, ed: &Editor| {
        if !project.is_edited(&ed.part) {
            let from = score.get(&ed.part).cloned().unwrap_or_default();
            project.notes.insert(ed.part.clone(), from);
        }
    };

    let hit = |project: &Project| -> Option<usize> {
        let ns = project.notes.get(&ed.part)?;
        ns.iter().position(|n| {
            n.pitch == pitch && (n.pos as f32) <= step && step < (n.pos + n.len.max(1)) as f32
        })
    };

    // --- 右クリックで消す
    if resp.secondary_clicked() {
        ensure(project, ed);
        if let Some(i) = hit(project) {
            history.record(project, Tag::Once);
            project.notes.get_mut(&ed.part).unwrap().remove(i);
            ed.selected = None;
            out.changed = true;
        }
        return;
    }

    // --- 掴む
    if resp.drag_started() {
        ensure(project, ed);
        if let Some(i) = hit(project) {
            let n = &project.notes[&ed.part][i];
            let right = v.x_of((n.pos + n.len.max(1)) as f32);
            ed.selected = Some((ed.part.clone(), i));
            ed.grab = if (right - pos.x).abs() <= EDGE {
                Grab::Resize { part: ed.part.clone(), index: i }
            } else {
                Grab::Move { part: ed.part.clone(), index: i, offset: step - n.pos as f32 }
            };
        } else {
            ed.grab = Grab::None;
        }
    }

    // --- 動かす / 長さを変える
    if resp.dragged() {
        match ed.grab.clone() {
            Grab::Move { part, index, offset } => {
                // 変える前に覚える。掴んでいるあいだは同じ Tag なので
                // 何フレーム動かしても1段にまとまる
                let want = (step - offset).max(0.0);
                let s = ed.snap.max(1) as f32;
                let np = ((want / s).round() * s) as u32;
                let differs = project
                    .notes
                    .get(&part)
                    .and_then(|ns| ns.get(index))
                    .is_some_and(|n| n.pos != np.min(total.saturating_sub(n.len.max(1))) || n.pitch != pitch);
                if differs {
                    history.record(project, Tag::Move(part.clone(), index));
                }
                if let Some(ns) = project.notes.get_mut(&part) {
                    if let Some(n) = ns.get_mut(index) {
                        let np = np.min(total.saturating_sub(n.len.max(1)));
                        if n.pos != np || n.pitch != pitch {
                            n.pos = np;
                            n.pitch = pitch.clamp(0, 127);
                            out.changed = true;
                        }
                    }
                }
            }
            Grab::Resize { part, index } => {
                let s = ed.snap.max(1) as f32;
                let differs = project.notes.get(&part).and_then(|ns| ns.get(index)).is_some_and(|n| {
                    let end = ((step / s).round() * s).max((n.pos + 1) as f32) as u32;
                    n.len != (end.saturating_sub(n.pos)).max(1).min(total - n.pos)
                });
                if differs {
                    history.record(project, Tag::Resize(part.clone(), index));
                }
                if let Some(ns) = project.notes.get_mut(&part) {
                    if let Some(n) = ns.get_mut(index) {
                        let end = ((step / s).round() * s).max((n.pos + 1) as f32) as u32;
                        let len = (end.saturating_sub(n.pos)).max(1).min(total - n.pos);
                        if n.len != len {
                            n.len = len;
                            ed.new_len = len; // 次に置くときの長さにも使う
                            out.changed = true;
                        }
                    }
                }
            }
            Grab::None => {}
        }
        return;
    }

    // --- 左クリックで置く（空いている所だけ）
    if resp.clicked() {
        ensure(project, ed);
        if let Some(i) = hit(project) {
            ed.selected = Some((ed.part.clone(), i));
            return;
        }
        if step < 0.0 || step >= total as f32 || !(0..=127).contains(&pitch) {
            return;
        }
        let at = ed.snapped(step).min(total.saturating_sub(1));
        let len = ed.new_len.max(1).min(total - at);
        history.record(project, Tag::Once);
        project.add_note(
            &ed.part,
            Note { pos: at, len, pitch, vel: 100, mora: String::new() },
        );
        // 置いたものを選んだことにする
        ed.selected = project.notes[&ed.part]
            .iter()
            .position(|n| n.pos == at && n.pitch == pitch)
            .map(|i| (ed.part.clone(), i));
        out.changed = true;
    }

    // --- 掴んでいる所の見た目を変える
    if let Some(ns) = project.notes.get(&ed.part) {
        let on_edge = ns.iter().any(|n| {
            n.pitch == pitch && (v.x_of((n.pos + n.len.max(1)) as f32) - pos.x).abs() <= EDGE
        });
        if on_edge {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    let _ = Vec2::ZERO;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        View {
            zoom: 10.0,
            scroll: 0.0,
            top_pitch: 84,
            row_h: 10.0,
            rect: Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 400.0)),
        }
    }

    #[test]
    fn screen_and_score_round_trip() {
        let v = view();
        for step in [0.0f32, 1.0, 15.5, 64.0] {
            let x = v.x_of(step);
            assert!((v.step_at(x) - step).abs() < 1e-3, "step {step}");
        }
        for pitch in [84i32, 72, 60, 48] {
            let y = v.y_of(pitch);
            // 行の上端を指すので、その行の中を指せば戻る
            assert_eq!(v.pitch_at(y + 1.0), pitch, "pitch {pitch}");
        }
    }

    #[test]
    fn scroll_shifts_the_mapping() {
        let mut v = view();
        v.scroll = 200.0;
        assert!((v.step_at(v.x_of(30.0)) - 30.0).abs() < 1e-3);
        assert!(v.x_of(0.0) < v.rect.left(), "左へ流れていない");
    }

    #[test]
    fn wheel_moves_sideways() {
        let mut sc = 100.0f32;
        // 手前へ回す（dy が負）と先へ進む
        wheel(&mut sc, -50.0, 0.0);
        assert!(sc > 100.0, "先へ進まない: {sc}");
        let forward = sc;
        // 奥へ回すと戻る
        wheel(&mut sc, 50.0, 0.0);
        assert!(sc < forward, "戻らない");
        assert!((sc - 100.0).abs() < 1e-3, "行って戻って元に戻らない: {sc}");
    }

    #[test]
    fn trackpad_horizontal_also_works() {
        let mut sc = 100.0f32;
        wheel(&mut sc, 0.0, -30.0);
        assert!(sc > 100.0, "横の送りが効かない: {sc}");
    }

    #[test]
    fn no_scroll_no_change() {
        let mut sc = 50.0f32;
        wheel(&mut sc, 0.0, 0.0);
        assert_eq!(sc, 50.0);
    }

    fn song_4_4(bars: u32) -> Song {
        let src = format!(
            "let TITLE = \"t\"; let BPM = 120;\n\
             let SECTIONS = [[\"A\", {bars}, \"p\", \"k\", \"m\", 1.0]];\n\
             let VOICES = #{{ lead: #{{ ch: 0, patch: \"piano\" }} }};"
        );
        tonescript_song::load_str(&src).expect("読めるはず")
    }

    #[test]
    fn fit_shows_exactly_that_many_bars() {
        let s = song_4_4(16);
        let w = 800.0;
        for n in [2u32, 4, 8, 16] {
            let z = zoom_for_bars(&s, 1, n, w);
            // n 小節 = n*16 目盛り。それが画面の幅ちょうどに収まる
            let shown = w / z;
            assert!(
                (shown - (n * 16) as f32).abs() < 0.5,
                "{n}小節のはずが {shown}目盛り"
            );
        }
    }

    #[test]
    fn fit_handles_odd_meters() {
        // 4/4 が2小節、7/8 が2小節。合わせて 16+16+14+14 = 60 目盛り
        let src = "let TITLE = \"t\"; let BPM = 120;\n\
                   let SECTIONS = [[\"A\", 2, \"p\", \"k\", \"m\", 1.0],\n\
                                   [\"B\", 2, \"p\", \"k\", \"m\", 1.0, [7, 8]]];\n\
                   let VOICES = #{ lead: #{ ch: 0, patch: \"piano\" } };";
        let s = tonescript_song::load_str(src).expect("読めるはず");
        let w = 600.0;
        // 頭から4小節 = 全部
        let z = zoom_for_bars(&s, 1, 4, w);
        assert!((w / z - 60.0).abs() < 0.5, "{}目盛り", w / z);
        // 3小節目から2小節 = 7/8 が2つ = 28
        let z = zoom_for_bars(&s, 3, 2, w);
        assert!((w / z - 28.0).abs() < 0.5, "{}目盛り", w / z);
    }

    #[test]
    fn fit_whole_song() {
        let s = song_4_4(16);
        let z = zoom_for_whole(&s, 800.0);
        assert!((800.0 / z - 256.0).abs() < 0.5, "全体が収まらない");
    }

    #[test]
    fn fit_stays_inside_the_limits() {
        let s = song_4_4(400);
        // うんと長い曲を全部収めようとしても、下限で止まる
        let z = zoom_for_whole(&s, 100.0);
        assert!(z >= MIN_ZOOM, "下限を割った: {z}");
        // うんと狭い範囲へ寄せても、上限で止まる
        let z = zoom_for_bars(&s, 1, 1, 100_000.0);
        assert!(z <= MAX_ZOOM, "上限を超えた: {z}");
    }

    #[test]
    fn fit_does_not_break_past_the_end() {
        let s = song_4_4(4);
        // 曲より多い小節数を頼まれても落ちない
        let z = zoom_for_bars(&s, 1, 999, 800.0);
        assert!(z.is_finite() && z > 0.0);
        // 曲より後ろの小節から数えても落ちない
        let z = zoom_for_bars(&s, 999, 4, 800.0);
        assert!(z.is_finite() && z > 0.0);
    }

    #[test]
    fn snap_rounds_down_to_the_grid() {
        let mut ed = Editor::default();
        ed.snap = 4;
        assert_eq!(ed.snapped(0.0), 0);
        assert_eq!(ed.snapped(3.9), 0);
        assert_eq!(ed.snapped(4.0), 4);
        assert_eq!(ed.snapped(7.5), 4);
        ed.snap = 1;
        assert_eq!(ed.snapped(7.5), 7);
        // 0 を渡しても割り算で落ちない
        ed.snap = 0;
        assert_eq!(ed.snapped(7.5), 7);
    }
}
