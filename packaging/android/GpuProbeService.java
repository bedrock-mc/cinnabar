package @APPLICATION_ID@;

import android.app.Service;
import android.content.Intent;
import android.graphics.SurfaceTexture;
import android.os.Binder;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.Parcel;
import android.os.Process;
import android.view.Surface;

/** Runs one driver request in a private process, with an independent watchdog. */
public final class GpuProbeService extends Service {
    private final Handler main = new Handler(Looper.getMainLooper());
    private boolean started;

    /** Runs Rust's shared device and presentation test on the worker thread. */
    private static native String execute(String arguments, Surface surface);

    private final Binder binder = new Binder() {
        /** Accepts a single request and returns its result through the caller's binder. */
        @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) {
            if (code != IBinder.FIRST_CALL_TRANSACTION) return false;
            final String arguments = data.readString();
            final long timeoutMillis = data.readLong();
            final IBinder callback = data.readStrongBinder();
            main.post(() -> startProbe(arguments, timeoutMillis, callback));
            return true;
        }
    };

    /** Gives the same-application caller access to this private process. */
    @Override public IBinder onBind(Intent intent) { return binder; }

    /** Starts exactly one request; the watchdog can kill a wedged native worker. */
    private void startProbe(String arguments, long timeoutMillis, IBinder callback) {
        if (started) return;
        started = true;
        main.postDelayed(() -> finishProbe(callback, "{\"verdict\":\"timed_out\",\"stdout\":\"\"}"), timeoutMillis);
        new Thread(() -> {
            String result;
            try {
                System.loadLibrary("@CLIENT_LIBRARY@");
                SurfaceTexture texture = new SurfaceTexture(false);
                texture.setDefaultBufferSize(16, 16);
                Surface surface = new Surface(texture);
                try { result = execute(arguments, surface); }
                finally { surface.release(); texture.release(); }
            } catch (Throwable error) {
                result = "{\"verdict\":\"failed\",\"stdout\":\"\"}";
            }
            final String output = result;
            main.post(() -> finishProbe(callback, output));
        }, "gpu-probe").start();
    }

    /** Sends a bounded result, then stops only this service process. */
    private void finishProbe(IBinder callback, String result) {
        Parcel parcel = Parcel.obtain();
        Parcel reply = Parcel.obtain();
        try {
            parcel.writeString(result);
            callback.transact(IBinder.FIRST_CALL_TRANSACTION, parcel, reply, 0);
        } catch (android.os.RemoteException ignored) {
        } finally {
            parcel.recycle();
            reply.recycle();
            Process.killProcess(Process.myPid());
        }
    }
}
