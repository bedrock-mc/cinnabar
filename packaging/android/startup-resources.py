"""Build the Android starting theme from the same specification as both live overlays."""
import json
from pathlib import Path
import sys

specification = json.loads(Path(sys.argv[1]).read_text())
output = Path(sys.argv[2])
r, g, b, a = specification["background"]
color = f"#{a:02x}{r:02x}{g:02x}{b:02x}"
(output / "values" / "startup.xml").write_text(f'''<resources>
    <color name="startup_background">{color}</color>
    <style name="StartupTheme" parent="android:style/Theme.Material.NoActionBar.Fullscreen">
        <item name="android:windowBackground">@drawable/startup_background</item>
        <item name="android:windowDisablePreview">false</item>
    </style>
</resources>
''')
(output / "values-v31" / "startup.xml").write_text('''<resources>
    <style name="StartupTheme" parent="android:style/Theme.Material.NoActionBar.Fullscreen">
        <item name="android:windowBackground">@drawable/startup_background</item>
        <item name="android:windowSplashScreenBackground">@color/startup_background</item>
        <item name="android:windowSplashScreenAnimatedIcon">@mipmap/ic_launcher</item>
        <item name="android:windowDisablePreview">false</item>
    </style>
</resources>
''')
(output / "drawable" / "startup_background.xml").write_text('''<layer-list xmlns:android="http://schemas.android.com/apk/res/android">
    <item android:drawable="@color/startup_background" />
    <item><bitmap android:src="@mipmap/ic_launcher" android:gravity="center" /></item>
</layer-list>
''')
