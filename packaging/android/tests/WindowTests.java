package app.cinnabar.windowtests;

import android.app.AlertDialog;
import android.app.Instrumentation;
import android.content.Intent;
import android.graphics.Bitmap;
import java.io.File;
import java.io.FileOutputStream;
import android.os.Bundle;
import android.view.WindowInsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.function.BooleanSupplier;

/** Device regressions for navigation visibility, focus recovery and independent keyboard control. */
public final class WindowTests extends Instrumentation {
    private static final int RESULT_OK = -1;
    private WindowActivity activity;

    @Override public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        start();
    }

    @Override public void onStart() {
        Bundle results = new Bundle();
        try {
            Intent launch = new Intent(getTargetContext(), WindowActivity.class)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            activity = (WindowActivity) startActivitySync(launch);
            await(() -> activity.hasWindowFocus(), "window focus");
            await(() -> visible(WindowInsets.Type.navigationBars())
                    && hasObstruction(WindowInsets.Type.navigationBars()), "baseline navigation bar geometry");
            capture("window-before.png");
            runOnMainSync(() -> {
                activity.immersive = new ImmersiveMode(activity.getWindow());
                activity.immersive.restore();
            });
            await(() -> systemBarsHidden(), "initial immersive mode");
            capture("window-after.png");

            runOnMainSync(() -> {
                activity.editor.requestFocus();
                activity.getWindow().getInsetsController().show(WindowInsets.Type.ime());
            });
            await(() -> visible(WindowInsets.Type.ime()) && hasObstruction(WindowInsets.Type.ime()), "keyboard opens in immersive mode");
            runOnMainSync(() -> activity.immersive.restore());
            await(() -> visible(WindowInsets.Type.ime()) && hasObstruction(WindowInsets.Type.ime()), "bar restoration preserves the keyboard");
            capture("window-keyboard.png");
            runOnMainSync(() -> activity.getWindow().getInsetsController().hide(WindowInsets.Type.ime()));
            await(() -> !visible(WindowInsets.Type.ime())
                    && systemBarsHidden(), "keyboard dismissal restores immersive mode");

            AlertDialog[] dialog = new AlertDialog[1];
            runOnMainSync(() -> {
                dialog[0] = new AlertDialog.Builder(activity).setMessage("Window focus test").create();
                dialog[0].show();
            });
            await(() -> !activity.hasWindowFocus(), "dialog takes window focus");
            runOnMainSync(() -> dialog[0].dismiss());
            await(() -> activity.hasWindowFocus() && systemBarsHidden(),
                    "dialog dismissal restores immersive mode");
            results.putString("stream", "PASS: initial hiding, keyboard independence, dismissal and focus recovery\n");
            runOnMainSync(() -> activity.finish());
            finish(RESULT_OK, results);
        } catch (Throwable failure) {
            results.putString("stream", "FAIL: " + failure + "\n");
            if (activity != null) runOnMainSync(() -> activity.finish());
            finish(0, results);
        }
    }

    /** Captures only this isolated window fixture after Android finishes its animations. */
    private void capture(String name) throws Exception {
        getUiAutomation().waitForIdle(100, 3000);
        Bitmap image = getUiAutomation().takeScreenshot();
        if (image == null) throw new AssertionError("screen capture unavailable");
        try (FileOutputStream output = new FileOutputStream(new File(getTargetContext().getFilesDir(), name))) {
            if (!image.compress(Bitmap.CompressFormat.PNG, 100, output)) {
                throw new AssertionError("screen capture failed");
            }
        } finally {
            image.recycle();
        }
    }

    /** A hidden status bar alone must not make visible navigation pass this check. */
    private boolean systemBarsHidden() {
        return !visible(WindowInsets.Type.navigationBars())
                && !visible(WindowInsets.Type.statusBars())
                && !visible(WindowInsets.Type.captionBar());
    }

    /** Requires actual occupied screen area, including navigation on a landscape side edge. */
    private boolean hasObstruction(int type) {
        android.graphics.Insets insets = activity.getWindow().getDecorView().getRootWindowInsets().getInsets(type);
        return insets.left > 0 || insets.top > 0 || insets.right > 0 || insets.bottom > 0;
    }

    /** Reads current platform visibility on the UI thread. */
    private boolean visible(int type) {
        WindowInsets insets = activity.getWindow().getDecorView().getRootWindowInsets();
        return insets != null && insets.isVisible(type);
    }

    /** Waits for real layout events, with a bounded failure if Android never applies the change. */
    private void await(BooleanSupplier condition, String description) throws InterruptedException {
        CountDownLatch ready = new CountDownLatch(1);
        runOnMainSync(() -> {
            activity.insetsChanged = () -> { if (condition.getAsBoolean()) ready.countDown(); };
            activity.insetsChanged.run();
            activity.editor.requestApplyInsets();
        });
        if (!ready.await(10, TimeUnit.SECONDS)) throw new AssertionError(description);
        runOnMainSync(() -> activity.insetsChanged = () -> {});
    }
}
