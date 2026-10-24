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
import java.util.regex.Pattern;

/** Makes an unexpected exit visible on the next launch without needing USB debugging. */
final class StartupDiagnostics {
    private static final int LOG_TAIL_BYTES = 32768;
    private static final String GENERIC_EXIT = "Android stopped @PRODUCT_NAME@ for an unknown reason.";
    private static final Pattern LOG_TIMESTAMP = Pattern.compile(
            "^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d+)?Z\\s+");

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
                    + "\n" + exitCause(exit.getReason(), exit.getStatus())
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

    /** Only exits with an identified unexpected cause need a recovery dialog. */
    static boolean unexpected(int reason, int status) {
        return !GENERIC_EXIT.equals(exitCause(reason, status));
    }

    /** Names every admitted failure; intentional shutdown and unknown reasons use generic text. */
    static String exitCause(int reason, int status) {
        switch (reason) {
            case ApplicationExitInfo.REASON_CRASH:
                return "@PRODUCT_NAME@ stopped because of an application crash.";
            case ApplicationExitInfo.REASON_CRASH_NATIVE:
                return "@PRODUCT_NAME@ stopped because of a native crash.";
            case ApplicationExitInfo.REASON_LOW_MEMORY:
                return "Android closed @PRODUCT_NAME@ because the device ran low on memory.";
            case ApplicationExitInfo.REASON_ANR:
                return "Android closed @PRODUCT_NAME@ because it stopped responding.";
            case ApplicationExitInfo.REASON_EXIT_SELF:
                if (status != 0) return "@PRODUCT_NAME@ exited after an error.";
                return GENERIC_EXIT;
            case ApplicationExitInfo.REASON_SIGNALED:
                if (status != android.system.OsConstants.SIGKILL)
                    return "@PRODUCT_NAME@ stopped because it received a system signal.";
                return GENERIC_EXIT;
            default:
                return GENERIC_EXIT;
        }
    }

    /** Reads a bounded tail of the app's own log; absent logs never prevent a retry. */
    static String logTail(File file) {
        try (RandomAccessFile source = new RandomAccessFile(file, "r")) {
            int count = (int) Math.min(source.length(), LOG_TAIL_BYTES);
            source.seek(source.length() - count);
            byte[] bytes = new byte[count];
            source.readFully(bytes);
            return boundedTail(compactLog(boundedTail(bytes)).getBytes(StandardCharsets.UTF_8));
        } catch (IOException error) {
            return "No client log was written before the process stopped.";
        }
    }

    /** Collapses adjacent identical records while retaining the first timestamp and log order. */
    private static String compactLog(String log) {
        String[] lines = log.split("\\n", -1);
        StringBuilder output = new StringBuilder();
        for (int first = 0; first < lines.length; ) {
            String key = LOG_TIMESTAMP.matcher(lines[first]).replaceFirst("");
            int next = first + 1;
            while (!key.isEmpty() && next < lines.length
                    && key.equals(LOG_TIMESTAMP.matcher(lines[next]).replaceFirst(""))) next++;
            output.append(lines[first]);
            if (next - first > 1) output.append(" (repeated ").append(next - first).append(" times)");
            if (next < lines.length) output.append('\n');
            first = next;
        }
        return output.toString();
    }

    /** Keeps the newest bytes without starting inside a UTF-8 character. */
    private static String boundedTail(byte[] bytes) {
        int start = Math.max(0, bytes.length - LOG_TAIL_BYTES);
        while (start < bytes.length && (bytes[start] & 0xc0) == 0x80) start++;
        return new String(bytes, start, bytes.length - start, StandardCharsets.UTF_8);
    }
}
