@echo off
REM Centura Capture launcher (dev mode). Double-click to start the app with logging.
setlocal
cd /d "%~dp0"

REM Only one instance can run at a time (it locks target\debug\meetily.exe).
netstat -ano | findstr ":3118 " | findstr LISTENING >nul
if %errorlevel%==0 (
  echo.
  echo Centura Capture is already running ^(port 3118 is in use^).
  echo Close that app window first, then run this again.
  echo.
  pause
  exit /b 1
)

if not exist "logs" mkdir "logs"
for /f %%i in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd_HHmmss"') do set "STAMP=%%i"
set "LOG=%~dp0logs\centura_%STAMP%.log"

REM whisper-rs bindgen needs libclang from LLVM 18 (LLVM 22 breaks the build).
set "LIBCLANG_PATH=C:\Program Files\LLVM\bin"

echo Starting Centura Capture (CPU build).
echo The app window opens automatically once the build finishes (first run is slow).
echo Logging to: %LOG%
echo.

cd /d "%~dp0frontend"
call pnpm run tauri:dev > "%LOG%" 2>&1
pause
