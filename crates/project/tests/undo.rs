//! 取り消しの通しの確かめ。
//!
//! 画面での操作の順番をそのままなぞる。ここが合っていないと、
//! 「Ctrl+Z を押しても戻らない」「押しすぎると消える」が起きる。

use tonescript_project::store::Store;
use tonescript_project::{History, Project, Tag};
use tonescript_song::model::{Curve, Lane, Note};
use std::collections::HashMap;

fn note(pos: u32, pitch: i32) -> Note {
    Note { pos, len: 4, pitch, vel: 100, mora: String::new() }
}

/// lead の音符の数。全部取り消すとパートの鍵ごと消えるので、
/// 無いときは 0 として数える。
fn count(p: &Project) -> usize {
    p.notes.get("lead").map(|v| v.len()).unwrap_or(0)
}

/// 画面の「置く」に当たる。変える前に覚えてから足す。
fn place(h: &mut History, p: &mut Project, pos: u32, pitch: i32) {
    h.record(p, Tag::Once);
    p.add_note("lead", note(pos, pitch));
}

/// 画面の「ドラッグ」に当たる。何フレームも動かして、最後に指を離す。
fn drag(h: &mut History, p: &mut Project, index: usize, to: u32) {
    let from = p.notes["lead"][index].pos;
    let step = if to > from { 1i64 } else { -1 };
    let mut x = from as i64;
    while x != to as i64 {
        x += step;
        h.record(p, Tag::Move("lead".into(), index));
        p.notes.get_mut("lead").unwrap()[index].pos = x as u32;
    }
    h.end_group();
}

#[test]
fn place_three_then_undo_all_the_way() {
    let mut h = History::default();
    let mut p = Project::new("x");
    place(&mut h, &mut p, 0, 60);
    place(&mut h, &mut p, 4, 62);
    place(&mut h, &mut p, 8, 64);
    assert_eq!(count(&p), 3);

    for want in [2, 1, 0] {
        assert!(h.undo(&mut p), "戻せない");
        assert_eq!(count(&p), want, "1つずつ戻っていない");
    }
    assert!(!h.undo(&mut p), "空なのにまだ戻せる");
}

#[test]
fn a_long_drag_undoes_in_one_press() {
    let mut h = History::default();
    let mut p = Project::new("x");
    place(&mut h, &mut p, 0, 60);
    h.end_group();

    // 0 から 40 まで、40 フレームかけて動かす
    drag(&mut h, &mut p, 0, 40);
    assert_eq!(p.notes["lead"][0].pos, 40);

    // Ctrl+Z 一回で掴む前へ戻る
    assert!(h.undo(&mut p));
    assert_eq!(p.notes["lead"][0].pos, 0, "40回押さないと戻らない");
    // もう一回で、置く前へ
    assert!(h.undo(&mut p));
    assert!(p.notes.get("lead").is_none_or(|v| v.is_empty()));
}

#[test]
fn undo_then_redo_returns_exactly() {
    let mut h = History::default();
    let mut p = Project::new("x");
    place(&mut h, &mut p, 0, 60);
    h.end_group();
    drag(&mut h, &mut p, 0, 12);
    let after = p.clone();

    h.undo(&mut p);
    h.undo(&mut p);
    h.redo(&mut p);
    h.redo(&mut p);
    assert_eq!(p, after, "やり直しで元に戻らない");
}

#[test]
fn editing_after_undo_drops_the_redo_branch() {
    let mut h = History::default();
    let mut p = Project::new("x");
    place(&mut h, &mut p, 0, 60);
    place(&mut h, &mut p, 8, 64);
    h.undo(&mut p); // 8 のほうを取り消した
    assert!(h.can_redo());

    // ここで別のことをしたら、やり直せる先は消える
    place(&mut h, &mut p, 16, 67);
    assert!(!h.can_redo());
    assert_eq!(p.notes["lead"].len(), 2);
    let poss: Vec<u32> = p.notes["lead"].iter().map(|n| n.pos).collect();
    assert_eq!(poss, vec![0, 16]);
}

#[test]
fn undo_covers_automation_and_mute_too() {
    let mut h = History::default();
    let mut p = Project::new("x");
    place(&mut h, &mut p, 0, 60);

    h.record(&p, Tag::Once);
    p.automation.insert(
        "lead".into(),
        HashMap::from([(Lane::Gain, Curve::new(vec![(0, 1.0), (64, 0.0)]))]),
    );
    h.record(&p, Tag::Once);
    p.muted.push("bass".into());

    h.undo(&mut p);
    assert!(p.muted.is_empty(), "ミュートが戻らない");
    h.undo(&mut p);
    assert!(p.automation.is_empty(), "線が戻らない");
    h.undo(&mut p);
    assert!(p.notes.get("lead").is_none_or(|v| v.is_empty()), "音符が戻らない");
}

#[test]
fn undone_state_is_what_gets_saved() {
    // 取り消したあとに保存したら、取り消し後のものが保存されること。
    let mut d = std::env::temp_dir();
    d.push(format!("tsp_undo_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();

    let mut h = History::default();
    let mut p = Project::new("x");
    place(&mut h, &mut p, 0, 60);
    place(&mut h, &mut p, 8, 64);
    h.undo(&mut p);

    let st = Store::new(&d, "x");
    st.save(&p).unwrap();
    let back = st.load().unwrap().unwrap();
    assert_eq!(back.notes["lead"].len(), 1, "取り消す前のものが保存された");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn deep_history_does_not_lose_recent_work() {
    // 上限を超えて操作しても、直近ぶんは必ず戻せること。
    let mut h = History::new(10);
    let mut p = Project::new("x");
    for i in 0..50u32 {
        place(&mut h, &mut p, i * 4, 60);
    }
    assert_eq!(p.notes["lead"].len(), 50);
    for _ in 0..10 {
        assert!(h.undo(&mut p));
    }
    assert_eq!(p.notes["lead"].len(), 40, "直近 10 手が戻らない");
    assert!(!h.undo(&mut p), "上限を超えて戻せてしまう");
}
