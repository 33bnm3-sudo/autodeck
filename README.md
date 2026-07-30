# AutoDeck

Turn your phone into a wireless remote for your PC. A radial dial on your phone screen launches apps, opens folders, and controls volume on your computer — all over your local WiFi, no cloud, no account.

## What it does

- Drag any app, folder, or file onto the PC dial to assign it to a button
- Press that button from your phone to launch it on the PC
- Drag the dial to adjust Windows volume in real time
- Works entirely over your local network — nothing leaves your WiFi

## Download

Grab the latest build from the [Releases](https://github.com/33bnm3-sudo/autodeck/releases) page:

- **Windows PC**: `AutoDeck-x64.exe` (most PCs) or `AutoDeck-arm64.exe` (ARM64 PCs, e.g. Snapdragon-based) — no installer, just download and double-click to run. An installer (`setup.exe`/`.msi`) is also provided if you'd rather have a Start Menu entry and uninstaller.
- **Android**: `autodeck.apk` — you'll need to allow "install from unknown sources" since this isn't on the Play Store

The exe isn't code-signed yet, so Windows may show an "Unknown publisher" SmartScreen warning on first run — click "More info → Run anyway." (This applies to both the portable exe and the installer; it's about code signing, not how it's packaged.)

## Setup

1. Run the PC app (`AutoDeck.exe`) — it sits in your system tray. The first time it starts listening for your phone, Windows will ask to allow it through the firewall — click **Allow access**, that's the normal OS prompt for any app that accepts local network connections.
2. Install and open the Android app on your phone, connected to the **same WiFi** as your PC.
3. The phone auto-discovers the PC. The first time it connects, a popup appears on the PC asking you to **Allow** or **Deny** the new device — click Allow.
4. That's it — future reconnects won't ask again. On the PC, drag apps/folders onto the dial buttons to set them up.

A phone only ever connects to one PC. If you want to switch which PC it talks to, unpair it from the PC's Settings screen and reconnect on the same network as the new PC.

## Building from source

See [`PROTOCOL.md`](PROTOCOL.md) for the phone↔PC wire protocol, and [`STATUS.md`](STATUS.md) for current known issues and what's left to do.

- **PC agent** (`agent/`): Tauri 2 + SvelteKit (Svelte 5) + Rust. `npm install` then `npm run tauri dev` to run, `npm run tauri build` for a release installer.
- **Phone app** (`phone/`): Kotlin, native Android. `./gradlew assembleDebug` (needs `JAVA_HOME` pointed at a JDK, e.g. Android Studio's bundled `jbr`).

## Privacy

No cloud, no account, no telemetry — see [PRIVACY.md](PRIVACY.md).

## License

MIT — see [LICENSE](LICENSE).
