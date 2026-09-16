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
