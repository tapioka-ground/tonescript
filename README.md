# Tonescript

A DAW whose project file is text. 135 instruments, zero samples. Free, MIT.

Piano roll with a velocity lane, mixer with EQ, compression, group buses
and meters,
automation,
undo, autosave, MIDI files in and out.
Plug in a MIDI keyboard and play it. Record audio through a mic,
with a metronome and a count-in.
Notes sound the moment you place them — a real-time engine, not a re-render.
Every sound is computed — filters, reverb, mastering, all from scratch.
No plugins, no sample libraries. A 34-second track renders in 0.24s.

The song is plain text, so you can write it by hand — or hand
[SONGFILE.md](SONGFILE.md) to an AI and have it write it for you.
**The instruments are text too**, so an AI can design the sounds, not just
the notes. Guitars and basses are modelled strings, not sampled ones.

日本語は下

---

## Quick start

```
cargo build --release
```

Then double-click a `.bat`, or use the command:

| | |
|---|---|
| `tone-app.bat` | Opens the window. Pick a song, edit notes, press Space to play |
| `tone.bat` | Command line |

```
tone list               list songs
tone check    <song>    check it loads, see note counts
tone render   <song>    render to WAV
tone project  <song>    saved edits, backups, autosave
tone midi-out <song>    export MIDI
tone patches  [song]    list instruments (plus the song's own)
```

| Environment variable | Points at | Default |
|---|---|---|
| `TONESCRIPT_SONGS` | where songs live | `songs` |
| `TONESCRIPT_ROOT` | where output goes | `out` |

### Low latency

**Audio settings** in the toolbar picks the output, and shows the block size
the device actually hands us — not the one we asked for. That block is most
of the delay between pressing a key and hearing it.

On Windows the default path is WASAPI shared, and it decides the block size
itself. On the machine this was written on it gives 1056 frames (22 ms) and
ignores anything smaller. That is the floor; asking for 64 changes nothing.

To go below it you need ASIO:

```
set CPAL_ASIO_DIR=C:\path\to\asiosdk
cargo build --release --features asio
```

It needs the [ASIO SDK](https://www.steinberg.net/developer/) and LLVM
(bindgen). Neither ships with the build, which is why it is off by default —
with the feature on and the SDK missing, the build fails. Once it is in,
`ASIO` appears in the host list and the block size is yours to choose.

## What's inside

```
crates/dsp/      oscillators, envelopes, filters, 135 instruments, 11 drums, reverb
                 and the recipe format for instruments you write yourself
crates/song/     song files (Rhai), time signatures, automation
crates/render/   arrangement, synthesis, mixing, mastering, WAV
crates/engine/   real-time playback: scheduler, lock-free queue, mixer
crates/project/  edit state, autosave, undo
crates/midi/     MIDI read/write
crates/cli/      the `tone` command
crates/app/      the window (egui), audio in/out (cpal), MIDI keyboard (midir)
```

Dependencies: Rhai, rayon, egui, cpal, midir. That's all.

## Writing songs

**[SONGFILE.md](SONGFILE.md) is the full spec** — every value, its shape,
its range, its default, and what it sounds like.

The smallest song that makes a sound:

```rhai
let BPM = 120;
let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
let MELODY = #{ "1": bar([[4,"C4"], [4,"E4"], [8,"G4"]]), "2": bar([[16,"C5"]]) };
let ARRANGE = #{ "1": ["lead"], "2": ["lead"] };
```

To have an AI write one: *"Follow SONGFILE.md and write songs/x.rhai. Make it …"*
Then run `tone check x`. The loader verifies bar sums, value ranges and note
names, so if it passes, the song is structurally sound.

## Notes from the port

This started as Python (numpy + Numba + Tkinter). One rule guided the port:
**drop the tricks that only existed to work around Python's speed.**

Nearest-neighbour wavetable lookup, a one-pole filter faked with FFT
convolution, a dependency on Numba. All correct decisions in Python.
In Rust the plain loop is both faster and more accurate.

Correctness was checked with numbers, not ears. The RNG is numpy-compatible
(PCG64 + SeedSequence), so the same seed gives the same waveform as Python
and every instrument can be compared sample by sample.

| | Python | Rust | |
|---|---|---|---|
| saw @440Hz (error) | 0.00453 | 0.00028 | Rust 16× more accurate |
| saw @80Hz (error) | 0.00467 | 0.00012 | Rust 38× more accurate |
| `osc_saw`, 1s ×100 | 79.4 ms | 18.3 ms | 4.3× |
| `highpass`, 1s ×100 | 203.5 ms | 9.6 ms | **21×** |
| 920 notes | 2139 ms | 183 ms | 11.7× |
| render `example` | 4.30 s | 0.24 s | 18× |

All 11 drums match Python exactly. Of the 46 ported instruments, 20 are sample-exact
and 26 correlate above 0.999 — that difference is the wavetable
interpolation above, which is the improvement.

The 21× on `highpass` is where Python's cleverness disappeared: an FFT
convolution became a three-line loop.

492 tests.

## Not ported

Hand-drawn instrument icons (46 of them), the built-in singing voice
(PSOLA + UTAU), speech-to-pitch tracing, and music video generation.
Sing into a mic, or bring vocals in through `AUDIO_TRACKS` as WAV.

## License

MIT ([LICENSE](LICENSE)). That covers the source code, not the songs you
make with it, nor any samples or voice banks you supply.

---

# Tonescript（日本語）

プロジェクトファイルがテキストの DAW。楽器135種、音源ゼロ。無料・MIT。

ピアノロール（強さのレーン付き）、EQ とコンプとバスと針の付いたミキサー、
オートメーション、取り消し、自動保存、MIDI の読み書き。
MIDI 鍵盤を挿せば弾ける。マイクから歌も録れる（メトロノームとカウントイン付き）。
置いた音はその場で鳴る。作り直して鳴らすのではなく、鳴らしながら作っている。

音は全部計算で作っている。フィルタも残響もマスタリングも自前で、
プラグインも音源ライブラリも使わない。34秒の曲が 0.24 秒でできる。

曲はただのテキストなので、手で書いてもいいし、
[SONGFILE.ja.md](SONGFILE.ja.md) を AI に渡して書かせてもいい。
**音色もテキスト**なので、曲だけでなく音そのものを作らせられる。
ギターやベースは録った音ではなく、弦そのものを計算している。

## すぐ試す

```
cargo build --release
```

あとはフォルダの中の `.bat` をダブルクリックするだけ。

| | |
|---|---|
| `tone-app.bat` | 画面を開く。曲を選び、音符を触り、Space で鳴らす |
| `tone.bat` | コマンド |

```
tone list               曲の一覧
tone check    <曲>      読めるか、各パート何ノートか
tone render   <曲>      音にして WAV へ
tone project  <曲>      保存の状態（世代・自動保存）
tone midi-out <曲>      MIDI へ持ち出す
tone patches  [曲]      音色の一覧（曲が作った音色も）
```

| 環境変数 | 何を指すか | 既定 |
|---|---|---|
| `TONESCRIPT_SONGS` | 曲の置き場 | `songs` |
| `TONESCRIPT_ROOT` | 書き出し先 | `out` |

### 低遅延

上の「音の出口」で、どの機械で鳴らすかと塊の大きさを選ぶ。画面に出るのは
**実際に来た塊**で、頼んだ値ではない。鍵盤を押してから音が出るまでの遅れは、
ほとんどがこの塊で決まる。

Windows の既定の口（WASAPI の共有）は、塊を自分で決める。これを書いた機械
では 1056 サンプル（22ms）で、それより小さい頼みは聞かない。そこが底で、
64 を頼んでも何も変わらない。

そこから下げるには ASIO が要る。

```
set CPAL_ASIO_DIR=C:\ASIO SDK の場所
cargo build --release --features asio
```

[ASIO SDK](https://www.steinberg.net/developer/) と LLVM（bindgen が使う）が
要る。どちらも同梱できないので既定では入れていない。**SDK が無いまま
この feature を付けるとビルドが通らない。** 入れてしまえば口の一覧に `ASIO`
が出てきて、塊を自分で選べるようになる。

## 何が入っているか

```
crates/dsp/      発振器・エンベロープ・フィルタ・135音色・11ドラム・残響
                 自分で音色を作るための書式
crates/song/     曲ファイル（Rhai）、拍子、オートメーション
crates/render/   編曲・合成・ミックス・マスタリング・WAV
crates/engine/   鳴らしながら計算する側。先回り係・待たない受け渡し・ミキサー
crates/project/  編集の状態、自動保存、取り消し
crates/midi/     MIDI の読み書き
crates/cli/      tone コマンド
crates/app/      画面（egui）、音の出入り（cpal）、MIDI 鍵盤（midir）
```

依存は Rhai、rayon、egui、cpal、midir だけ。

## 曲の書き方

**全仕様は [SONGFILE.ja.md](SONGFILE.ja.md) にある。**
（英語版が正本：[SONGFILE.md](SONGFILE.md)）値ひとつずつ、形と範囲と
既定値、そして「こう書けばこう鳴る」が書いてある。

音が出る最小の曲：

```rhai
let BPM = 120;
let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
let MELODY = #{ "1": bar([[4,"C4"], [4,"E4"], [8,"G4"]]), "2": bar([[16,"C5"]]) };
let ARRANGE = #{ "1": ["lead"], "2": ["lead"] };
```

AI に書かせるなら「SONGFILE.ja.md に従って songs/x.rhai を書いて。〜な曲で」。
そのあと `tone check x`。小節の合計・値の範囲・音名は読み込み時に全部
検算されるので、通れば曲として破綻していない。

## 移植について

もとは Python（numpy + Numba + Tkinter）。方針は1つだけ。
**Python の速度を回避するための細工は、移し替えずに捨てる。**

波形テーブルの最近傍引き、1極フィルタの FFT 畳み込み、Numba への依存。
どれも Python では正しい判断だが、Rust では素直なループのほうが速く、
しかも正確になる。

合っているかは耳ではなく数字で確かめた。乱数を numpy 互換
（PCG64 + SeedSequence）にしてあるので、同じ種なら Python と同じ波形が
出て、音色を1本ずつサンプル単位で突き合わせられる。

| | Python | Rust | |
|---|---|---|---|
| ノコギリ波 440Hz（誤差） | 0.00453 | 0.00028 | Rust が 16倍 正確 |
| ノコギリ波 80Hz（誤差） | 0.00467 | 0.00012 | Rust が 38倍 正確 |
| `osc_saw` 1秒 ×100 | 79.4 ms | 18.3 ms | 4.3倍 |
| `highpass` 1秒 ×100 | 203.5 ms | 9.6 ms | **21倍** |
| 920 ノート | 2139 ms | 183 ms | 11.7倍 |
| `example` の書き出し | 4.30 s | 0.24 s | 18倍 |

ドラム11種は全部一致。移植した音色46種のうち20本がサンプル完全一致、26本が
相関 0.999 以上。差が出たぶんは上の補間の改善そのもの。

`highpass` の21倍が「Python の賢さが消えた」ところ。FFT 畳み込みが
3行のループになった。

テスト 492 件。

## 移植しなかったもの

手描きの楽器アイコン46個、内蔵の歌声合成（PSOLA + UTAU 音源）、
喋りから抑揚を写すもの、ミュージックビデオの生成。
歌はマイクで録るか、`AUDIO_TRACKS` に WAV を置く。

## ライセンス

MIT（[LICENSE](LICENSE)）。ソースコードのライセンスであって、
このソフトで作った曲や、別途用意する音源・素材には及ばない。
