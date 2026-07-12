# Repository agent instructions

## Bevy client screenshots on Windows

- Computer Use/WGC currently cannot capture the Cinnabar Bevy window reliably.
- For live visual verification, capture the desktop with the native Windows GDI `CopyFromScreen` method and write PNG files beneath `%TEMP%`.
- Inspect those temporary PNGs with the image-viewing tool. Do not rely on stale Computer Use observations or claim visual verification without inspecting a fresh capture.
- Keep Mojang assets and all screenshots out of git.
