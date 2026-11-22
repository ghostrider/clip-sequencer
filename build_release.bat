@echo off
rem Builds Clip Sequencer for Resolume: double-click this file.
rem Result: release\seq-app.exe, release\seq-app-cli.exe and release\clip-sequencer-<version>-windows-x64.zip
rem
rem This file installs nothing. It checks for the tools below, says which are missing and how to get them, then
rem runs scripts\release.ps1. Needed:
rem   - Rust (cargo), stable:            https://rustup.rs
rem   - Node.js 20 or newer (node, npm): https://nodejs.org
rem   - cargo-about (third-party notices): cargo install --locked cargo-about --features cli
rem   - Visual Studio 2022 or its Build Tools, workload "Desktop development with C++" (MSVC, MSBuild; FFGL plugin):
rem                                      https://visualstudio.microsoft.com/downloads/
rem   - Git, and the FFGL SDK submodule:  https://git-scm.com , then in this folder: git submodule update --init
rem npm downloads the UI's own packages (web\package-lock.json) during the build; that needs the internet once.

setlocal
cd /d "%~dp0"
set MISSING=0

where cargo >nul 2>nul
if errorlevel 1 (
  echo [missing] Rust ^(cargo^): install from https://rustup.rs
  set MISSING=1
)
where node >nul 2>nul
if errorlevel 1 (
  echo [missing] Node.js ^(node^): install from https://nodejs.org
  set MISSING=1
)
where npm >nul 2>nul
if errorlevel 1 (
  echo [missing] npm ^(comes with Node.js^): install from https://nodejs.org
  set MISSING=1
)
where cargo-about >nul 2>nul
if errorlevel 1 (
  echo [missing] cargo-about: run   cargo install --locked cargo-about --features cli
  set MISSING=1
)
where git >nul 2>nul
if errorlevel 1 (
  echo [missing] Git: install from https://git-scm.com
  set MISSING=1
)
if not exist "external\ffgl\source\lib\ffgl" (
  echo [missing] FFGL SDK in external\ffgl: run   git submodule update --init
  set MISSING=1
)
set VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe
set MSBUILD=
if exist "%VSWHERE%" (
  for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -find MSBuild\**\Bin\MSBuild.exe`) do set MSBUILD=%%i
)
if not defined MSBUILD (
  echo [missing] Visual Studio 2022 or Build Tools with "Desktop development with C++":
  echo           https://visualstudio.microsoft.com/downloads/
  set MISSING=1
)

if "%MISSING%"=="1" (
  echo.
  echo Install the missing tools above, then double-click build_release.bat again.
  pause
  exit /b 1
)

echo All tools found. Building, this takes a few minutes...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\release.ps1"
if errorlevel 1 (
  echo.
  echo BUILD FAILED. See the messages above.
  pause
  exit /b 1
)
echo.
echo Done. The app is in the release folder:
dir /b release\*.exe release\*.zip
pause
