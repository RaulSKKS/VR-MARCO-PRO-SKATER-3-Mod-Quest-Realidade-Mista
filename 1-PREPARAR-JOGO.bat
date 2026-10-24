@echo off
cd /d "%~dp0"
title Skate 3 AR - Marco VR - Passo 1: preparar o jogo
echo Skate 3 AR (Marco VR) - passo 1: preparar o jogo a partir da sua ISO
echo.
echo Antes: coloque a ISO do Skate 3 (Xbox 360) na pasta COLOQUE-SUA-ISO-AQUI.
echo Precisa de internet na primeira vez (baixa o conversor e o extrator).
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Preparar.ps1"
if errorlevel 1 goto fail
echo.
pause
exit /b 0
:fail
echo.
echo Nao terminou. Leia a mensagem acima, corrija e rode este arquivo de novo.
pause
exit /b 1
