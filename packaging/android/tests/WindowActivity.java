package app.cinnabar.windowtests;

import android.app.Activity;
import android.os.Bundle;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.widget.EditText;

/** A native-window fixture that exercises the game's system-bar policy without game assets. */
public final class WindowActivity extends Activity {
    ImmersiveMode immersive;
    EditText editor;
    Runnable insetsChanged = () -> {};

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
        editor = new EditText(this);
        editor.setHint("Android window test");
        editor.setImeOptions(android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI);
        setContentView(editor);
        editor.getViewTreeObserver().addOnGlobalLayoutListener(() -> insetsChanged.run());
        editor.setOnApplyWindowInsetsListener((view, insets) -> {
            if (immersive != null) {
                immersive.keyboardVisibilityChanged(insets.isVisible(WindowInsets.Type.ime()));
            }
            insetsChanged.run();
            return view.onApplyWindowInsets(insets);
        });
    }

    @Override public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        insetsChanged.run();
    }

    @Override public void onResume() {
        super.onResume();
        if (immersive != null) immersive.restore();
    }
}
