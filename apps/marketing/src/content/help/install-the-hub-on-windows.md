---
title: Install the Hub on Windows
summary: Put the Hub on your counter PC, check the installer, and pair it with your café.
category: setup
reviewed: 2026-10-31
---

The Hub is the program on your counter PC that runs the floor, billing, the POS and reports. Use a PC that stays on while the café is open.

1. Open [Downloads](/downloads) and download **Windows Hub**.
2. Check the file. Copy the SHA-256 shown on the Downloads page, run the `Get-FileHash` line from "How to verify" in PowerShell, and make sure the two match letter for letter.
3. Run the installer. Windows 10 or later is needed; Windows 7 is not supported.
4. On the setup page, reach the pairing step and type its 8-character code into the Hub. The code works once.
5. When the Hub checks in, the setup page moves on by itself.

The Hub keeps running if your internet goes down. Kiosks carry on their sessions, and the Hub catches up when the link returns.

Next: [pair a kiosk](/help/pair-a-kiosk).
