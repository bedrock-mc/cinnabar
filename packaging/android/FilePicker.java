package @APPLICATION_ID@;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ContentResolver;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;

/** Lets native import workers choose documents through the Storage Access Framework. */
public final class FilePicker {
    /** Distinguishes picker results from any other activity result. */
    static final int REQUEST_CODE = 0x4349;
    private static final Object LOCK = new Object();
    private static Activity activity;
    private static Pending pending;
    private static int selections;

    private FilePicker() { }

    /** One open document request, completed on the activity thread. */
    private static final class Pending {
        boolean done;
        boolean unavailable;
        final List<Uri> uris = new ArrayList<>();
    }

    /** Publishes the activity that hosts pickers; called from its onCreate. */
    static void attach(Activity host) {
        synchronized (LOCK) { activity = host; }
    }

    /**
     * Blocks a native worker thread until the user chooses documents, then copies them into
     * private import storage so native code can read them by path. Returns null when no picker
     * could open and an empty array on cancellation. Each selection gets its own directory, since
     * another worker may still be importing an earlier one; the first selection of a process
     * clears earlier sessions' copies, and Android may reclaim the cache directory at any time.
     * A document larger than a non-negative {@code maxBytes} fails the selection without
     * filling storage; a failed selection removes its copies. The picker shows {@code mimeType}
     * documents, or all documents when the caller passes a wildcard for custom file types.
     */
    public static String[] pick(boolean multiple, long maxBytes, String mimeType)
            throws IOException, InterruptedException {
        final Activity host;
        final Pending request = new Pending();
        final int selection;
        final File imports;
        synchronized (LOCK) {
            if (activity == null) return null;
            if (pending != null) throw new IOException("Another file picker is already open");
            host = activity;
            pending = request;
            selection = selections++;
            imports = new File(host.getCacheDir(), "imports");
            if (selection == 0) delete(imports);
        }
        final Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType(mimeType)
                .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
        host.runOnUiThread(() -> {
            try {
                host.startActivityForResult(intent, REQUEST_CODE);
            } catch (ActivityNotFoundException | SecurityException error) {
                synchronized (LOCK) {
                    request.unavailable = true;
                    finish(request);
                }
            }
        });
        synchronized (LOCK) {
            try {
                while (!request.done) LOCK.wait();
            } finally {
                if (pending == request) pending = null;
            }
        }
        if (request.unavailable) return null;
        File directory = new File(imports, Integer.toString(selection));
        List<String> paths = new ArrayList<>();
        ContentResolver resolver = host.getContentResolver();
        try {
            for (int index = 0; index < request.uris.size(); index++) {
                Uri uri = request.uris.get(index);
                String name = name(resolver, uri);
                File target = new File(new File(directory, Integer.toString(index)), name);
                if (!target.getParentFile().mkdirs()) {
                    throw new IOException("Could not create " + target.getParent());
                }
                copy(resolver, uri, target, name, maxBytes);
                paths.add(target.getAbsolutePath());
            }
        } catch (IOException | RuntimeException error) {
            delete(directory);
            throw error;
        }
        return paths.toArray(new String[0]);
    }

    /** Completes the pending request from the activity's result; false for other requests. */
    static boolean onResult(int requestCode, int resultCode, Intent data) {
        if (requestCode != REQUEST_CODE) return false;
        synchronized (LOCK) {
            Pending request = pending;
            if (request == null) return true;
            if (resultCode == Activity.RESULT_OK && data != null) {
                ClipData clip = data.getClipData();
                if (clip != null) {
                    for (int index = 0; index < clip.getItemCount(); index++) {
                        Uri uri = clip.getItemAt(index).getUri();
                        if (uri != null) request.uris.add(uri);
                    }
                } else if (data.getData() != null) {
                    request.uris.add(data.getData());
                }
            }
            finish(request);
        }
        return true;
    }

    /** Wakes the waiting worker; callers hold LOCK. */
    private static void finish(Pending request) {
        request.done = true;
        LOCK.notifyAll();
    }

    /** The document's display name, kept as one path segment so its extension survives. */
    private static String name(ContentResolver resolver, Uri uri) {
        String name = null;
        try (Cursor cursor = resolver.query(
                uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) {
                name = cursor.getString(0);
            }
        } catch (RuntimeException ignored) {
            // Some providers reject metadata queries but still open the document.
        }
        if (name == null) name = uri.getLastPathSegment();
        if (name == null) name = "";
        name = name.replace('/', '_').replace('\0', '_');
        return name.isEmpty() || name.equals(".") || name.equals("..") ? "import" : name;
    }

    /** Streams one document into private storage, stopping once it exceeds a non-negative limit. */
    private static void copy(ContentResolver resolver, Uri uri, File target, String name,
            long maxBytes) throws IOException {
        try (InputStream input = resolver.openInputStream(uri);
             OutputStream output = new FileOutputStream(target)) {
            if (input == null) throw new IOException("Could not open " + name);
            byte[] buffer = new byte[64 * 1024];
            long copied = 0;
            for (int count; (count = input.read(buffer)) != -1; ) {
                copied += count;
                if (maxBytes >= 0 && copied > maxBytes) {
                    throw new IOException(name + " is larger than the " + size(maxBytes)
                            + " import limit");
                }
                output.write(buffer, 0, count);
            }
        }
    }

    /** Formats a byte limit for the import status line. */
    private static String size(long bytes) {
        long mebibyte = 1024 * 1024;
        return bytes >= mebibyte && bytes % mebibyte == 0
                ? bytes / mebibyte + " MiB" : bytes + "-byte";
    }

    /** Removes a directory tree of import copies. */
    private static void delete(File file) {
        File[] children = file.listFiles();
        if (children != null) for (File child : children) delete(child);
        file.delete();
    }
}
