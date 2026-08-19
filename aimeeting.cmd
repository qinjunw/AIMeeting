@echo off
setlocal EnableExtensions

cd /d "%~dp0"

set "ACTION=%~1"
if "%ACTION%"=="" set "ACTION=dev"

if /I "%ACTION%"=="start" set "ACTION=dev"
if /I "%ACTION%"=="desktop" set "ACTION=dev"
if /I "%ACTION%"=="package" set "ACTION=release"

if /I "%ACTION%"=="help" goto :help
if /I "%ACTION%"=="-h" goto :help
if /I "%ACTION%"=="--help" goto :help

if /I "%ACTION%"=="dev" goto :dev
if /I "%ACTION%"=="web" goto :web
if /I "%ACTION%"=="lint" goto :lint
if /I "%ACTION%"=="build" goto :build
if /I "%ACTION%"=="test" goto :test
if /I "%ACTION%"=="check" goto :check
if /I "%ACTION%"=="format" goto :format
if /I "%ACTION%"=="clippy" goto :clippy
if /I "%ACTION%"=="release" goto :release
if /I "%ACTION%"=="portable" goto :portable
if /I "%ACTION%"=="verify-portable" goto :verify_portable

echo [AIMeeting] Unknown command: %ACTION%
echo.
goto :help

:dev
echo [AIMeeting] Starting desktop dev app...
call npm.cmd run desktop:dev
exit /b %ERRORLEVEL%

:web
echo [AIMeeting] Starting frontend-only Vite dev server...
call npm.cmd run dev -- --host 127.0.0.1
exit /b %ERRORLEVEL%

:lint
echo [AIMeeting] Running TypeScript check...
call npm.cmd run lint
exit /b %ERRORLEVEL%

:build
echo [AIMeeting] Building frontend...
call npm.cmd run build
exit /b %ERRORLEVEL%

:test
echo [AIMeeting] Running frontend and Rust tests...
call npm.cmd run test
exit /b %ERRORLEVEL%

:check
echo [AIMeeting] Running full local check...
call npm.cmd run check
exit /b %ERRORLEVEL%

:format
echo [AIMeeting] Checking Rust formatting...
call npm.cmd run format:check
exit /b %ERRORLEVEL%

:clippy
echo [AIMeeting] Running Rust Clippy...
call npm.cmd run clippy
exit /b %ERRORLEVEL%

:release
echo [AIMeeting] Building Windows installers...
call npm.cmd run check
if errorlevel 1 exit /b %ERRORLEVEL%
call npm.cmd run desktop:build -- --bundles nsis,msi
exit /b %ERRORLEVEL%

:portable
echo [AIMeeting] Building and verifying Windows no-install package...
pwsh -NoProfile -File "%~dp0scripts\build-portable.ps1"
exit /b %ERRORLEVEL%

:verify_portable
echo [AIMeeting] Verifying latest Windows no-install package...
pwsh -NoProfile -File "%~dp0scripts\verify-portable.ps1"
exit /b %ERRORLEVEL%

:help
echo AIMeeting root entry script
echo.
echo Usage:
echo   aimeeting.cmd              Start Tauri desktop dev app
echo   aimeeting.cmd dev          Start Tauri desktop dev app
echo   aimeeting.cmd web          Start frontend-only Vite server
echo   aimeeting.cmd lint         Run TypeScript check
echo   aimeeting.cmd build        Build frontend
echo   aimeeting.cmd test         Run frontend and Rust tests
echo   aimeeting.cmd check        Run tests, types, formatting, and Clippy
echo   aimeeting.cmd format       Check Rust formatting
echo   aimeeting.cmd clippy       Run Rust Clippy with warnings denied
echo   aimeeting.cmd release      Build unsigned Windows installers
echo   aimeeting.cmd portable     Build and verify a Windows no-install ZIP
echo   aimeeting.cmd verify-portable  Verify the latest no-install ZIP
echo.
echo Notes:
echo   Use dev for normal local testing. The web command does not start Tauri backend commands.
exit /b 0
