@echo off
cd /d "%~dp0"
title VR MARCO PRO SKATER - Passo 2: instalar no Quest
echo VR MARCO PRO SKATER - passo 2: instalar no Quest
echo.
echo Antes: Quest ligado, cabo USB conectado e modo desenvolvedor ativado.
echo Se aparecer um aviso no oculos, aceite a depuracao USB.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Instalar.ps1"
if errorlevel 1 goto fail
echo.
pause
exit /b 0
:fail
echo.
echo Nao terminou. Leia a mensagem acima, corrija e rode este arquivo de novo.
pause
exit /b 1
