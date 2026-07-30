# Privacy

AutoDeck doesn't collect, transmit, or store any of your data outside your own devices.

- **No cloud, no account.** The phone app and PC app talk to each other directly over your local WiFi network. There is no server in between and no internet connection is required for the app to function.
- **No analytics, no telemetry.** Nothing about your usage is sent anywhere.
- **What's stored, and where:**
  - The PC app stores your button assignments (`buttons.json`) and the list of devices you've approved (`paired_devices.json`) in its local app data folder on your PC. Neither ever leaves your computer.
  - The phone app stores a device ID and pairing token in its local app preferences, used only to reconnect to your PC without re-prompting for approval.
- **Network permissions:** the Android app requests local network and location permissions. Location access is required by Android to discover devices via WiFi (mDNS) — it is not used to determine or store your actual location.
- **Auto-update:** the PC app checks GitHub Releases for new versions. This is a direct request to GitHub to check for a newer file; no usage data is sent with it.

If you have questions, open an issue on the [GitHub repository](https://github.com/33bnm3-sudo/autodeck).
