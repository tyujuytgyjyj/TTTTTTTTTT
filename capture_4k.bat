@echo off
title APEX.drive - Automated 4K Cinematic Studio Capture Tool
echo ============================================================================
echo Running APEX.drive Automated 4K Cinematic Capture Tool (--capture-4k)...
echo Generating 4K Golden-Hour Studio Shots for Main Menu, Loading Screen & Cards
echo ============================================================================
cargo run --release -p crashdrive -- --capture-4k --msaa
echo Done! All 4K screenshots saved in screenshots\shot_01_4k.png .. shot_06_4k.png
pause
