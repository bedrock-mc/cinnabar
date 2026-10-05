use serde_json::Value;
use ui::mod_panel::Icon;

use super::widgets::{named, panel, rounded};

/// Small vector marks use the same antialiased JSON-UI geometry as the controls.
pub(super) fn mark(kind: Icon, size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    let mut strokes = Vec::new();
    match kind {
        Icon::Pointer => {
            for (a, b) in [
                ([2., 1.], [4., 13.]),
                ([2., 1.], [13., 7.]),
                ([13., 7.], [8., 8.]),
                ([8., 8.], [11., 13.]),
                ([11., 13.], [9., 14.]),
                ([9., 14.], [6., 9.]),
                ([6., 9.], [4., 13.]),
            ] {
                line(&mut strokes, a, b, 1.2, color);
            }
        }
        Icon::Crosshair => {
            ring(&mut strokes, [8., 8.], 4.5, color);
            for (a, b) in [
                ([8., 0.5], [8., 5.]),
                ([8., 11.], [8., 15.5]),
                ([0.5, 8.], [5., 8.]),
                ([11., 8.], [15.5, 8.]),
            ] {
                line(&mut strokes, a, b, 1.2, color);
            }
        }
        Icon::Ruler => {
            for (a, b) in [
                ([1., 11.], [11., 1.]),
                ([11., 1.], [15., 5.]),
                ([15., 5.], [5., 15.]),
                ([5., 15.], [1., 11.]),
                ([4., 8.], [6., 10.]),
                ([7., 5.], [9., 7.]),
                ([10., 2.], [12., 4.]),
            ] {
                line(&mut strokes, a, b, 1., color);
            }
        }
        Icon::Settings => {
            ring(&mut strokes, [8., 8.], 4.7, color);
            ring(&mut strokes, [8., 8.], 1.8, color);
            for n in 0..8 {
                let angle = n as f64 * std::f64::consts::TAU / 8.;
                let point = |r: f64| [8. + r * angle.cos(), 8. + r * angle.sin()];
                line(&mut strokes, point(4.5), point(6.5), 1.6, color);
            }
        }
        Icon::None => {}
    }
    let scale = size / 16.;
    for stroke in &mut strokes {
        let node = stroke.as_object_mut().unwrap().values_mut().next().unwrap();
        for field in ["offset", "size"] {
            for coordinate in node[field].as_array_mut().unwrap() {
                *coordinate = serde_json::json!(coordinate.as_f64().unwrap() * scale);
            }
        }
        node["radius"] = serde_json::json!(node["radius"].as_f64().unwrap() * scale);
    }
    panel([size, size], offset, strokes)
}

pub(super) fn brand(size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    let mut strokes = Vec::new();
    let unit = size / 16.;
    for (a, b) in [
        ([13., 4.], [8., 1.]),
        ([8., 1.], [2., 4.5]),
        ([2., 4.5], [2., 11.5]),
        ([2., 11.5], [8., 15.]),
        ([8., 15.], [13., 12.]),
    ] {
        line(
            &mut strokes,
            [a[0] * unit, a[1] * unit],
            [b[0] * unit, b[1] * unit],
            2.8 * unit,
            color,
        );
    }
    panel([size, size], offset, strokes)
}

pub(super) fn sword(size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    let u = size / 16.;
    let mut strokes = Vec::new();
    for (a, b, weight) in [
        ([3., 13.], [13., 3.], 2.),
        ([3., 8.], [8., 13.], 1.5),
        ([11., 3.], [14., 2.], 1.),
        ([14., 2.], [13., 5.], 1.),
    ] {
        line(
            &mut strokes,
            [a[0] * u, a[1] * u],
            [b[0] * u, b[1] * u],
            weight * u,
            color,
        );
    }
    panel([size, size], offset, strokes)
}

pub(super) fn close(size: f64, offset: [f64; 2], color: [f64; 4]) -> Value {
    let mut strokes = Vec::new();
    line(&mut strokes, [1., 1.], [size - 1., size - 1.], 0.7, color);
    line(&mut strokes, [size - 1., 1.], [1., size - 1.], 0.7, color);
    panel([size, size], offset, strokes)
}

pub(super) fn chevron(offset: [f64; 2], color: [f64; 4]) -> Value {
    let mut strokes = Vec::new();
    line(&mut strokes, [0., 0.], [2.5, 2.5], 0.8, color);
    line(&mut strokes, [2.5, 2.5], [5., 0.], 0.8, color);
    panel([5., 3.], offset, strokes)
}

fn ring(strokes: &mut Vec<Value>, center: [f64; 2], radius: f64, color: [f64; 4]) {
    for n in 0..32 {
        let angle = n as f64 * std::f64::consts::TAU / 32.;
        dot(
            strokes,
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ],
            1.,
            color,
        );
    }
}

fn line(strokes: &mut Vec<Value>, a: [f64; 2], b: [f64; 2], weight: f64, color: [f64; 4]) {
    let steps = ((b[0] - a[0]).hypot(b[1] - a[1]) / 0.5).ceil() as usize;
    for n in 0..=steps {
        let t = n as f64 / steps.max(1) as f64;
        dot(
            strokes,
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            weight,
            color,
        );
    }
}

fn dot(strokes: &mut Vec<Value>, point: [f64; 2], weight: f64, color: [f64; 4]) {
    strokes.push(named(
        &format!("stroke_{}", strokes.len()),
        rounded(
            [weight; 2],
            [point[0] - weight / 2., point[1] - weight / 2.],
            weight / 2.,
            color,
        ),
    ));
}
