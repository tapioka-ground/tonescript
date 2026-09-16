@echo off
rem Tonescript (Rust 版) のコマンド。
rem   tone list            曲の一覧
rem   tone check example   曲ファイルを確かめる
rem   tone render example  音にして書き出す
rem   tone patches         使える音色
rem   tone project example 保存の状態
setlocal
cd /d "%~dp0"
set "EXE=%~dp0target\release\tone.exe"
if not exist "%EXE%" (
    echo [!] まだビルドされていません。先にこれを実行してください:
    echo       cargo build --release
    exit /b 1
)
"%EXE%" %*
endlocal
