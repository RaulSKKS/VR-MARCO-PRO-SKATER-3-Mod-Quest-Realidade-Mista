@echo off
cd /d "%~dp0"
title Skate 3 AR - Marco VR - Instalar tudo
echo Skate 3 AR (Marco VR) - instalar tudo de uma vez
echo.
echo 1. Prepara o jogo a partir da sua ISO (pasta COLOQUE-SUA-ISO-AQUI).
echo 2. Instala no Quest o jogo com tudo do Marco e os dados da sua ISO.
echo.
echo Antes: ISO na pasta COLOQUE-SUA-ISO-AQUI, internet, e o Quest no cabo USB
echo com o modo desenvolvedor ativado.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Preparar.ps1"
if errorlevel 1 goto fail
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Instalar.ps1"
if errorlevel 1 goto fail
echo.
pause
exit /b 0
:fail
echo.
echo Nao terminou. Leia a mensagem acima, corrija e rode este arquivo de novo.
echo O que ja foi feito nao se perde: ele continua de onde parou.
pause
exit /b 1
