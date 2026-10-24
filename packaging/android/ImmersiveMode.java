package @APPLICATION_ID@;

import android.os.Build;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;

/** Keeps system navigation out of the game while retaining edge-swipe access. */
final class ImmersiveMode {
    private final Window window;
    private boolean keyboardVisible;

    ImmersiveMode(Window window) {
        this.window = window;
        window.getDecorView().getViewTreeObserver().addOnWindowFocusChangeListener(focused -> {
            if (focused) restore();
        });
    }

    /** Restores immersive mode after the activity resumes or regains its window. */
    void restore() {
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController controller = window.getInsetsController();
            if (controller != null) {
                controller.setSystemBarsBehavior(
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                controller.hide(WindowInsets.Type.systemBars());
            }
        } else {
            window.getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
        }
    }

    /** Keyboard dismissal can leave navigation visible without changing window focus. */
    void keyboardVisibilityChanged(boolean visible) {
        boolean dismissed = keyboardVisible && !visible;
        keyboardVisible = visible;
        if (dismissed && window.getDecorView().hasWindowFocus()) restore();
    }
}
