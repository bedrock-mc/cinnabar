package @APPLICATION_ID@;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.os.SystemClock;
import android.view.View;
import java.io.InputStream;
import java.io.ByteArrayOutputStream;
import org.json.JSONArray;
import org.json.JSONObject;

/** Original feedback drawn before native resources and the destination interface are ready. */
final class StartupScreen extends View {
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final JSONObject specification;
    private final Bitmap logo;
    private final JSONObject layout;
    private String stage = "loading_assets";
    private boolean retired;

    StartupScreen(Context context) {
        super(context);
        try {
            specification = new JSONObject(read(context, "startup-screen.json"));
            layout = specification.getJSONObject("layout");
            try (InputStream input = context.getAssets().open("startup-title.png")) {
                logo = BitmapFactory.decodeStream(input);
            }
            String[] fonts = context.getAssets().list("resources/fonts");
            if (fonts != null && fonts.length == 1) {
                paint.setTypeface(Typeface.createFromAsset(context.getAssets(), "resources/fonts/" + fonts[0]));
            }
        } catch (Exception error) {
            throw new IllegalStateException("Cannot load startup artwork", error);
        }
        setBackgroundColor(color("background"));
    }

    /** Applies native work labels; readiness permanently removes this activity's overlay. */
    void update(String message) {
        if (retired) return;
        try {
            JSONObject value = new JSONObject(message);
            if ("ready".equals(value.getString("kind"))) {
                retired = true;
                if (getParent() instanceof android.view.ViewGroup) {
                    ((android.view.ViewGroup) getParent()).removeView(this);
                }
            } else {
                String next = value.getString("stage");
                if (specification.has(next)) stage = next;
                invalidate();
            }
        } catch (Exception error) {
            android.util.Log.w("StartupScreen", "Cannot read startup progress", error);
        }
    }

    /** Reads one bundled specification without depending on the unpacked prep kit. */
    private static String read(Context context, String path) throws Exception {
        try (InputStream input = context.getAssets().open(path);
                ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[4096];
            for (int count; (count = input.read(buffer)) != -1;) output.write(buffer, 0, count);
            return new String(output.toByteArray(), java.nio.charset.StandardCharsets.UTF_8);
        }
    }

    /** Converts the shared RGBA color to Android's ARGB representation. */
    private int color(String key) {
        JSONArray rgba = specification.optJSONArray(key);
        return android.graphics.Color.argb(rgba.optInt(3), rgba.optInt(0), rgba.optInt(1), rgba.optInt(2));
    }

    /** Reads one shared design-space metric. */
    private float metric(String name) { return (float) layout.optDouble(name); }

    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        float width = getWidth(), height = getHeight();
        float scale = Math.max(metric("minimum_scale"), Math.min(width / (float) layout.optJSONArray("design_size").optDouble(0),
                height / (float) layout.optJSONArray("design_size").optDouble(1)));
        if (logo != null) {
            float w = Math.min(metric("logo_width") * scale, width - metric("side_margin") * scale);
            float h = w * logo.getHeight() / logo.getWidth();
            paint.setAlpha(255);
            canvas.drawBitmap(logo, null, new android.graphics.RectF((width-w)/2, height*metric("logo_center_y")-h/2,
                    (width+w)/2, height*metric("logo_center_y")+h/2), paint);
        }
        paint.setColor(color("foreground"));
        paint.setTextSize(metric("label_size") * scale);
        paint.setTextAlign(Paint.Align.CENTER);
        canvas.drawText(specification.optString(stage), width/2, height*metric("label_top")-paint.ascent(), paint);
        int active = (int) ((SystemClock.uptimeMillis()/layout.optLong("step_millis")) % 3);
        for (int i=0; i<3; ++i) {
            paint.setColor(color(i == active ? "accent" : "foreground"));
            paint.setAlpha(i == active ? 255 : 70);
            float x = width/2 + (i-1)*metric("dot_spacing")*scale - metric("dot_size")*.5f*scale;
            canvas.drawRect(x, height*metric("indicator_top"), x+metric("dot_size")*scale, height*metric("indicator_top")+metric("dot_size")*scale, paint);
        }
        paint.setAlpha(255);
        if (!retired && isShown()) postInvalidateDelayed(layout.optLong("frame_millis"));
    }
}
