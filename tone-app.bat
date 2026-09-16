@echo off
rem Tonescript (Rust 版) の画面を開く。
rem   曲は songs\*.rhai、書き出しは out\ へ。
setlocal
cd /d "%~dp0"
set "EXE=%~dp0target\release\tone-app.exe"
if not exist "%EXE%" (
    echo [!] まだビルドされていません。先にこれを実行してください:
    echo       cargo build --release
    pause
    exit /b 1
)
start "" "%EXE%" %*
endlocal
