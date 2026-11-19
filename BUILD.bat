@echo off
rem Double-click to build the SonicBoost installer.
rem Add  -InstallTools  after the script name below on first run if Rust/Node are not installed.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-installer.ps1" %*
pause
