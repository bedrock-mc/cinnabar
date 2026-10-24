The window fixture runs the production immersive-mode policy in an Android activity,
without downloading game assets or changing the installed client. It verifies initial
navigation hiding, keyboard visibility, keyboard dismissal and focus recovery after a dialog.
It needs Android 11 or newer and a configured onscreen keyboard. Enable the virtual keyboard
when testing an emulator that also has a hardware keyboard.

```sh
python3 packaging/android/tests/run-window-tests.py --sdk "$ANDROID_HOME" \
  --serial emulator-5554 --out /tmp/cinnabar-window-tests
```

This checks Android window behavior; it does not replace client gameplay or phone acceptance.
It saves before/after and keyboard screenshots of the isolated fixture in `--out`.
The runner temporarily suppresses the first-use immersive-mode tutorial and restores
that preference afterward, so the tutorial cannot steal focus during keyboard checks.
The fixture uses its own package, `app.cinnabar.windowtests`.
