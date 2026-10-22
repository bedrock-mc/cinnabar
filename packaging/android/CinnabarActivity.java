package @APPLICATION_ID@;

import android.app.NativeActivity;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;

/** Hosts the client, whose JSON-UI engine draws every game screen. */
public final class CinnabarActivity extends NativeActivity {
    private volatile int bottomObstruction;

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
        final View decor = getWindow().getDecorView();
        decor.setOnApplyWindowInsetsListener((view, insets) -> {
            updateKeyboardGeometry(decor, insets);
            return view.onApplyWindowInsets(insets);
        });
        decor.getViewTreeObserver().addOnGlobalLayoutListener(() ->
                updateKeyboardGeometry(decor, decor.getRootWindowInsets()));
        decor.requestApplyInsets();
    }

    /** Publishes the keyboard overlap while leaving the Vulkan surface at full size. */
    private void updateKeyboardGeometry(View decor, WindowInsets insets) {
        if (insets == null) return;
        if (Build.VERSION.SDK_INT >= 30) {
            bottomObstruction = insets.getInsets(
                    WindowInsets.Type.ime() | WindowInsets.Type.systemBars()).bottom;
        } else {
            Rect visible = new Rect();
            int[] location = new int[2];
            decor.getWindowVisibleDisplayFrame(visible);
            decor.getLocationOnScreen(location);
            bottomObstruction = Math.max(0, location[1] + decor.getHeight() - visible.bottom);
        }
    }

    /** Returns bottom system-window obstruction relative to the full native surface. */
    public int bottomObstructionPixels() {
        return bottomObstruction;
    }

    @Override protected void onDestroy() {
        // The native loop never returns on destroy, which NativeActivity's teardown waits for,
        // and Bevy cannot start twice in one process anyway.
        android.os.Process.killProcess(android.os.Process.myPid());
        super.onDestroy();
    }
}
