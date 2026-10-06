@echo off
rem Launches the native Angry Birds Go! build. Builds it first if the exe is missing.
rem Extra arguments are passed through, e.g.  "Play Angry Birds Go.bat" --landing   (skip the intro)
cd /d "%~dp0game"
if not exist "target\fast\abg-game.exe" (
    echo Building, first run only...
    cargo build --profile fast || (pause & exit /b 1)
)
start "" "target\fast\abg-game.exe" %*
