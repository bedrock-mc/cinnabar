package @APPLICATION_ID@;

import android.app.Activity;
import android.app.ActivityManager;
import android.app.AlertDialog;
import android.app.ApplicationExitInfo;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.os.Build;
import android.widget.Toast;
import java.io.File;
import java.io.IOException;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.util.List;

/** Makes an unexpected exit visible on the next launch without needing USB debugging. */
final class StartupDiagnostics {
    private static final int LOG_TAIL_BYTES = 32768;

    /** Offers retry and copyable diagnostics once for each unexpected main-process exit. */
    static boolean showPreviousFailure(Activity activity, Runnable retry) {
        if (Build.VERSION.SDK_INT < 30) return false;
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
            String details = "@PRODUCT_NAME@ @VERSION_NAME@\n" + Build.MANUFACTURER + " "
                    + Build.MODEL + ", Android " + Build.VERSION.RELEASE + "\n\n"
                    + "Exit reason: " + exit.getReason() + ", status: " + exit.getStatus()
                    + "\n" + exit.getDescription() + "\n"
                    + "Process: " + exit.getProcessName() + " (" + exit.getPid() + ")\n"
                    + "Exit timestamp: " + exit.getTimestamp() + "\n\n"
                    + logTail(new File(activity.getFilesDir(), "data/logs/client.log"));
            AlertDialog dialog = new AlertDialog.Builder(activity)
                    .setTitle("@PRODUCT_NAME@ stopped unexpectedly")
                    .setMessage("Save report puts this log and any Android crash trace in Downloads.\n\n" + details)
                    .setPositiveButton("Retry", (which, button) -> retry.run())
                    .setNegativeButton("Save report", null)
                    .setNeutralButton("Copy details", null)
                    .setOnCancelListener(which -> activity.finish())
                    .create();
            dialog.setOnShowListener(which -> {
                dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(view -> {
                    activity.getSystemService(ClipboardManager.class)
                            .setPrimaryClip(ClipData.newPlainText("@PRODUCT_NAME@ diagnostics", details));
                    Toast.makeText(activity, "Diagnostic details copied", Toast.LENGTH_SHORT).show();
                });
                dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setOnClickListener(view ->
                        CrashReport.save(activity, dialog, exit, details));
            });
            dialog.show();
            return true;
        } catch (RuntimeException error) {
            android.util.Log.w("@PRODUCT_NAME@", "Cannot read previous process exit", error);
            return false;
        }
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
