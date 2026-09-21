//! 選ぶ・まとめて動かす・写して貼る。
//!
//! 画面を触らずに確かめられるよう、**計算だけをここへ出してある。**
//! 「どれが囲まれたか」「貼ったらどこへ入るか」は画面が無くても決まる。
//!
//! 選びかた
//! --------
//! 選んでいるものは「今のパートの何番目か」の並びで持つ。音符を足したり
//! 消したりすると番号がずれるので、**形が変わる操作のあとは選び直す**
//! （貼ったら貼ったものが選ばれ、消したら何も選ばれていない状態になる）。

use tonescript_song::model::Note;

/// 写したもの。**先頭を 0 に寄せて持つ**ので、どこへでも貼れる。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    pub notes: Vec<Note>,
    /// 元の長さ（目盛り）。続けて貼るときの間隔に使う
    pub span: u32,
}

impl Clip {
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

/// 囲んだ四角に入っている音符。
///
/// 端が少しでも重なっていれば入っているとみなす。**完全に含むものだけ**に
/// すると、長い音符が掴めなくて「選べない」と言われる。
pub fn marquee(notes: &[Note], from: (f32, i32), to: (f32, i32)) -> Vec<usize> {
    let (s0, s1) = (from.0.min(to.0), from.0.max(to.0));
    let (p0, p1) = (from.1.min(to.1), from.1.max(to.1));
    notes
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            let (a, b) = (n.pos as f32, (n.pos + n.len.max(1)) as f32);
            b > s0 && a < s1 && n.pitch >= p0 && n.pitch <= p1
        })
        .map(|(i, _)| i)
        .collect()
}

/// 選んでいるものを写す。先頭を 0 に寄せる。
pub fn copy(notes: &[Note], sel: &[usize]) -> Clip {
    let mut picked: Vec<Note> = sel.iter().filter_map(|i| notes.get(*i).cloned()).collect();
    if picked.is_empty() {
        return Clip::default();
    }
    picked.sort_by_key(|n| (n.pos, n.pitch));
    let base = picked.iter().map(|n| n.pos).min().unwrap_or(0);
    let end = picked.iter().map(|n| n.pos + n.len.max(1)).max().unwrap_or(0);
    for n in &mut picked {
        n.pos -= base;
    }
    Clip { notes: picked, span: end - base }
}

/// 貼る。`at` から置く。曲の外へ出るぶんは落とす。
///
/// 返すのは「足す音符」。呼ぶ側が既存の後ろへ繋ぐ。
pub fn paste(clip: &Clip, at: u32, total: u32) -> Vec<Note> {
    if clip.is_empty() || total == 0 {
        return Vec::new();
    }
    clip.notes
        .iter()
        .filter_map(|n| {
            let pos = at + n.pos;
            if pos >= total {
                return None; // 曲の外。落とす
            }
            let len = n.len.max(1).min(total - pos);
            Some(Note { pos, len, ..n.clone() })
        })
        .collect()
}

/// まとめて動かす。動かせたら `true`。
///
/// **1つでも端に当たるなら、全部動かさない。** 当たったものだけ止めると、
/// 和音がばらけて戻せなくなる。
pub fn nudge(notes: &mut [Note], sel: &[usize], dstep: i32, dpitch: i32, total: u32) -> bool {
    if sel.is_empty() || (dstep == 0 && dpitch == 0) {
        return false;
    }
    for i in sel {
        let Some(n) = notes.get(*i) else { continue };
        let pos = n.pos as i64 + dstep as i64;
        let pitch = n.pitch + dpitch;
        if pos < 0 || pos + n.len.max(1) as i64 > total as i64 {
            return false;
        }
        if !(0..=127).contains(&pitch) {
            return false;
        }
    }
    for i in sel {
        let Some(n) = notes.get_mut(*i) else { continue };
        n.pos = (n.pos as i64 + dstep as i64) as u32;
        n.pitch += dpitch;
    }
    true
}

/// 目盛りへ揃える。
///
/// `strength` は寄せる割合。1.0 でぴったり、0.5 で半分だけ寄る。
/// **ぴったりにすると死ぬ**ことがあるので、割合を選べるようにしてある。
/// 人が弾いた僅かなずれが「ノリ」なので、全部消すと機械になる。
///
/// 長さは変えない。頭だけを揃えるのが普通で、長さまで揃えると
/// 切れ目が不自然になる。
pub fn quantize(notes: &mut [Note], sel: &[usize], grid: u32, strength: f32, total: u32) -> bool {
    let g = grid.max(1) as i64;
    let k = strength.clamp(0.0, 1.0);
    if sel.is_empty() || k <= 0.0 {
        return false;
    }
    let mut changed = false;
    for i in sel {
        let Some(n) = notes.get_mut(*i) else { continue };
        let pos = n.pos as i64;
        // いちばん近い目盛り
        let near = ((pos as f64 / g as f64).round() as i64) * g;
        let want = pos + ((near - pos) as f32 * k).round() as i64;
        let len = n.len.max(1) as i64;
        let want = want.clamp(0, (total as i64 - len).max(0));
        if want != pos {
            n.pos = want as u32;
            changed = true;
        }
    }
    changed
}

/// 長さを目盛りへ揃える。短すぎるものは1目盛り残す。
pub fn quantize_len(notes: &mut [Note], sel: &[usize], grid: u32, total: u32) -> bool {
    let g = grid.max(1);
    let mut changed = false;
    for i in sel {
        let Some(n) = notes.get_mut(*i) else { continue };
        let near = (((n.len.max(1) as f64) / g as f64).round() as u32 * g).max(g);
        let len = near.min(total.saturating_sub(n.pos)).max(1);
        if n.len != len {
            n.len = len;
            changed = true;
        }
    }
    changed
}

/// 強さを変える。
///
/// 掴んだものが**選んでいるものの1つなら、選んだぶん全部を同じだけ**動かす。
/// 1本だけ動かすと、和音の中の1音だけが浮く。
///
/// 返り値は変わったかどうか。
pub fn set_vel(notes: &mut [Note], index: usize, want: u8, sel: &[usize]) -> bool {
    let Some(now) = notes.get(index) else { return false };
    let want = want.clamp(1, 127);
    if now.vel == want {
        return false;
    }
    let delta = want as i32 - now.vel as i32;
    if sel.contains(&index) && sel.len() > 1 {
        for i in sel {
            if let Some(n) = notes.get_mut(*i) {
                n.vel = (n.vel as i32 + delta).clamp(1, 127) as u8;
            }
        }
    } else if let Some(n) = notes.get_mut(index) {
        n.vel = want;
    }
    true
}

/// 選んでいるものの強さを揃える。
///
/// 打ち込んだままだと全部 100 で平らに聞こえる。まず揃えてから、
/// 傾ける・数本だけ持ち上げる、という順で触ることが多い。
pub fn flatten(notes: &mut [Note], sel: &[usize], vel: u8) -> bool {
    let vel = vel.clamp(1, 127);
    let mut changed = false;
    for i in sel {
        if let Some(n) = notes.get_mut(*i) {
            changed |= n.vel != vel;
            n.vel = vel;
        }
    }
    changed
}

/// 選んでいるものを、位置の順に `from` から `to` へ傾ける。
///
/// **音符の並び順ではなく、目盛りの位置で決める。** 並び順で傾けると、
/// 後から足した音符が飛び飛びに強くなる。
pub fn ramp(notes: &mut [Note], sel: &[usize], from: u8, to: u8) -> bool {
    if sel.len() < 2 {
        return false;
    }
    let mut ps: Vec<(usize, u32)> =
        sel.iter().filter_map(|i| notes.get(*i).map(|n| (*i, n.pos))).collect();
    if ps.len() < 2 {
        return false;
    }
    ps.sort_by_key(|(_, p)| *p);
    let (first, last) = (ps[0].1 as f32, ps[ps.len() - 1].1 as f32);
    let span = (last - first).max(1.0);
    let (a, b) = (from.clamp(1, 127) as f32, to.clamp(1, 127) as f32);
    let mut changed = false;
    for (i, pos) in ps {
        let t = (pos as f32 - first) / span;
        let v = (a + (b - a) * t).round().clamp(1.0, 127.0) as u8;
        if let Some(n) = notes.get_mut(i) {
            changed |= n.vel != v;
            n.vel = v;
        }
    }
    changed
}

/// 選んでいるものの強さの平均。揃えるときの目安に使う。
pub fn mean_vel(notes: &[Note], sel: &[usize]) -> Option<u8> {
    let vs: Vec<u16> = sel.iter().filter_map(|i| notes.get(*i)).map(|n| n.vel as u16).collect();
    if vs.is_empty() {
        return None;
    }
    Some((vs.iter().sum::<u16>() / vs.len() as u16) as u8)
}

/// その目盛りの所にある音符。強さのレーンで掴むのに使う。
///
/// 重なっていたら**上の音**を採る（見えているものが掴める）。
pub fn at_step(notes: &[Note], step: f32) -> Option<usize> {
    let mut best: Option<(usize, i32)> = None;
    for (i, n) in notes.iter().enumerate() {
        if (n.pos as f32) <= step && step < (n.pos + n.len.max(1)) as f32 {
            if best.is_none_or(|(_, p)| n.pitch > p) {
                best = Some((i, n.pitch));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// 選んでいるものを消す。消したあとの並びを返す。
pub fn remove(notes: &[Note], sel: &[usize]) -> Vec<Note> {
    notes
        .iter()
        .enumerate()
        .filter(|(i, _)| !sel.contains(i))
        .map(|(_, n)| n.clone())
        .collect()
}

/// 今の選択の次へ複製する。返すのは `(足す音符, 新しく選ぶ番号)`。
pub fn duplicate(notes: &[Note], sel: &[usize], total: u32) -> (Vec<Note>, Vec<usize>) {
    let clip = copy(notes, sel);
    if clip.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let base = sel.iter().filter_map(|i| notes.get(*i)).map(|n| n.pos).min().unwrap_or(0);
    let add = paste(&clip, base + clip.span.max(1), total);
    let first = notes.len();
    let picked = (first..first + add.len()).collect();
    (add, picked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(pos: u32, len: u32, pitch: i32) -> Note {
        Note { pos, len, pitch, vel: 100, mora: String::new() }
    }

    fn sample() -> Vec<Note> {
        vec![n(0, 4, 60), n(4, 4, 64), n(8, 8, 67), n(16, 4, 72)]
    }

    #[test]
    fn a_box_catches_what_it_touches() {
        let ns = sample();
        // 0〜8 目盛り、音程 60〜64 を囲む
        let got = marquee(&ns, (0.0, 60), (8.0, 64));
        assert_eq!(got, vec![0, 1]);
        // 逆向きに引いても同じ
        assert_eq!(marquee(&ns, (8.0, 64), (0.0, 60)), vec![0, 1]);
    }

    #[test]
    fn a_long_note_is_caught_by_its_middle() {
        // 端だけ重なっていれば掴める。完全に含まないと選べないのでは使えない
        let ns = sample();
        let got = marquee(&ns, (10.0, 67), (11.0, 67));
        assert_eq!(got, vec![2], "長い音符の途中を囲んでも選べない");
    }

    #[test]
    fn an_empty_box_catches_nothing() {
        let ns = sample();
        assert!(marquee(&ns, (30.0, 60), (40.0, 70)).is_empty());
        assert!(marquee(&ns, (0.0, 100), (20.0, 110)).is_empty(), "音程が外れているのに選んだ");
    }

    #[test]
    fn copying_moves_the_start_to_zero() {
        let ns = sample();
        let c = copy(&ns, &[1, 2]);
        assert_eq!(c.notes.len(), 2);
        assert_eq!(c.notes[0].pos, 0, "先頭が 0 に寄っていない");
        assert_eq!(c.notes[1].pos, 4);
        // 4 から 16 まで＝12 目盛りぶん
        assert_eq!(c.span, 12);
    }

    #[test]
    fn copying_nothing_gives_nothing() {
        assert!(copy(&sample(), &[]).is_empty());
        assert!(copy(&sample(), &[99]).is_empty(), "無い番号で何か写った");
    }

    #[test]
    fn pasting_puts_it_where_it_was_asked() {
        let c = copy(&sample(), &[0, 1]);
        let got = paste(&c, 32, 64);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].pos, 32);
        assert_eq!(got[1].pos, 36);
        assert_eq!(got[0].pitch, 60);
    }

    #[test]
    fn pasting_past_the_end_drops_what_sticks_out() {
        let c = copy(&sample(), &[0, 1]);
        // 曲が 34 目盛りしかない。2つめ（+4）は入らない
        let got = paste(&c, 32, 34);
        assert_eq!(got.len(), 1, "曲の外へ貼った");
        assert_eq!(got[0].len, 2, "端で切り詰めていない");
        // まるごと外なら何も入らない
        assert!(paste(&c, 100, 34).is_empty());
    }

    #[test]
    fn nudging_moves_everything_together() {
        let mut ns = sample();
        assert!(nudge(&mut ns, &[0, 1], 2, 0, 64));
        assert_eq!(ns[0].pos, 2);
        assert_eq!(ns[1].pos, 6);
        assert_eq!(ns[2].pos, 8, "選んでいないものが動いた");
    }

    #[test]
    fn nudging_transposes_too() {
        let mut ns = sample();
        assert!(nudge(&mut ns, &[0, 1], 0, 12, 64));
        assert_eq!(ns[0].pitch, 72);
        assert_eq!(ns[1].pitch, 76);
    }

    #[test]
    fn nothing_moves_if_one_of_them_cannot() {
        // 和音がばらけないこと
        let mut ns = sample();
        let before = ns.clone();
        assert!(!nudge(&mut ns, &[0, 1], -1, 0, 64), "頭を突き抜けた");
        assert_eq!(ns, before, "止まったのに一部だけ動いた");

        let mut ns = vec![n(0, 4, 0), n(4, 4, 60)];
        let before = ns.clone();
        assert!(!nudge(&mut ns, &[0, 1], 0, -1, 64), "音程が 0 より下へ行った");
        assert_eq!(ns, before);

        let mut ns = sample();
        let before = ns.clone();
        assert!(!nudge(&mut ns, &[3], 100, 0, 64), "曲の外へ出た");
        assert_eq!(ns, before);
    }

    #[test]
    fn quantizing_pulls_notes_onto_the_grid() {
        let mut ns = vec![n(1, 4, 60), n(7, 4, 62), n(9, 4, 64)];
        assert!(quantize(&mut ns, &[0, 1, 2], 4, 1.0, 64));
        assert_eq!(ns[0].pos, 0, "1 が 0 へ");
        assert_eq!(ns[1].pos, 8, "7 が 8 へ");
        assert_eq!(ns[2].pos, 8, "9 が 8 へ");
        // 長さは触らない
        assert_eq!(ns[0].len, 4);
    }

    #[test]
    fn a_note_exactly_between_two_lines_goes_forward() {
        // 10 は 8 と 12 のちょうど真ん中。どちらへ行くかは決めておく
        let mut ns = vec![n(10, 4, 60)];
        quantize(&mut ns, &[0], 4, 1.0, 64);
        assert_eq!(ns[0].pos, 12, "真ん中のときは後ろへ");
    }

    #[test]
    fn half_strength_moves_half_way() {
        // ノリを残すための割合。全部消すと機械になる
        let mut ns = vec![n(2, 4, 60)];
        quantize(&mut ns, &[0], 4, 0.5, 64);
        assert_eq!(ns[0].pos, 3, "2 から 4 へ半分なら 3");
    }

    #[test]
    fn quantizing_zero_strength_does_nothing() {
        let mut ns = vec![n(3, 4, 60)];
        assert!(!quantize(&mut ns, &[0], 4, 0.0, 64));
        assert_eq!(ns[0].pos, 3);
    }

    #[test]
    fn quantizing_never_pushes_a_note_out_of_the_song() {
        let mut ns = vec![n(62, 4, 60)];
        quantize(&mut ns, &[0], 8, 1.0, 64);
        assert!(ns[0].pos + ns[0].len <= 64, "曲の外へ出た: {}", ns[0].pos);
    }

    #[test]
    fn note_lengths_can_be_tidied_too() {
        let mut ns = vec![n(0, 3, 60), n(8, 9, 62)];
        assert!(quantize_len(&mut ns, &[0, 1], 4, 64));
        assert_eq!(ns[0].len, 4, "3 が 4 へ");
        assert_eq!(ns[1].len, 8, "9 が 8 へ");
        // 頭は動かない
        assert_eq!(ns[1].pos, 8);
    }

    #[test]
    fn a_tidied_length_never_reaches_past_the_end() {
        let mut ns = vec![n(60, 3, 60)];
        quantize_len(&mut ns, &[0], 8, 64);
        assert!(ns[0].pos + ns[0].len <= 64);
        assert!(ns[0].len >= 1, "長さが 0 になった");
    }

    #[test]
    fn changing_one_velocity_leaves_the_others_alone() {
        let mut ns = sample();
        assert!(set_vel(&mut ns, 1, 60, &[]));
        assert_eq!(ns[1].vel, 60);
        assert_eq!(ns[0].vel, 100, "他の音まで変わった");
        // 同じ値なら何もしない
        assert!(!set_vel(&mut ns, 1, 60, &[]));
    }

    #[test]
    fn changing_a_selected_velocity_moves_them_all_by_the_same_amount() {
        // 和音の中の1音だけが浮かないこと
        let mut ns = vec![n(0, 4, 60), n(0, 4, 64), n(0, 4, 67)];
        ns[1].vel = 80;
        ns[2].vel = 120;
        assert!(set_vel(&mut ns, 0, 110, &[0, 1, 2]));
        assert_eq!(ns[0].vel, 110);
        assert_eq!(ns[1].vel, 90, "同じ差だけ動いていない");
        assert_eq!(ns[2].vel, 127, "上限で止まっていない");
    }

    #[test]
    fn velocity_stays_inside_one_to_one_twenty_seven() {
        let mut ns = sample();
        set_vel(&mut ns, 0, 0, &[]);
        assert_eq!(ns[0].vel, 1, "0 になった（鳴らない音符ができる）");
        set_vel(&mut ns, 0, 200, &[]);
        assert_eq!(ns[0].vel, 127);
    }

    #[test]
    fn flattening_makes_them_all_the_same() {
        let mut ns = sample();
        ns[0].vel = 40;
        ns[1].vel = 120;
        assert!(flatten(&mut ns, &[0, 1], 90));
        assert_eq!(ns[0].vel, 90);
        assert_eq!(ns[1].vel, 90);
        assert_eq!(ns[2].vel, 100, "選んでいないものが変わった");
        // もう一度やっても変わらない
        assert!(!flatten(&mut ns, &[0, 1], 90));
    }

    #[test]
    fn a_ramp_follows_the_positions_not_the_order() {
        // 後から足した音符が飛び飛びに強くならないこと
        let mut ns = vec![n(16, 4, 60), n(0, 4, 62), n(8, 4, 64)];
        assert!(ramp(&mut ns, &[0, 1, 2], 40, 120));
        assert_eq!(ns[1].vel, 40, "いちばん前が始まりの値になっていない");
        assert_eq!(ns[2].vel, 80, "真ん中が半分になっていない");
        assert_eq!(ns[0].vel, 120, "いちばん後ろが終わりの値になっていない");
    }

    #[test]
    fn a_ramp_can_go_down_too() {
        let mut ns = sample();
        ramp(&mut ns, &[0, 1, 2, 3], 120, 40);
        assert_eq!(ns[0].vel, 120);
        assert!(ns[3].vel < ns[0].vel, "下がっていない");
    }

    #[test]
    fn one_note_cannot_be_ramped() {
        let mut ns = sample();
        assert!(!ramp(&mut ns, &[0], 40, 120), "1本で傾けた");
        assert_eq!(ns[0].vel, 100);
    }

    #[test]
    fn the_average_is_what_flattening_starts_from() {
        let mut ns = sample();
        ns[0].vel = 60;
        ns[1].vel = 100;
        assert_eq!(mean_vel(&ns, &[0, 1]), Some(80));
        assert_eq!(mean_vel(&ns, &[]), None);
    }

    #[test]
    fn the_top_note_is_the_one_you_grab() {
        // 重なっていたら、見えている上の音が掴める
        let ns = vec![n(0, 8, 60), n(0, 8, 72), n(0, 8, 64)];
        assert_eq!(at_step(&ns, 3.0), Some(1));
        assert_eq!(at_step(&ns, 9.0), None, "音符の無い所で掴めた");
    }

    #[test]
    fn removing_keeps_the_rest_in_order() {
        let ns = sample();
        let left = remove(&ns, &[1, 2]);
        assert_eq!(left.len(), 2);
        assert_eq!(left[0].pos, 0);
        assert_eq!(left[1].pos, 16);
    }

    #[test]
    fn duplicating_lands_right_after_the_original() {
        let ns = sample();
        // 0〜8 の2つ（8目盛りぶん）を複製すると、8 から置かれる
        let (add, picked) = duplicate(&ns, &[0, 1], 64);
        assert_eq!(add.len(), 2);
        assert_eq!(add[0].pos, 8);
        assert_eq!(add[1].pos, 12);
        assert_eq!(picked, vec![4, 5], "複製したものが選ばれていない");
    }

    #[test]
    fn duplicating_at_the_end_of_the_song_adds_nothing() {
        let ns = sample();
        let (add, picked) = duplicate(&ns, &[3], 20);
        assert!(add.is_empty(), "曲の外へ複製した");
        assert!(picked.is_empty());
    }
}
