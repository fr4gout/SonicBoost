@echo off
rem Pushes SonicBoost to https://github.com/fr4gout/SonicBoost and tags v1.0.0
rem -> GitHub Actions builds the installers and publishes the Release.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0publish.ps1" %*
pause
