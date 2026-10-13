package @APPLICATION_ID@;

import android.app.NativeActivity;

/** Hosts the client, whose JSON-UI engine draws every game screen. */
public final class CinnabarActivity extends NativeActivity {
    @Override protected void onDestroy() {
        // The native loop never returns on destroy, which NativeActivity's teardown waits for,
        // and Bevy cannot start twice in one process anyway.
        android.os.Process.killProcess(android.os.Process.myPid());
        super.onDestroy();
    }
}
