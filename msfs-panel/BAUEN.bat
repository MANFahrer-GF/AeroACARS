@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0"
echo === AeroACARS Panel v0.13.1 bauen und installieren ===

rem 1) fspackagetool suchen
set "FPT="
for %%P in (
 "C:\MSFS 2024 SDK\Tools\bin\fspackagetool.exe"
 "C:\MSFS SDK\Tools\bin\fspackagetool.exe"
 "%ProgramFiles%\MSFS 2024 SDK\Tools\bin\fspackagetool.exe"
 "D:\MSFS 2024 SDK\Tools\bin\fspackagetool.exe"
) do if exist %%P set "FPT=%%~P"
if not defined FPT (
  where fspackagetool.exe >nul 2>nul && for /f "delims=" %%i in ('where fspackagetool.exe') do set "FPT=%%i"
)
if not defined FPT (
  echo fspackagetool.exe nicht gefunden.
  set /p FPT=Voller Pfad zu fspackagetool.exe: 
)

rem 2) bauen (PROJEKT-Datei, nicht die Paketdefinition)
if exist "%~dp0Build\Packages" rmdir /s /q "%~dp0Build\Packages"
if exist "%~dp0Build\_PackageInt" rmdir /s /q "%~dp0Build\_PackageInt"
"%FPT%" -nopause "%~dp0Build\aeroacars-panel-project.xml"

rem 3) Ergebnis pruefen
set "OUT=%~dp0Build\Packages\aeroacars-panel"
if not exist "%OUT%\layout.json" (
  echo Gebautes Paket nicht gefunden unter %OUT%
  echo Bitte das Fenster oben fotografieren.
  pause & exit /b 1
)
echo Gebaut: %OUT%

rem 4) Community-Ordner finden: ZUERST aus der Sim-Konfiguration (UserCfg.opt),
rem    dort steht, wo der Sim wirklich liest. Erst danach raten.
set "COMM="
rem Thomas' Sim liest hier (Stand 03.10.2026): E:\MSFS24_Community\Community
if exist "E:\MSFS24_Community\Community" set "COMM=E:\MSFS24_Community\Community"
if not defined COMM for %%F in (
 "%LOCALAPPDATA%\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalState\UserCfg.opt"
 "%APPDATA%\Microsoft Flight Simulator 2024\UserCfg.opt"
) do if exist %%F (
  for /f "tokens=1,* delims= " %%a in ('findstr /b /c:"InstalledPackagesPath" %%F') do (
    set "RAW=%%b"
    set "RAW=!RAW:"=!"
    if exist "!RAW!\Community" set "COMM=!RAW!\Community"
  )
)
if not defined COMM (
  for %%C in (
   "%LOCALAPPDATA%\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache\Packages\Community"
   "%APPDATA%\Microsoft Flight Simulator 2024\Packages\Community"
  ) do if exist %%C set "COMM=%%~C"
)
if not defined COMM (
  echo Community-Ordner nicht automatisch gefunden.
  set /p COMM=Voller Pfad zum Community-Ordner: 
)
echo Community-Ordner des Sims: %COMM%

rem 4b) Alte AeroACARS-Panel-Pakete aus dem Weg (sie belegen dieselbe Panel-Kennung)
if not exist "%COMM%\..\alte-panels" mkdir "%COMM%\..\alte-panels"
for /d %%D in ("%COMM%\aeroacars-msfs-panel" "%COMM%\aeroacars-flight-review" "%COMM%\aeroacars-ingamepanel") do (
  if exist %%D (
    echo Altes Paket verschoben: %%D
    move %%D "%COMM%\..\alte-panels\" >nul
  )
)

rem 5) alte Version raus, neue rein
if exist "%COMM%\aeroacars-panel" rmdir /s /q "%COMM%\aeroacars-panel"
robocopy "%OUT%" "%COMM%\aeroacars-panel" /E /NFL /NDL /NJH /NJS >nul
echo Installiert nach: %COMM%\aeroacars-panel
echo Jetzt MSFS neu starten, AeroACARS-Desktop-App laufen lassen.
pause
