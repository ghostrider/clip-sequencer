@echo off
rem Starts the sequencer (debug build) with the MIDI clock from "Default App Loopback (B)".
rem Build first with: cargo build -p seq-app   (and the UI: cd web && npm run build)
rem The UI is then at http://127.0.0.1:8090

set "EXE=%~dp0target\debug\seq-app.exe"
if not exist "%EXE%" (
    echo Debug build not found: %EXE%
    echo Run "cargo build -p seq-app" in %~dp0 first.
    pause
    exit /b 1
)

"%EXE%" --midi-port "Default App Loopback (B)" %*
if errorlevel 1 pause
