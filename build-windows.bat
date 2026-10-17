@echo off
title Backlog Panda - compilation
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\build-windows.ps1"
echo.
pause
