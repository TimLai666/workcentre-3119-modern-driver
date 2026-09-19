@echo off
rem WorkCentre 3119 scanner driver: one-click install (trust certificate,
rem install or update the package, restart the WIA service, verify).
rem Asks for administrator rights (UAC) and keeps the window open at the end.
setlocal
set "HERE=%~dp0"
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)
title WorkCentre 3119 scanner driver - install
powershell -NoProfile -ExecutionPolicy Bypass -File "%HERE%wc3119-setup.ps1" -Action Install -TrustCertificate -Apply
set "CODE=%errorlevel%"
echo.
if "%CODE%"=="0" (
    echo Done. The scanner is ready for Windows Scan, Windows Fax and Scan and other WIA applications.
) else if "%CODE%"=="3010" (
    echo Windows asks for a reboot before the driver is in use. Reboot, then run install.cmd again to verify.
) else (
    echo Install failed with code %CODE%. Logs: %ProgramData%\WorkCentre3119Driver\setup-logs
)
echo.
pause
exit /b %CODE%
