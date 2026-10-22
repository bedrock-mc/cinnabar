package @APPLICATION_ID@;

import android.app.NativeActivity;
import android.content.Intent;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;

/** Hosts the client, whose JSON-UI engine draws every game screen. */
public final class CinnabarActivity extends NativeActivity {
    private volatile int bottomObstruction;
    private StartupScreen startup;
    private volatile boolean startupActive;
    private volatile long startupGeneration;
    private ImmersiveMode immersiveMode;

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        FilePicker.attach(this);
        startup = new StartupScreen(this);
        addContentView(startup, new android.view.ViewGroup.LayoutParams(
                android.view.ViewGroup.LayoutParams.MATCH_PARENT, android.view.ViewGroup.LayoutParams.MATCH_PARENT));
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
        immersiveMode = new ImmersiveMode(getWindow());
        final View decor = getWindow().getDecorView();
        decor.setOnApplyWindowInsetsListener((view, insets) -> {
            updateKeyboardGeometry(decor, insets);
            return view.onApplyWindowInsets(insets);
        });
        decor.getViewTreeObserver().addOnGlobalLayoutListener(() ->
                updateKeyboardGeometry(decor, decor.getRootWindowInsets()));
        decor.requestApplyInsets();
    }

    @Override protected void onResume() {
        startupGeneration++;
        startupActive = true;
        super.onResume();
        immersiveMode.restore();
    }

    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (!FilePicker.onResult(requestCode, resultCode, data)) {
            super.onActivityResult(requestCode, resultCode, data);
        }
    }

    @Override protected void onPause() {
        startupActive = false;
        startupGeneration++;
        super.onPause();
    }

    /** Native startup reports work on its own thread; only the activity thread owns views. */
    public void startupProgress(String message, long generation) {
        final boolean active = startupActive;
        runOnUiThread(() -> {
            if (active && startupActive && generation == startupGeneration
                    && !isFinishing() && !isDestroyed() && startup != null) startup.update(message);
        });
    }

    /** Lets native rendering invalidate readiness even when a pause preserves its surface. */
    public long startupGeneration() { return startupGeneration; }

    /** Publishes the keyboard overlap while leaving the Vulkan surface at full size. */
    private void updateKeyboardGeometry(View decor, WindowInsets insets) {
        if (insets == null) return;
        if (Build.VERSION.SDK_INT >= 30) {
            bottomObstruction = insets.getInsets(
                    WindowInsets.Type.ime() | WindowInsets.Type.systemBars()).bottom;
            immersiveMode.keyboardVisibilityChanged(insets.isVisible(WindowInsets.Type.ime()));
        } else {
            Rect visible = new Rect();
            int[] location = new int[2];
            decor.getWindowVisibleDisplayFrame(visible);
            decor.getLocationOnScreen(location);
            bottomObstruction = Math.max(0, location[1] + decor.getHeight() - visible.bottom);
            immersiveMode.keyboardVisibilityChanged(bottomObstruction > insets.getStableInsetBottom());
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
