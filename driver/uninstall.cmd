@echo off
rem WorkCentre 3119 scanner driver: one-click uninstall (remove the package,
rem the COM registration and the test certificate trust).
rem Asks for administrator rights (UAC) and keeps the window open at the end.
setlocal
set "HERE=%~dp0"
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)
title WorkCentre 3119 scanner driver - uninstall
powershell -NoProfile -ExecutionPolicy Bypass -File "%HERE%wc3119-setup.ps1" -Action Uninstall -UntrustCertificate -Apply
set "CODE=%errorlevel%"
echo.
if "%CODE%"=="0" (
    echo Done. The driver and the certificate trust were removed.
) else (
    echo Uninstall failed with code %CODE%. Logs: %ProgramData%\WorkCentre3119Driver\setup-logs
)
echo.
pause
exit /b %CODE%
