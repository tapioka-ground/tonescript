//! 道具側の設定。曲ではなく**この機械**に付いているもの。
//!
//! 曲の設定（[`crate::settings`]）は曲ファイルへ書き戻す。あれは曲の一部で、
//! 誰が開いても同じでなければいけない。こちらは逆で、どの音の口を使うか、
//! 塊をいくつにするかは、その人の機械の話。曲ファイルへ書くと、渡した
//! 相手の機械で音が出なくなる。
//!
//! なので別のファイルへ置く。無ければおまかせで動く。

use std::path::{Path, PathBuf};

use tonescript_project::json::{self, Value};

/// この機械の設定。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    pub audio: crate::play::Prefs,
}

impl Prefs {
    /// 置き場所。書き出し先と同じ所に置く（消えても設定が戻るだけ）。
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("prefs.json")
    }

    /// 読む。無ければ既定。**壊れていても既定で続ける。**
    ///
    /// ここで止まると、設定ファイルが1つ壊れただけで道具が起動しない
    pub fn load(dir: &Path) -> Prefs {
        let Ok(text) = std::fs::read_to_string(Self::path(dir)) else {
            return Prefs::default();
        };
        let Ok(v) = json::parse(&text) else {
            return Prefs::default();
        };
        let mut p = Prefs::default();
        if let Some(a) = v.get("audio") {
            let text = |k: &str| a.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
            p.audio.host = text("host");
            p.audio.device = text("device");
            // 知らない大きさは受け取らない。選べない値が入ると画面が迷う
            let n = a.get("buffer").and_then(|x| x.as_u32()).unwrap_or(0);
            p.audio.buffer = if crate::play::Prefs::SIZES.contains(&n) { n } else { 0 };
        }
        p
    }

    /// 書く。失敗しても理由を返すだけで、作業は止めない。
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let mut audio = Value::obj();
        audio.insert("host", self.audio.host.as_str().into());
        audio.insert("device", self.audio.device.as_str().into());
        audio.insert("buffer", self.audio.buffer.into());
        let mut root = Value::obj();
        root.insert("audio", audio);
        if let Some(parent) = Self::path(dir).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(Self::path(dir), json::to_string(&root)).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tonescript_prefs_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn nothing_saved_means_leave_it_to_the_machine() {
        let d = tmpdir("empty");
        let p = Prefs::load(&d);
        assert_eq!(p, Prefs::default());
        assert!(p.audio.host.is_empty(), "勝手に口を選んでいる");
        assert_eq!(p.audio.buffer, 0, "勝手に塊を決めている");
    }

    #[test]
    fn it_comes_back_the_way_it_went_in() {
        let d = tmpdir("round");
        let p = Prefs {
            audio: crate::play::Prefs {
                host: "ASIO".into(),
                device: "うちの機械".into(),
                buffer: 128,
            },
        };
        p.save(&d).unwrap();
        assert_eq!(Prefs::load(&d), p);
    }

    #[test]
    fn a_broken_file_does_not_stop_the_tool() {
        let d = tmpdir("broken");
        std::fs::write(Prefs::path(&d), "{これは JSON ではない").unwrap();
        assert_eq!(Prefs::load(&d), Prefs::default(), "壊れたファイルで止まった");
    }

    #[test]
    fn a_size_we_cannot_offer_is_dropped() {
        // 選べない値が残っていると、画面のどれにも当たらず固まって見える
        let d = tmpdir("odd");
        let p = Prefs {
            audio: crate::play::Prefs { buffer: 333, ..Default::default() },
        };
        p.save(&d).unwrap();
        assert_eq!(Prefs::load(&d).audio.buffer, 0, "選べない大きさが残った");
    }
}
