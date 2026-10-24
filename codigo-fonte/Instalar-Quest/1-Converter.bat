@echo off
cd /d "%~dp0"
echo Preparando o conversor...
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Baixar.ps1"
if errorlevel 1 goto fail
echo.
echo O ISO nao sera pedido de novo.
echo A pasta extraida ao lado do jogo e que vale.
echo Deixe esta janela aberta ate o mapa abrir.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Converter.ps1"
if errorlevel 1 goto fail
echo.
echo Pronto. Conecte o Quest e rode 2-Enviar-Quest.bat
pause
exit /b 0
:fail
echo.
echo Nao terminou. Feche qualquer janela de erro do setup e rode este arquivo de novo.
pause
exit /b 1
