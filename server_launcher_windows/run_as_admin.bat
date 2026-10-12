@echo off
:: ============================================================================
:: Server Launcher Windows - Self-Elevating Administrator Launcher
:: ============================================================================
setlocal EnableDelayedExpansion

:: Check for Administrator permissions
net session >nul 2>&1
if %errorLevel% == 0 (
    goto :run_app
) else (
    echo Elevating permissions... Requesting Administrator privileges (UAC)...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

:run_app
cd /d "%~dp0"
echo Starting Server Launcher (Windows Optimized) with Administrator Privileges...
if exist "python.exe" (
    python.exe server_launcher_windows.py
) else (
    py -3 server_launcher_windows.py 2>nul || python server_launcher_windows.py
)
if %errorLevel% neq 0 (
    echo.
    echo Application exited with code %errorLevel%.
    pause
)
