package @APPLICATION_ID@;

import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.Binder;
import android.os.IBinder;
import android.os.Parcel;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

/** Private GPU service bridge; callers must allow native activity lifecycle events to run. */
public final class GpuProbe {
    /** Runs one isolated request with the timeout supplied by the Rust policy. */
    public static String run(Context context, String arguments, long timeoutMillis) throws Exception {
        final long started = android.os.SystemClock.elapsedRealtime();
        final CountDownLatch done = new CountDownLatch(1);
        final AtomicReference<String> result = new AtomicReference<>();
        final Binder callback = new Binder() {
            /** Receives the service result before its process exits. */
            @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) {
                if (code != IBinder.FIRST_CALL_TRANSACTION) return false;
                result.compareAndSet(null, data.readString());
                return true;
            }
        };
        ServiceConnection connection = new ServiceConnection() {
            private boolean dispatched;
            /** Dispatches the request when Android has created the separate process. */
            @Override public void onServiceConnected(ComponentName name, IBinder service) {
                if (dispatched) return;
                dispatched = true;
                Parcel data = Parcel.obtain();
                try {
                    service.linkToDeath(() -> done.countDown(), 0);
                    data.writeString(arguments);
                    data.writeLong(Math.max(1, timeoutMillis - (android.os.SystemClock.elapsedRealtime() - started)));
                    data.writeStrongBinder(callback);
                    service.transact(IBinder.FIRST_CALL_TRANSACTION, data, null, IBinder.FLAG_ONEWAY);
                } catch (android.os.RemoteException error) {
                    result.compareAndSet(null, "{\"verdict\":\"inconclusive\",\"stdout\":\"\"}");
                    done.countDown();
                } finally { data.recycle(); }
            }
            /** A missing reply cannot distinguish a native crash from service shutdown. */
            @Override public void onServiceDisconnected(ComponentName name) {
                // The death recipient completes the wait after any synchronous result callback.
            }
        };
        if (!context.bindService(new Intent(context, GpuProbeService.class), connection, Context.BIND_AUTO_CREATE)) {
            return "{\"verdict\":\"inconclusive\",\"stdout\":\"\"}";
        }
        try {
            if (!done.await(timeoutMillis, TimeUnit.MILLISECONDS)) {
                return "{\"verdict\":\"timed_out\",\"stdout\":\"\"}";
            }
            String output = result.get();
            return output == null ? "{\"verdict\":\"inconclusive\",\"stdout\":\"\"}" : output;
        } finally { context.unbindService(connection); }
    }
}
