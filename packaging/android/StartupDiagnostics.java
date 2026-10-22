package @APPLICATION_ID@;

import android.app.Activity;
import android.app.ActivityManager;
import android.app.AlertDialog;
import android.app.ApplicationExitInfo;
import android.os.Build;
import android.widget.Toast;
import java.io.File;
import java.io.IOException;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.util.List;

/** Makes an unexpected exit visible on the next launch without needing USB debugging. */
final class StartupDiagnostics {
    private static final Object SEND_LOCK = new Object();
    private static final int LOG_TAIL_BYTES = 32768;
    private static final String REPORTED_SAVED = "reported-saved-report";
    private static final String SHARE_NOTICE = "Send report shares a redacted error and recent log lines with the @PRODUCT_NAME@ team. Reports stay on this device unless you consent.";

    /** Offers one relaunch dialog and starts healthy retries after the prior log was captured. */
    static boolean showPreviousFailure(BootstrapActivity activity, Runnable retry, String saved) {
        if (Build.VERSION.SDK_INT >= 30 && showHistoricalFailure(activity, retry)) return true;
        sendSaved(activity, null, false);
        if (saved == null || saved.equals(activity.getPreferences(0).getString(REPORTED_SAVED, null))) return false;
        activity.getPreferences(0).edit().putString(REPORTED_SAVED, saved).apply();
        AlertDialog dialog = new AlertDialog.Builder(activity)
                .setTitle("@PRODUCT_NAME@ stopped unexpectedly")
                .setMessage("A previous launch saved a crash report on this device.\n\n" + SHARE_NOTICE)
                .setPositiveButton("Retry", (which, button) -> retry.run())
                .setNegativeButton("Close", (which, button) -> activity.finish())
                .setNeutralButton("Send report", null)
                .setOnCancelListener(which -> activity.finish()).create();
        dialog.setOnShowListener(which -> dialog.getButton(AlertDialog.BUTTON_NEUTRAL)
                .setOnClickListener(view -> sendSaved(activity, dialog, true)));
        dialog.show();
        return true;
    }

    /** Offers OS exit details on API 30+, with the frozen prior log retained for its worker. */
    private static boolean showHistoricalFailure(BootstrapActivity activity, Runnable retry) {
        try {
            ActivityManager manager = activity.getSystemService(ActivityManager.class);
            List<ApplicationExitInfo> exits = manager.getHistoricalProcessExitReasons(
                    activity.getPackageName(), 0, 1);
            if (exits.isEmpty()) return false;
            ApplicationExitInfo exit = exits.get(0);
            if (!activity.getApplicationInfo().processName.equals(exit.getProcessName())) return false;
            if (!unexpected(exit.getReason(), exit.getStatus())) return false;
            android.content.SharedPreferences preferences = activity.getPreferences(0);
            if (exit.getTimestamp() <= preferences.getLong("reported-exit", 0)) return false;
            preferences.edit().putLong("reported-exit", exit.getTimestamp()).apply();
            String details = "@PRODUCT_NAME@ previous process\n" + Build.MANUFACTURER + " "
                    + Build.MODEL + ", Android " + Build.VERSION.RELEASE + "\n\n"
                    + "Exit reason: " + exit.getReason() + ", status: " + exit.getStatus()
                    + "\n" + exit.getDescription() + "\n"
                    + "Process: " + exit.getProcessName() + " (" + exit.getPid() + ")\n"
                    + "Exit timestamp: " + exit.getTimestamp() + "\n\n"
                    + logTail(new File(activity.getFilesDir(), "data/logs/client.log"));
            AlertDialog dialog = new AlertDialog.Builder(activity)
                    .setTitle("@PRODUCT_NAME@ stopped unexpectedly")
                    .setMessage(SHARE_NOTICE + "\n\n" + details)
                    .setPositiveButton("Retry", (which, button) -> retry.run())
                    .setNegativeButton("Save report", null)
                    .setNeutralButton("Send report", null)
                    .setOnCancelListener(which -> activity.finish())
                    .create();
            dialog.setOnShowListener(which -> {
                android.widget.TextView message = dialog.findViewById(android.R.id.message);
                if (message != null) message.setTextIsSelectable(true);
                dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(view ->
                        send(activity, dialog, exit, true));
                dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setOnClickListener(view ->
                        CrashReport.save(activity, dialog, exit, details));
            });
            dialog.show();
            send(activity, dialog, exit, false);
            return true;
        } catch (RuntimeException error) {
            android.util.Log.w("@PRODUCT_NAME@", "Cannot read previous process exit", error);
            return false;
        }
    }

    /** Serializes automatic and manual attempts, passing only Android's bounded native trace. */
    private static void send(BootstrapActivity activity, AlertDialog dialog,
            ApplicationExitInfo exit, boolean user) {
        new Thread(() -> {
            String result;
            synchronized (SEND_LOCK) {
                byte[] trace = null;
                if (Build.VERSION.SDK_INT >= 31 && exit.getReason() == ApplicationExitInfo.REASON_CRASH_NATIVE) {
                    try (java.io.InputStream input = exit.getTraceInputStream()) {
                        if (input != null) {
                            java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
                            byte[] buffer = new byte[8192];
                            for (int count; (count = input.read(buffer)) != -1;) {
                                if (out.size() + count > 4 * 1024 * 1024) break;
                                out.write(buffer, 0, count);
                            }
                            trace = out.toByteArray();
                        }
                    } catch (IOException error) { /* The error and local log still form a report. */ }
                }
                result = activity.crashReport(activity.getFilesDir().getAbsolutePath(),
                        activity.getApplicationInfo().nativeLibraryDir, exit.getTimestamp(),
                        exit.getReason(), exit.getStatus(), String.valueOf(exit.getDescription()), trace, user);
            }
            if (user) activity.runOnUiThread(() -> {
                if (!activity.isFinishing() && dialog.isShowing())
                    Toast.makeText(activity, result, Toast.LENGTH_LONG).show();
            });
        }, "crash-upload").start();
    }

    /** Completes saved main-process reports on API 26+ without waiting for network work in the UI. */
    private static void sendSaved(BootstrapActivity activity, AlertDialog dialog, boolean user) {
        new Thread(() -> {
            String result;
            synchronized (SEND_LOCK) {
                result = activity.crashReport(activity.getFilesDir().getAbsolutePath(),
                        activity.getApplicationInfo().nativeLibraryDir, 0, 0, 0, "", null, user);
            }
            if (user) activity.runOnUiThread(() -> {
                if (!activity.isFinishing() && dialog != null && dialog.isShowing())
                    Toast.makeText(activity, result, Toast.LENGTH_LONG).show();
            });
        }, "crash-retry").start();
    }

    /** Intentional activity teardown sends SIGKILL; it must not produce a crash dialog. */
    static boolean unexpected(int reason, int status) {
        return reason == ApplicationExitInfo.REASON_CRASH
                || reason == ApplicationExitInfo.REASON_CRASH_NATIVE
                || reason == ApplicationExitInfo.REASON_LOW_MEMORY
                || reason == ApplicationExitInfo.REASON_ANR
                || (reason == ApplicationExitInfo.REASON_EXIT_SELF && status != 0)
                || (reason == ApplicationExitInfo.REASON_SIGNALED && status != android.system.OsConstants.SIGKILL);
    }

    /** Reads a bounded tail of the app's own log; absent logs never prevent a retry. */
    static String logTail(File file) {
        try (RandomAccessFile source = new RandomAccessFile(file, "r")) {
            int count = (int) Math.min(source.length(), LOG_TAIL_BYTES);
            source.seek(source.length() - count);
            byte[] bytes = new byte[count];
            source.readFully(bytes);
            return new String(bytes, StandardCharsets.UTF_8);
        } catch (IOException error) {
            return "No client log was written before the process stopped.";
        }
    }
}
