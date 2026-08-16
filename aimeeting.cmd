@echo off
setlocal EnableExtensions

cd /d "%~dp0"

set "ACTION=%~1"
if "%ACTION%"=="" set "ACTION=dev"

if /I "%ACTION%"=="dev" goto :dev
if /I "%ACTION%"=="web" goto :web
if /I "%ACTION%"=="check" goto :check
if /I "%ACTION%"=="build" goto :build
goto :help

:dev
call npm.cmd run desktop:dev
exit /b %ERRORLEVEL%

:web
call npm.cmd run dev -- --host 127.0.0.1
exit /b %ERRORLEVEL%

:check
call npm.cmd run lint
if errorlevel 1 exit /b %ERRORLEVEL%
call cargo test --manifest-path src-tauri\Cargo.toml
exit /b %ERRORLEVEL%

:build
call npm.cmd run build
exit /b %ERRORLEVEL%

:help
echo Usage: aimeeting.cmd [dev^|web^|check^|build]
exit /b 0
