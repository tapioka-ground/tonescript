//! 通しの確かめ。落ちたあとに作業が戻るか、というのがここの本題。

use tonescript_project::store::{Autosaver, Store, BACKUPS};
use tonescript_project::Project;
use tonescript_song::model::{Curve, Lane, Note};
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn dir(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("tsp_life_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn note(pos: u32, pitch: i32) -> Note {
    Note { pos, len: 4, pitch, vel: 100, mora: String::new() }
}

#[test]
fn work_survives_a_crash() {
    let d = dir("crash");
    let st = Store::new(&d, "example");

    // 1. 何か作って手で保存
    let mut p = Project::new("example");
    p.add_note("lead", note(0, 60));
    st.save(&p).unwrap();

    // 2. さらに直す。自動保存だけが走る（手では保存していない）
    p.add_note("lead", note(16, 64));
    p.add_note("lead", note(32, 67));
    st.autosave(&p).unwrap();

    // 3. ここで落ちたことにする。新しく開き直す
    let st2 = Store::new(&d, "example");
    let opened = st2.load().unwrap().unwrap();
    assert_eq!(opened.notes["lead"].len(), 1, "本体は保存した時点のまま");

    // 4. 「前回きちんと閉じていない」と気づく
    assert!(st2.pending_autosave().is_some(), "落ちたことに気づけていない");

    // 5. 戻すと、保存していなかったぶんまで返ってくる
    let back = st2.load_autosave().unwrap().unwrap();
    assert_eq!(back.notes["lead"].len(), 3, "作業が消えた");

    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn clean_exit_leaves_nothing_pending() {
    let d = dir("clean");
    let st = Store::new(&d, "example");
    let mut p = Project::new("example");
    p.add_note("lead", note(0, 60));
    st.autosave(&p).unwrap();
    st.save(&p).unwrap(); // きちんと保存して閉じた
    assert!(st.pending_autosave().is_none(), "閉じたのに問いかけが出る");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_bad_save_does_not_destroy_the_previous_one() {
    let d = dir("keep");
    let st = Store::new(&d, "example");

    let mut good = Project::new("example");
    good.add_note("lead", note(0, 60));
    st.save(&good).unwrap();

    // 間違えて全部消して保存してしまった
    let empty = Project::new("example");
    st.save(&empty).unwrap();
    assert!(st.load().unwrap().unwrap().notes.is_empty());

    // 世代から戻せる
    let back = st.load_backup(1).unwrap();
    assert_eq!(back.notes["lead"].len(), 1, "戻せない");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn everything_round_trips_through_the_file() {
    let d = dir("all");
    let st = Store::new(&d, "example");
    let mut p = Project::new("example");
    p.add_note("lead", Note { pos: 4, len: 7, pitch: 71, vel: 88, mora: "ら".into() });
    p.add_note("bass", note(0, 40));
    p.automation.insert(
        "lead".into(),
        HashMap::from([
            (Lane::Gain, Curve::new(vec![(0, 1.0), (64, 0.3)])),
            (Lane::Pan, Curve::new(vec![(0, -1.0), (64, 1.0)])),
        ]),
    );
    p.gains.insert("bass".into(), 1.4);
    p.muted.push("perc".into());
    p.soloed.push("lead".into());

    st.save(&p).unwrap();
    let back = st.load().unwrap().unwrap();
    assert_eq!(back, p, "保存して読み返したら変わった");
    // 歌詞も残る
    assert_eq!(back.notes["lead"][0].mora, "ら");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn autosave_fires_only_after_the_wait() {
    let mut a = Autosaver::new(Duration::from_secs(20));
    let t0 = Instant::now();
    a.touched_at(t0);
    assert!(!a.is_dirty() == false, "直したのに覚えていない");
    assert!(!a.due_at(t0), "直した瞬間に書きに行く");
    assert!(!a.due_at(t0 + Duration::from_secs(19)), "早すぎる");
    assert!(a.due_at(t0 + Duration::from_secs(20)), "遅すぎる");
    a.saved();
    assert!(!a.due_at(t0 + Duration::from_secs(600)), "保存後も書き続ける");
}

#[test]
fn many_saves_do_not_grow_without_bound() {
    let d = dir("bound");
    let st = Store::new(&d, "example");
    for i in 0..40 {
        let mut p = Project::new("example");
        p.add_note("lead", note(i, 60));
        st.save(&p).unwrap();
    }
    assert_eq!(st.backups().len(), BACKUPS);
    // 置き場にあるファイルの数も抑えられている
    let files = std::fs::read_dir(&d).unwrap().flatten().count();
    assert!(files <= 3, "散らかっている: {files} 個");
    std::fs::remove_dir_all(&d).ok();
}
