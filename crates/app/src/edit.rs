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

/// 音声トラックのどこを掴んでいるか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipPart {
    /// 真ん中。動かす
    Body,
    /// 左の端。頭を切る
    Head,
    /// 右の端。尻を切る
    Tail,
}

/// 掴んでいるもの。
#[derive(Clone, Debug, PartialEq)]
pub enum Grab {
    None,
    /// 動かしている（掴んだ位置からのずれを覚えておく）。
    ///
    /// **縦のずれも覚える。** 覚えずに「指のある段」へ合わせると、
    /// 段を1つ外して掴んだときに、掴んだ瞬間に半音ずれる
    Move { part: String, index: usize, offset: f32, pitch_off: i32 },
    /// 右端を掴んで長さを変えている
    Resize { part: String, index: usize },
    /// 四角で囲んで選んでいる（掴んだ所）
    Box { from: (f32, i32) },
}

/// 編集の設定と途中の状態。
pub struct Editor {
    pub part: String,
    /// 新しく置く音符の長さ（目盛り）
    pub new_len: u32,
    /// 置くときに合わせる刻み
    pub snap: u32,
    pub grab: Grab,
    /// 選んでいる音符（今のパートの何番目か）。色を変えて分かるようにする。
    ///
    /// 番号で持つので、**音符を足したり消したりしたら選び直す**
    pub sel: Vec<usize>,
    /// 囲んでいる最中の反対の角
    pub box_to: Option<(f32, i32)>,
    /// 下に強さのレーンを出すか
    pub show_vel: bool,
    /// 強さのレーンを引きずっている最中か
    pub vel_drag: bool,
    /// 音声トラックのレーンを出すか
    pub show_audio: bool,
    /// 音声トラックを掴んでいる `(名前, どこを, 掴んだときのずれ)`
    pub clip_grab: Option<(String, ClipPart, f32)>,
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
            sel: Vec::new(),
            box_to: None,
            show_vel: true,
            vel_drag: false,
            show_audio: true,
            clip_grab: None,
            view_w: 800.0,
            scrubbing: false,
            loop_from: None,
        }
    }
}

impl Editor {
    /// 選んでいるか。
    pub fn is_selected(&self, i: usize) -> bool {
        self.sel.contains(&i)
    }

    /// 選び直す。
    pub fn select(&mut self, list: Vec<usize>) {
        self.sel = list;
        self.sel.sort_unstable();
        self.sel.dedup();
    }

    /// 1つ足す／外す。
    pub fn toggle(&mut self, i: usize) {
        if let Some(k) = self.sel.iter().position(|x| *x == i) {
            self.sel.remove(k);
        } else {
            self.sel.push(i);
            self.sel.sort_unstable();
        }
    }

    pub fn clear_sel(&mut self) {
        self.sel.clear();
    }

    pub fn snapped(&self, step: f32) -> u32 {
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
    /// 触った音を鳴らしてほしい `(パート, 音程, 強さ, 長さ)`。
    /// 置いた・掴んだ・音程を変えたときに入る
    pub hit: Option<(String, i32, u8, u32)>,
    /// 音声トラックを触った
    pub clip: Option<ClipEdit>,
}

/// 音声トラック1本ぶんの見え方。画面側が用意する。
///
/// 波形そのものではなく、**山の高さだけを間引いたもの**を持つ。
/// 3分の歌は 800 万サンプルあるが、描くのに要るのは数百点しかない。
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub label: String,
    /// 曲のどこから鳴るか（目盛り）
    pub at: u32,
    /// 何目盛りぶん鳴るか（切り詰めたあと）
    pub len: f32,
    /// 何秒ぶん鳴るか（切り詰めたあと）。目盛りと秒の換算に使う
    pub secs: f32,
    /// 山の高さ。左から順に
    pub peaks: Vec<f32>,
    pub gain: f32,
    /// 切り詰めた秒数。端を掴んだときの目安に出す
    pub trim_in: f32,
    pub trim_out: f32,
}

/// 音声トラックを触った結果。
#[derive(Clone, Debug, PartialEq)]
pub enum ClipEdit {
    /// ここへ動かした（目盛り）
    Move { name: String, at: u32 },
    /// 頭を何秒落とすか変えた
    TrimIn { name: String, secs: f32 },
    /// 尻を何秒落とすか変えた
    TrimOut { name: String, secs: f32 },
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
    clips: &[Clip],
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
    // 下に強さのレーン。狭い窓では出さない（譜面が潰れる）
    let lane_h = if ed.show_vel && whole.height() > RULER_H + VEL_H + 80.0 { VEL_H } else { 0.0 };
    let lane = Rect::from_min_max(Pos2::new(whole.min.x, whole.max.y - lane_h), whole.max);
    // 物差しのすぐ下に音声の帯。小節に揃えて見せるため、譜面と同じ目盛りを使う
    let audio_h = if ed.show_audio && !clips.is_empty() {
        (CLIP_H * clips.len() as f32).min(CLIP_H * 3.0)
    } else {
        0.0
    };
    let audio = Rect::from_min_max(
        Pos2::new(whole.min.x, ruler.max.y),
        Pos2::new(whole.max.x, ruler.max.y + audio_h),
    );
    let rect = Rect::from_min_max(Pos2::new(whole.min.x, audio.max.y),
        Pos2::new(whole.max.x, whole.max.y - lane_h));

    // 音程の範囲。**今触っているパートに合わせる**（`pitch_range` を見よ）
    let (lo, hi) = pitch_range(score, &ed.part);
    let rows = (hi - lo + 1) as f32;
    let row_h = (rect.height() / rows).clamp(ROW_MIN, 20.0);

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
    draw_box(&p, ed, &view);
    if lane_h > 0.0 {
        draw_vel(&p, project, score, ed, &view, &lane);
    }
    if audio_h > 0.0 {
        draw_clips(&p, clips, ed, &view, &audio);
    }

    // ---- 触り
    let pointer = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
    if let Some(pos) = pointer {
        if (audio_h > 0.0 && audio.contains(pos) && !ed.scrubbing) || ed.clip_grab.is_some() {
            handle_clips(&resp, ui, pos, clips, ed, &view, &audio, &mut out);
        } else if (lane_h > 0.0 && lane.contains(pos) && !ed.scrubbing) || ed.vel_drag {
            // 強さのレーン。ここでは音符を置かず、強さだけ変える
            handle_vel(&resp, pos, project, score, history, ed, &view, &lane, &mut out);
        } else if ruler.contains(pos) || ed.scrubbing {
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
        ed.box_to = None;
        ed.vel_drag = false;
        ed.clip_grab = None;
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
            let picked = front && ed.is_selected(i);
            let a = if front { 80 + n.vel } else { 40 + n.vel / 2 };
            let c = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), a);
            p.rect_filled(r, 2.0, c);
            if picked {
                p.rect_stroke(r, 2.0, Stroke::new(1.5, theme::LABEL));
            }
        }
    }
}

/// 囲んでいる四角を描く。
fn draw_box(p: &egui::Painter, ed: &Editor, v: &View) {
    let (Grab::Box { from }, Some(to)) = (&ed.grab, ed.box_to) else { return };
    let r = Rect::from_two_pos(
        Pos2::new(v.x_of(from.0), v.y_of(from.1)),
        Pos2::new(v.x_of(to.0), v.y_of(to.1) + v.row_h),
    );
    p.rect_filled(r, 1.0, Color32::from_rgba_unmultiplied(0x0a, 0x84, 0xff, 40));
    p.rect_stroke(r, 1.0, Stroke::new(1.0, theme::BLUE));
}

/// 強さのレーン。音符1本ずつを棒で出す。
///
/// **音程は見ない。** 同じ所に重なっている音は、上の音の棒だけが見える
/// （掴めるのも上の音）。和音の中の1本だけを変えたいときは、先に選んでおく。
fn draw_vel(
    p: &egui::Painter,
    project: &Project,
    score: &Score,
    ed: &Editor,
    v: &View,
    lane: &Rect,
) {
    p.rect_filled(*lane, 0.0, theme::SURFACE);
    p.line_segment(
        [Pos2::new(lane.left(), lane.top()), Pos2::new(lane.right(), lane.top())],
        Stroke::new(1.0, theme::LINE_STRONG),
    );
    let notes: &[tonescript_song::model::Note] = project
        .notes
        .get(&ed.part)
        .map(|v| v.as_slice())
        .or_else(|| score.get(&ed.part).map(|v| v.as_slice()))
        .unwrap_or(&[]);
    let base = theme::part_color(&ed.part);
    let inner = lane.height() - 8.0;
    for (i, n) in notes.iter().enumerate() {
        let x0 = v.x_of(n.pos as f32);
        let x1 = v.x_of((n.pos + n.len.max(1)) as f32);
        if x1 < lane.left() || x0 > lane.right() {
            continue;
        }
        let h = inner * (n.vel as f32 / 127.0);
        let top = lane.bottom() - 4.0 - h;
        let r = Rect::from_min_max(
            Pos2::new(x0, top),
            Pos2::new((x1 - 1.0).max(x0 + 2.0), lane.bottom() - 4.0),
        );
        let picked = ed.is_selected(i);
        let a = if picked { 235 } else { 150 };
        p.rect_filled(r, 1.0, Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), a));
        if picked {
            p.rect_stroke(r, 1.0, Stroke::new(1.0, theme::LABEL));
        }
    }
}

/// 強さのレーンの触り。押した高さがそのまま強さになる。
#[allow(clippy::too_many_arguments)]
fn handle_vel(
    resp: &egui::Response,
    pos: Pos2,
    project: &mut Project,
    score: &Score,
    history: &mut History,
    ed: &mut Editor,
    v: &View,
    lane: &Rect,
    out: &mut Touched,
) {
    if !(resp.dragged() || resp.clicked() || resp.drag_started()) {
        return;
    }
    ed.vel_drag = true;
    // 曲ファイルが作ったぶんを触るときは、まず手元へ写す
    if !project.is_edited(&ed.part) {
        let from = score.get(&ed.part).cloned().unwrap_or_default();
        project.notes.insert(ed.part.clone(), from);
    }
    let step = v.step_at(pos.x);
    let inner = (lane.height() - 8.0).max(1.0);
    let up = (lane.bottom() - 4.0 - pos.y) / inner;
    let want = (up.clamp(0.0, 1.0) * 127.0).round().max(1.0) as u8;

    let Some(ns) = project.notes.get_mut(&ed.part) else { return };
    let Some(i) = crate::sel::at_step(ns, step) else { return };
    // 引きずっているあいだは1段にまとめる
    history.record(project, Tag::Curve(ed.part.clone(), "vel"));
    let Some(ns) = project.notes.get_mut(&ed.part) else { return };
    if crate::sel::set_vel(ns, i, want, &ed.sel) {
        out.changed = true;
    }
}

/// 音声トラック1本ぶんの高さ。
pub const CLIP_H: f32 = 46.0;
/// 端を掴む幅（ピクセル）。
const CLIP_EDGE: f32 = 6.0;

/// 音声トラックを、小節に揃えて描く。
fn draw_clips(p: &egui::Painter, clips: &[Clip], ed: &Editor, v: &View, lane: &Rect) {
    p.rect_filled(*lane, 0.0, theme::SURFACE);
    for (i, c) in clips.iter().enumerate() {
        let top = lane.top() + i as f32 * CLIP_H;
        if top > lane.bottom() {
            break;
        }
        let r = clip_rect(c, v, lane, i);
        if r.right() < lane.left() || r.left() > lane.right() {
            continue;
        }
        let held = ed.clip_grab.as_ref().is_some_and(|(n, _, _)| *n == c.name);
        let base = theme::part_color(&c.name);
        p.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 46));
        p.rect_stroke(
            r,
            3.0,
            Stroke::new(if held { 1.6 } else { 1.0 }, if held { theme::LABEL } else { base }),
        );
        // 波形。山の高さを縦の線で出す
        let mid = r.center().y;
        let half = (CLIP_H * 0.5 - 8.0).max(2.0);
        let w = r.width().max(1.0);
        let n = c.peaks.len().max(1);
        let step = (w / 220.0).max(1.0); // 1ピクセルおきに描くと細かすぎる
        let mut x = r.left().max(lane.left());
        while x < r.right().min(lane.right()) {
            let t = ((x - r.left()) / w).clamp(0.0, 1.0);
            let h = c.peaks[((t * n as f32) as usize).min(n - 1)] * half * c.gain.min(1.5);
            p.line_segment(
                [Pos2::new(x, mid - h), Pos2::new(x, mid + h)],
                Stroke::new(1.0, base),
            );
            x += step;
        }
        // 名前。左端が画面の外なら、見える所へ寄せる
        let tx = r.left().max(lane.left()) + 4.0;
        p.text(
            Pos2::new(tx, top + 2.0),
            egui::Align2::LEFT_TOP,
            &c.label,
            egui::FontId::proportional(11.0),
            theme::DIM,
        );
    }
}

/// そのトラックが画面のどこに出るか。
fn clip_rect(c: &Clip, v: &View, lane: &Rect, row: usize) -> Rect {
    let top = lane.top() + row as f32 * CLIP_H;
    Rect::from_min_max(
        Pos2::new(v.x_of(c.at as f32), top + 1.0),
        Pos2::new(v.x_of(c.at as f32 + c.len).max(v.x_of(c.at as f32) + 3.0), top + CLIP_H - 3.0),
    )
}

/// 音声トラックの触り。真ん中を掴めば動き、端を掴めば切り詰める。
#[allow(clippy::too_many_arguments)]
fn handle_clips(
    resp: &egui::Response,
    _ui: &egui::Ui,
    pos: Pos2,
    clips: &[Clip],
    ed: &mut Editor,
    v: &View,
    lane: &Rect,
    out: &mut Touched,
) {
    let step = v.step_at(pos.x);
    if resp.drag_started() {
        ed.clip_grab = None;
        for (i, c) in clips.iter().enumerate() {
            let r = clip_rect(c, v, lane, i);
            if !r.contains(pos) {
                continue;
            }
            let part = if (pos.x - r.left()).abs() <= CLIP_EDGE {
                ClipPart::Head
            } else if (r.right() - pos.x).abs() <= CLIP_EDGE {
                ClipPart::Tail
            } else {
                ClipPart::Body
            };
            ed.clip_grab = Some((c.name.clone(), part, step - c.at as f32));
            break;
        }
        return;
    }
    if !resp.dragged() {
        return;
    }
    let Some((name, part, offset)) = ed.clip_grab.clone() else { return };
    let Some(c) = clips.iter().find(|c| c.name == name) else { return };
    // 1目盛りあたり何秒か。切り詰めを秒で持っているので換算する
    let per_step = ((v.x_of(1.0) - v.x_of(0.0)) / v.zoom.max(1e-6)).abs();
    let _ = per_step;
    match part {
        ClipPart::Body => {
            let want = ed.snapped((step - offset).max(0.0));
            if want != c.at {
                out.clip = Some(ClipEdit::Move { name, at: want });
            }
        }
        ClipPart::Head => {
            // 左端を右へ動かすと、そのぶん頭が落ちる
            let d = step - c.at as f32;
            let secs = c.trim_in + steps_to_secs(d, c);
            out.clip = Some(ClipEdit::TrimIn { name, secs: secs.max(0.0) });
        }
        ClipPart::Tail => {
            let d = (c.at as f32 + c.len) - step;
            let secs = c.trim_out + steps_to_secs(d, c);
            out.clip = Some(ClipEdit::TrimOut { name, secs: secs.max(0.0) });
        }
    }
}

/// 目盛りの差を秒へ。そのトラックの長さから逆算する。
fn steps_to_secs(steps: f32, c: &Clip) -> f32 {
    if c.len <= 0.0 || c.secs <= 0.0 {
        return 0.0;
    }
    steps * (c.secs / c.len)
}

/// 強さのレーンの高さ。
pub const VEL_H: f32 = 84.0;

/// 音符の右端をつかむ幅（ピクセル）。
const EDGE: f32 = 7.0;

/// 掴むときに、上下何段まで大目に見るか。
///
/// 1段が 7〜12px しかないので、狙った段をぴったり指すのは難しい。
/// 外すと「何も無い所を引きずった」ことになって、四角で囲む動きに化ける。
/// **掴むときだけ**緩める。置くときは緩めない（すぐ上に音符があると、
/// その隣に置けなくなる）
const GRAB_ROWS: i32 = 1;

/// 右端を掴んだ音符。
///
/// **中に居るかどうかより先に、これを見る。**
///
/// 右端は音符の「外」にある（位置 `pos + len` は次の音符が始まる所）。
/// 中に居ることを条件にすると、帯の外半分では長さ変更にならず、
/// 何も無い所を引きずったことになって**四角で囲む動きに化ける**。
/// 矢印は「伸ばせます」の形のままなので、効かない理由が分からない。
///
/// 帯は音符の幅の 4割までにする。短い音符で帯が全体を覆うと、
/// 今度は動かせなくなる
/// そこで引きずったら何が起きるか。
///
/// **矢印もこれで決める。** 前は矢印と掴みで別々に見ていたので、
/// 矢印は「伸ばせます」なのに引きずると動く、逆に何も出ていないのに
/// 伸びる、ということが起きていた
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    /// 右端を掴んで長さを変える
    Resize(usize),
    /// 掴んで動かす
    Move(usize),
    /// 何も無いので、四角で囲んで選ぶ
    Select,
}

/// 押した所で何が起きるか。
///
/// **これ1つを、矢印を出す側と掴む側の両方が呼ぶ。**
pub fn what_at(ns: &[Note], pitch: i32, x: f32, step: f32, v: &View) -> What {
    if let Some(i) = edge_at(ns, pitch, x, v) {
        return What::Resize(i);
    }
    // 中を掴む。段を1つ外しても拾う
    let over = |n: &Note| (n.pos as f32) <= step && step < (n.pos + n.len.max(1)) as f32;
    let hit = ns
        .iter()
        .enumerate()
        .filter(|(_, n)| (n.pitch - pitch).abs() <= GRAB_ROWS && over(n))
        // ぴったりの段を優先し、同じなら先に置いたものを
        .min_by_key(|(i, n)| ((n.pitch - pitch).abs(), *i))
        .map(|(i, _)| i);
    match hit {
        Some(i) => What::Move(i),
        None => What::Select,
    }
}

fn edge_at(ns: &[Note], pitch: i32, x: f32, v: &View) -> Option<usize> {
    ns.iter().position(|n| {
        if (n.pitch - pitch).abs() > GRAB_ROWS {
            return false;
        }
        let len = n.len.max(1);
        let band = EDGE.min(len as f32 * v.zoom * 0.4);
        (v.x_of((n.pos + len) as f32) - x).abs() <= band
    })
}

/// 1段をこれより低くしない。
///
/// これより低いと、音符が線になって掴めない。全部が入りきらなくても、
/// **触れる大きさのほうを優先する**
const ROW_MIN: f32 = 7.0;

/// 画面に出す音程の幅。狭くしすぎない。
const MIN_ROWS: i32 = 22;

/// 縦に出す音程の範囲を決める。
///
/// **今触っているパートの音域に合わせる。** 曲に出てくる音程を全部入れると、
/// ベースから旋律まで 45〜64段になり、1段が 5〜10px にまで潰れる。
/// そこまで細いと、掴むのも動かすのも当たらない（実際そうなっていた）。
///
/// 旋律だけなら 15段ほどで済むので、同じ高さでも1段が3倍近く取れる。
/// 他のパートは範囲から外れたぶんが見えなくなるが、触れないものが
/// 見えていることより、触るものが掴めることのほうが大事。
fn pitch_range(score: &Score, part: &str) -> (i32, i32) {
    let span = |ns: &[Note]| -> Option<(i32, i32)> {
        let lo = ns.iter().map(|n| n.pitch).min()?;
        let hi = ns.iter().map(|n| n.pitch).max()?;
        Some((lo, hi))
    };
    // 触っているパート → 曲全体 → 既定（C3〜C6）の順で当てる
    let found = score
        .get(part)
        .and_then(|ns| span(ns))
        .or_else(|| {
            let all: Vec<Note> = score.values().flatten().cloned().collect();
            span(&all)
        })
        .unwrap_or((48, 84));
    let (mut lo, mut hi) = (found.0 - 3, found.1 + 3);
    // 狭すぎると、隣の音へ動かす余地が画面に無くなる
    while hi - lo + 1 < MIN_ROWS {
        lo -= 1;
        hi += 1;
    }
    (lo.max(0), hi.min(127))
}

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
        // 触り始めた時点の生成を控えておく。あとで曲ファイルが直されたとき、
        // 「曲ファイルが変えた所」と「人が変えた所」を見分けるのに要る
        let from = score.get(&ed.part).cloned().unwrap_or_default();
        project.take_over(&ed.part, &from);
    };

    // どの音符の上か。`ed` を借りたままにしないよう、パート名を写して持つ
    let part_name = ed.part.clone();
    let over = |n: &Note| (n.pos as f32) <= step && step < (n.pos + n.len.max(1)) as f32;
    let hit = |project: &Project| -> Option<usize> {
        let ns = project.notes.get(&part_name)?;
        ns.iter().position(|n| n.pitch == pitch && over(n))
    };

    // --- 右クリックで消す。選んでいるものの上なら、選んだぶん全部
    if resp.secondary_clicked() {
        ensure(project, ed);
        if let Some(i) = hit(project) {
            history.record(project, Tag::Once);
            let ns = project.notes.get_mut(&ed.part).unwrap();
            if ed.is_selected(i) && ed.sel.len() > 1 {
                *ns = crate::sel::remove(ns, &ed.sel);
            } else {
                ns.remove(i);
            }
            ed.clear_sel();
            out.changed = true;
        }
        return;
    }

    // --- 掴む
    if resp.drag_started() {
        ensure(project, ed);
        // **押した所で決める。**
        //
        // egui は指が数ピクセル動いてから「引きずり始めた」と言う。その
        // ときの指の位置は、押した所からもう離れている。今の位置で決めると、
        // 端を押して横へ引いた瞬間に帯から出て、動かす方になる
        //（矢印は端のままなので、余計に分からない）
        let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
        let (s_step, s_pitch) = (v.step_at(start.x), v.pitch_at(start.y));
        let ns = project.notes.get(&ed.part).cloned().unwrap_or_default();
        let what = what_at(&ns, s_pitch, start.x, s_step, v);
        if let What::Resize(i) | What::Move(i) = what {
            let n = &project.notes[&ed.part][i];
            out.hit = Some((ed.part.clone(), n.pitch, n.vel, n.len));
            // 選んでいないものを掴んだら、それだけを選び直す。
            // 選んでいるものを掴んだら、選んだまま（まとめて動かすため）
            if !ed.is_selected(i) {
                ed.select(vec![i]);
            }
            ed.grab = match what {
                What::Resize(_) => Grab::Resize { part: ed.part.clone(), index: i },
                _ => Grab::Move {
                    part: ed.part.clone(),
                    index: i,
                    offset: s_step - n.pos as f32,
                    pitch_off: s_pitch - n.pitch,
                },
            };
        } else {
            // 何も無い所から引きずったら、四角で囲んで選ぶ
            ed.grab = Grab::Box { from: (step, pitch) };
            ed.box_to = Some((step, pitch));
        }
    }

    // --- 動かす / 長さを変える
    if resp.dragged() {
        match ed.grab.clone() {
            Grab::Move { part, index, offset, pitch_off } => {
                // 変える前に覚える。掴んでいるあいだは同じ Tag なので
                // 何フレーム動かしても1段にまとまる
                let want = (step - offset).max(0.0);
                let s = ed.snap.max(1) as f32;
                let np = ((want / s).round() * s) as u32;
                let Some(now) = project.notes.get(&part).and_then(|ns| ns.get(index)).cloned()
                else {
                    return;
                };
                let np = np.min(total.saturating_sub(now.len.max(1)));
                // 掴んだときのずれを引いてから比べる。指の段そのものではない
                let (dstep, dpitch) =
                    (np as i32 - now.pos as i32, (pitch - pitch_off) - now.pitch);
                if dstep == 0 && dpitch == 0 {
                    return;
                }
                history.record(project, Tag::Move(part.clone(), index));
                let Some(ns) = project.notes.get_mut(&part) else { return };
                // 掴んだ1本だけでなく、選んでいるぶん全部を同じだけ動かす
                let sel: Vec<usize> =
                    if ed.sel.len() > 1 { ed.sel.clone() } else { vec![index] };
                if crate::sel::nudge(ns, &sel, dstep, dpitch, total) {
                    if dpitch != 0 {
                        // 音程が動いたら、その音を返す。掴んだまま
                        // 上下すれば音階が聞こえる
                        out.hit = Some((part.clone(), now.pitch + dpitch, now.vel, now.len));
                    }
                    out.changed = true;
                }
            }
            Grab::Box { from } => {
                ed.box_to = Some((step, pitch));
                let ns = project.notes.get(&ed.part).cloned().unwrap_or_default();
                let picked = crate::sel::marquee(&ns, from, (step, pitch));
                ed.select(picked);
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
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if let Some(i) = hit(project) {
            let n = &project.notes[&ed.part][i];
            out.hit = Some((ed.part.clone(), n.pitch, n.vel, n.len));
            // Shift か Ctrl を押しながらなら、選んだものに足す／外す
            if add {
                ed.toggle(i);
            } else {
                ed.select(vec![i]);
            }
            return;
        }
        if add {
            // 押しながら空きを叩いたら、選んだものを解くだけ
            ed.clear_sel();
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
        out.hit = Some((ed.part.clone(), pitch, 100, len));
        // 置いたものを選んだことにする
        let picked = project.notes[&ed.part]
            .iter()
            .position(|n| n.pos == at && n.pitch == pitch);
        ed.select(picked.into_iter().collect());
        out.changed = true;
    }

    // --- 掴んでいる所の見た目を変える
    //
    // **画面に描いてあるものを見る。** 前は「手で直したぶん」だけを見て
    // いたので、まだ触っていないパートでは矢印が出なかった。それでいて
    // 引きずれば伸びる（掴む側は触った時点で写すので）。
    // 触った瞬間から矢印が出始める、という分かりにくい違いになっていた
    let shown: &[Note] = project
        .notes
        .get(&part_name)
        .map(|v| v.as_slice())
        .or_else(|| score.get(&part_name).map(|v| v.as_slice()))
        .unwrap_or(&[]);
    match what_at(shown, pitch, pos.x, step, v) {
        What::Resize(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
        What::Move(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
        What::Select => {}
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

    /// **音符が掴める大きさになっていること。**
    ///
    /// 曲に出てくる音程を全部入れていたので、1段が 5〜10px に潰れて
    /// 掴めなかった。触っているパートに合わせれば、同じ高さでも太くなる
    #[test]
    fn the_rows_are_thick_enough_to_grab() {
        let mut score: Score = Score::new();
        // ベース（低い）と旋律（高い）。前はこの幅ぜんぶを画面に詰めていた
        score.insert("bass".into(), vec![note(0, 4, 40), note(4, 4, 47)]);
        score.insert("lead".into(), vec![note(0, 4, 67), note(4, 4, 81)]);

        let (lo, hi) = pitch_range(&score, "lead");
        let rows = hi - lo + 1;
        assert!(rows <= 26, "旋律を触っているのに {rows} 段も出している");
        assert!(lo <= 67 && hi >= 81, "触っているパートが画面から外れている");

        // よくある高さで、指で掴める太さになること
        for h in [300.0f32, 500.0] {
            let row_h = (h / rows as f32).clamp(ROW_MIN, 20.0);
            assert!(row_h >= 11.0, "高さ{h}で1段 {row_h:.1}px しかない");
        }

        // パートを変えれば、そちらへ寄ること
        let (lo2, hi2) = pitch_range(&score, "bass");
        assert!(lo2 <= 40 && hi2 >= 47, "ベースが画面から外れている");
        assert!(hi2 < 67, "旋律まで入れている（また潰れる）");
    }

    #[test]
    fn a_narrow_part_still_gets_room_to_move() {
        // 1音しか無いパート。ここを音域そのままにすると、
        // 上下へ動かす余地が画面に無くなる
        let mut score: Score = Score::new();
        score.insert("lead".into(), vec![note(0, 4, 60)]);
        let (lo, hi) = pitch_range(&score, "lead");
        assert!(hi - lo + 1 >= MIN_ROWS, "幅が {} しかない", hi - lo + 1);
        assert!(lo < 60 && hi > 60, "音符が端に寄っている");
    }

    #[test]
    fn an_empty_part_falls_back_to_something_sensible() {
        let mut score: Score = Score::new();
        score.insert("bass".into(), vec![note(0, 4, 40)]);
        // まだ音符の無いパートを触っている → 曲全体から当てる
        let (lo, hi) = pitch_range(&score, "lead");
        assert!(lo <= 40 && hi >= 40, "曲の音程が入っていない");
        // 曲にも何も無ければ既定
        let (lo, hi) = pitch_range(&Score::new(), "lead");
        assert!(lo <= 48 && hi >= 84, "既定が狭い（{lo}〜{hi}）");
        assert!((0..=127).contains(&lo) && (0..=127).contains(&hi));
    }

    /// 段を1つ外しても掴めること。ただし**掴んだ瞬間にずれない**こと。
    #[test]
    fn a_near_miss_still_grabs_the_note() {
        let v = view();
        let ns = vec![note(0, 4, 60)];
        let right = v.x_of(4.0);
        // 1段上・1段下からでも、右端として掴める
        assert_eq!(edge_at(&ns, 61, right, &v), Some(0), "1段上から掴めない");
        assert_eq!(edge_at(&ns, 59, right, &v), Some(0), "1段下から掴めない");
        // 2段離れたら掴まない（隣の音符を巻き込まないため）
        assert_eq!(edge_at(&ns, 62, right, &v), None, "2段離れても掴んだ");
    }

    fn note(pos: u32, len: u32, pitch: i32) -> Note {
        Note { pos, len, pitch, vel: 100, mora: String::new() }
    }

    /// **右端の帯が、音符の外側でも効くこと。**
    ///
    /// ここが「音符の中に居ること」を条件にしていたので、帯の外半分では
    /// 長さ変更にならず、四角で囲む動きに化けていた。矢印は「伸ばせます」の
    /// 形のままなので、効かない理由が画面から分からない
    #[test]
    fn the_right_edge_can_be_grabbed_from_either_side() {
        let v = view();
        let ns = vec![note(0, 4, 60)];
        // 音符は 0〜4 目盛り = 画面の 100〜140px
        let right = v.x_of(4.0);
        assert_eq!(right, 140.0);
        // 内側から
        assert_eq!(edge_at(&ns, 60, right - 3.0, &v), Some(0), "内側で掴めない");
        // **外側から。** ここが効いていなかった
        assert_eq!(edge_at(&ns, 60, right + 3.0, &v), Some(0), "外側で掴めない");
        // ちょうど端
        assert_eq!(edge_at(&ns, 60, right, &v), Some(0));
        // 遠い所は掴まない
        assert_eq!(edge_at(&ns, 60, right + 20.0, &v), None, "遠すぎるのに掴んだ");
        assert_eq!(edge_at(&ns, 60, v.x_of(1.0), &v), None, "真ん中で掴んだ");
        // 離れた段は掴まない（隣1段までは大目に見る。
        // [`a_near_miss_still_grabs_the_note`] を見よ）
        assert_eq!(edge_at(&ns, 64, right, &v), None, "離れた段で掴んだ");
    }

    /// 短い音符で、帯が音符を覆い尽くさないこと。
    ///
    /// 覆うと、今度は**動かせなくなる**。長さを変えるほうだけが効いて、
    /// 掴んで動かせない音符ができる
    #[test]
    fn a_short_note_can_still_be_grabbed_in_the_middle() {
        let v = view();
        // 1目盛り = 10px の音符。帯は 7px ではなく 4px までに縮む
        let ns = vec![note(2, 1, 60)];
        let right = v.x_of(3.0);
        assert_eq!(edge_at(&ns, 60, right, &v), Some(0), "端で掴めない");
        // 音符の頭のあたりは「端」ではないこと
        let head = v.x_of(2.0) + 1.0;
        assert_eq!(edge_at(&ns, 60, head, &v), None, "頭まで端になっている");
    }

    /// 拡大率を下げても、端が隣の音符へはみ出さないこと。
    #[test]
    fn the_band_shrinks_with_the_zoom() {
        let mut v = view();
        v.zoom = 2.0; // 1目盛り 2px。16分が線のような細さ
        let ns = vec![note(0, 1, 60), note(1, 1, 60)];
        // 1つ目の右端 = 2つ目の左端。ここは1つ目の端として掴めること
        let x = v.x_of(1.0);
        assert_eq!(edge_at(&ns, 60, x, &v), Some(0));
        // 2つ目の右端も、それとして掴めること（帯が重なって1つ目に吸われない）
        assert_eq!(edge_at(&ns, 60, v.x_of(2.0), &v), Some(1), "隣に吸われた");
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
