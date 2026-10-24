@echo off
cd /d "%~dp0"
echo Instalando o APK e enviando os mapas para o Quest...
echo O som do skatista ja vai dentro do APK.
echo O oculos precisa estar ligado, no cabo USB, com a depuracao aceita.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Enviar-Quest.ps1"
if errorlevel 1 goto fail
echo.
pause
exit /b 0
:fail
echo.
pause
exit /b 1
