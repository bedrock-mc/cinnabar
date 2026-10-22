package @APPLICATION_ID@;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.ProgressDialog;
import android.content.Intent;
import android.content.res.AssetManager;
import android.os.Bundle;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

/** Unpacks the prep kit and prepares game assets behind Android dialogs, then starts the game. */
public final class BootstrapActivity extends Activity {
    static { System.loadLibrary("@CLIENT_LIBRARY@"); }

    private static final String RESOURCES = "resources";

    /** Numbers setup attempts across activity instances, so a cancellation names its own. */
    private static final AtomicLong ATTEMPTS = new AtomicLong();
    /** One attempt at a time unpacks and prepares, across activity instances. */
    private static final Object SETUP = new Object();

    /** Null when the game can start, empty when the user quit, otherwise the failure. */
    private native String prepare(String filesDir, String nativeLibraryDir, long attempt);
    private native void cancel(long attempt);

    /** Freezes the previous session before Retry or game startup can rotate its logs. */
    private native String captureCrashReports(String files, String libraries);

    /** Runs only on a healthy worker; timestamp zero also completes prior saved reports. */
    native String crashReport(String files, String libraries, long timestamp, int reason,
            int status, String description, byte[] tombstone, boolean send);

    private volatile long attempt;
    private ProgressDialog progress;
    private volatile CountDownLatch consentAnswer;

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        setContentView(new StartupScreen(this));
        new Thread(() -> {
            String saved = captureCrashReports(getFilesDir().getAbsolutePath(),
                    getApplicationInfo().nativeLibraryDir);
            runOnUiThread(() -> {
                if (isFinishing() || isDestroyed()) return;
                if (!StartupDiagnostics.showPreviousFailure(this, this::start, saved)) start();
            });
        }, "crash-capture").start();
    }

    private void start() {
        showProgress("Preparing @PRODUCT_NAME@");
        long current = ATTEMPTS.incrementAndGet();
        attempt = current;
        new Thread(() -> setup(current), "setup").start();
    }

    private void setup(long current) {
        String result;
        synchronized (SETUP) {
            try {
                if (isFinishing() || isDestroyed()) {
                    result = "";
                } else {
                    unpackResources();
                    result = prepare(getFilesDir().getAbsolutePath(),
                            getApplicationInfo().nativeLibraryDir, current);
                }
            } catch (Exception error) {
                result = error.toString();
            }
        }
        String outcome = result;
        runOnUiThread(() -> finishSetup(outcome));
    }

    /** Replaces the unpacked kit whenever a different APK is installed. */
    private void unpackResources() throws Exception {
        String installed = Long.toString(
                getPackageManager().getPackageInfo(getPackageName(), 0).lastUpdateTime);
        File root = new File(getFilesDir(), RESOURCES);
        File stamp = new File(getFilesDir(), RESOURCES + ".installed");
        if (root.isDirectory() && stamp.isFile()
                && installed.equals(new String(Files.readAllBytes(stamp.toPath()), StandardCharsets.UTF_8))) {
            return;
        }
        delete(root);
        copy(getAssets(), RESOURCES, root);
        Files.write(stamp.toPath(), installed.getBytes(StandardCharsets.UTF_8));
    }

    private static void copy(AssetManager assets, String path, File target) throws IOException {
        String[] children = assets.list(path);
        if (children == null || children.length == 0) {
            try (InputStream source = assets.open(path); OutputStream sink = new FileOutputStream(target)) {
                byte[] buffer = new byte[65536];
                for (int count; (count = source.read(buffer)) != -1; ) sink.write(buffer, 0, count);
            }
            return;
        }
        if (!target.isDirectory() && !target.mkdirs()) throw new IOException("cannot create " + target);
        for (String child : children) copy(assets, path + "/" + child, new File(target, child));
    }

    private static void delete(File file) {
        File[] children = file.listFiles();
        if (children != null) for (File child : children) delete(child);
        file.delete();
    }

    /** Called by the native setup on its worker thread; blocks until the user answers. */
    public boolean requestConsent(String title, String body) throws InterruptedException {
        CountDownLatch answer = new CountDownLatch(1);
        AtomicBoolean accepted = new AtomicBoolean();
        consentAnswer = answer;
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed()) { answer.countDown(); return; }
            if (progress != null) progress.dismiss();
            new AlertDialog.Builder(this).setTitle(title).setMessage(body)
                    .setPositiveButton("Accept and download", (dialog, which) -> {
                        accepted.set(true);
                        answer.countDown();
                    })
                    .setNegativeButton("Cancel", (dialog, which) -> answer.countDown())
                    .setOnCancelListener(dialog -> answer.countDown())
                    .show();
        });
        answer.await();
        return accepted.get();
    }

    /** Called by the native setup on its worker thread. */
    public void showProgress(String message) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed()) return;
            if (progress == null || !progress.isShowing()) {
                progress = new ProgressDialog(this);
                progress.setTitle("@PRODUCT_NAME@");
                progress.setIndeterminate(true);
                progress.setOnCancelListener(dialog -> finish());
                progress.show();
            }
            progress.setMessage(message);
        });
    }

    private void finishSetup(String error) {
        if (isFinishing() || isDestroyed()) return;
        if (progress != null) progress.dismiss();
        if (error == null) {
            startActivity(new Intent(this, CinnabarActivity.class));
            finish();
        } else if (error.isEmpty()) {
            finish();
        } else {
            new AlertDialog.Builder(this).setTitle("@PRODUCT_NAME@ setup failed").setMessage(error)
                    .setPositiveButton("Retry", (dialog, which) -> start())
                    .setNegativeButton("Close", (dialog, which) -> finish())
                    .setOnCancelListener(dialog -> finish())
                    .show();
        }
    }

    @Override protected void onDestroy() {
        cancel(attempt);
        CountDownLatch answer = consentAnswer;
        if (answer != null) answer.countDown();
        if (progress != null) progress.dismiss();
        super.onDestroy();
    }
}
