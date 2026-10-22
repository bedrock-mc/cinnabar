package app.cinnabar.windowtests;

import android.app.Activity;
import android.os.Bundle;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.view.WindowInsetsAnimation;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import android.widget.EditText;

/** A native-window fixture that exercises the game's system-bar policy without game assets. */
public final class WindowActivity extends Activity {
    ImmersiveMode immersive;
    EditText editor;
    final Set<WindowInsetsAnimation> animations = new HashSet<>();
    Runnable insetsChanged = () -> {};

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
        editor = new EditText(this);
        editor.setHint("Android window test");
        editor.setImeOptions(android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI);
        setContentView(editor);
        editor.getViewTreeObserver().addOnGlobalLayoutListener(() -> insetsChanged.run());
        editor.setWindowInsetsAnimationCallback(new WindowInsetsAnimation.Callback(
                WindowInsetsAnimation.Callback.DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
            @Override public void onPrepare(WindowInsetsAnimation animation) {
                animations.add(animation);
            }

            @Override public WindowInsets onProgress(WindowInsets insets,
                    List<WindowInsetsAnimation> running) {
                return insets;
            }

            @Override public void onEnd(WindowInsetsAnimation animation) {
                animations.remove(animation);
                insetsChanged.run();
            }
        });
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
