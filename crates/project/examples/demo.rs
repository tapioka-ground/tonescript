//! 自動保存の流れを、実際のファイルで再現してみせる。
//!
//!   demo save              手で保存する
//!   demo edit-then-crash   さらに直して、自動保存だけ走らせる
//!   demo recover           開き直したときに何が見えるか

use tonescript_project::store::Store;
use tonescript_project::Project;
use tonescript_song::model::Note;

fn note(i: u32) -> Note {
    Note { pos: i * 4, len: 4, pitch: 60 + i as i32, vel: 100, mora: String::new() }
}

fn main() {
    let dir = std::env::var("TONESCRIPT_PROJECTS").expect("TONESCRIPT_PROJECTS を設定してください");
    let st = Store::new(&dir, "odd");
    match std::env::args().nth(1).unwrap_or_default().as_str() {
        "save" => {
            let mut p = Project::new("odd");
            for i in 0..4 {
                p.add_note("lead", note(i));
            }
            st.save(&p).unwrap();
            println!("手で保存した（4 ノート）");
        }
        "edit-then-crash" => {
            let mut p = st.load().unwrap().expect("先に save してください");
            for i in 4..9 {
                p.add_note("lead", note(i));
            }
            st.autosave(&p).unwrap();
            println!("さらに 5 ノート足して、自動保存だけ走った（手では保存していない）");
            println!("…ここで落ちたことにする");
        }
        "recover" => {
            let main = st.load().unwrap().expect("本体が無い");
            println!("本体には      {} ノート", main.notes["lead"].len());
            match st.load_autosave().unwrap() {
                Some(a) => println!(
                    "自動保存には  {} ノート  -> 戻せば作業は消えない",
                    a.notes["lead"].len()
                ),
                None => println!("自動保存なし"),
            }
        }
        _ => println!("save / edit-then-crash / recover のどれかを渡してください"),
    }
}
