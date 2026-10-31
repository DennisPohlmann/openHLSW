# Accessibility

openHLSW should be usable by as many people as possible. This file says what
works today, what does not yet, and how to report a barrier.

## What works

- **Keyboard**: menus, toolbar and shortcuts follow HLSW 1.4 (for example
  Ctrl+F / F3 to find a server); Tab moves between input fields, the console
  has a history and Tab completion, and typing in an open dropdown jumps to the
  matching entry.
- **Text size and font**: text size 10–16 px and any installed font can be set
  in the settings.
- **Contrast**: two skins, White and Black, so you can pick the one that is
  easier to read for you.
- **Status is not shown by color alone**: offline servers turn red and also
  move to the end of the list; ping values are shown as numbers next to the
  colored graph.
- **Languages**: 18 languages, the system language by default.

## Known limitations

- **Screen readers**: openHLSW is built with egui, which draws its own
  controls. The accessibility layer (AccessKit) is not enabled yet, so screen
  readers such as NVDA, Narrator, Orca or VoiceOver cannot read the window.
- System settings for high contrast, larger text or reduced motion are not
  picked up automatically.
- Some actions are only in right-click menus.

## Reporting a barrier

Please open an
[issue](https://github.com/DennisPohlmann/openHLSW/issues/new/choose) with the
**Bug report** form and describe what you tried, which assistive technology you
use (if any) and what got in your way. Suggestions that make openHLSW easier to
use are welcome as a **Feature request**.
