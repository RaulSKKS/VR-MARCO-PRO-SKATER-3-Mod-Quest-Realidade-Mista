@echo off
cd /d "%~dp0"
echo Baixando o APK, se ainda faltar...
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Baixar.ps1"
if errorlevel 1 goto fail
echo.
echo Instalando pelo cabo. Para usar o SideQuest, arraste o arquivo da pasta apk.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Instalar-Apk.ps1"
if errorlevel 1 goto fail
echo.
pause
exit /b 0
:fail
echo.
pause
exit /b 1
