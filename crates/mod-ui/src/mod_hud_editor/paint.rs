//! Shared painting for host and extension layout editors.
use super::*;
impl HudEditor {
    /// Paints the current draft and its editing controls without extension identity or controls.
    pub fn paint(
        &mut self,
        engine: Option<&FormEngine>,
        frame: &mut PaintFrame<'_>,
        animate: bool,
    ) {
        let resolved = RowIcons::resolve(&self.draft, |_, _| None);
        self.paint_with(engine, frame, &resolved, animate, &[], |_| {});
    }
    /// Supplies the optional extension's artwork, controls and reserved identity bindings.
    pub(super) fn paint_with(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
        resolved: &RowIcons,
        animate: bool,
        controls: &[ui::mod_panel::Control],
        write_identity: impl FnOnce(&mut Arc<DataSource>),
    ) {
        let Some(renderer) = engine else {
            self.finish(false);
            return;
        };
        let editor = self;
        if !editor.painting() {
            return;
        }
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let content = paint_frame.content;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if viewport[0] < 220. || viewport[1] < 120. {
            editor.finish(false);

            return;
        }
        let chrome_scale = if editor.history_buttons {
            (2. / f64::from(px)).max(1.)
        } else {
            1.
        };
        if editor.chrome_scale != chrome_scale {
            editor.chrome_scale = chrome_scale;
            editor.data_dirty = true;
            editor.catalog = None;
        }
        if editor.viewport != viewport {
            editor.data_dirty = true;
            editor.viewport = viewport;
            editor.catalog = None;
            editor.frame = None;
            editor.cancel_pointer_input();
            editor.anchor_motion = None;
        }
        editor.advance_transitions(paint_frame.now, animate);
        editor.advance_feedback(paint_frame.now, animate);
        if !editor.painting() {
            return;
        }
        editor.data_dirty |= editor.sync_control_fills(controls);
        let icons_changed = editor.row_icons != resolved.as_slice();
        if editor.data_dirty || icons_changed {
            let mut data = editor.data();
            if editor.draft.surface.is_some() {
                editor.transitions.write(&mut data);
            }
            editor.row_icons.clear();
            editor.row_icons.extend_from_slice(resolved.as_slice());
            editor.icons.clear();
            for (index, icon) in resolved.as_slice().iter().enumerate() {
                let scalar = if let Some(icon) = icon {
                    let id = editor.icons.len();
                    editor.icons.push(*icon);
                    Scalar::Num(id as f64)
                } else {
                    Scalar::Json(serde_json::Value::Null)
                };
                data.set_global(format!("#row_{index}_icon"), scalar);
            }
            if *editor.data != data {
                editor.data = Arc::new(data);
            }
            editor.data_dirty = false;
        }
        write_identity(&mut editor.data);
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let language = paint_frame.language;
        let world = editor
            .draft
            .surface
            .as_ref()
            .map(|surface| editor.transitions.world_backdrop(surface))
            .unwrap_or_default();
        if let Err(error) = paint_world_backdrop(paint_frame, world) {
            paint_frame.nodes.truncate(rollback.0);
            *paint_frame.next = rollback.1;
            editor.finish(false);
            tracing::warn!(%error, "HUD editor disabled after rendering failure");

            return;
        }
        let icons = std::mem::take(&mut editor.icons);
        let keep_out = std::mem::take(&mut editor.keep_out);
        let mut items = ItemRenderer {
            icons: &icons,
            id_aux: &[],
        };
        let mut glass = GlassRenderer::new(world);
        let mut zones = feedback::KeepOutRenderer { rects: &keep_out };
        let mut renderers: [&mut dyn CustomRenderer; 3] = [&mut items, &mut glass, &mut zones];
        // Keep pointer feedback outside the editor borrow while measuring native hit bounds.
        let view = std::mem::take(&mut editor.view);
        let art = ScreenArt {
            view: Some(&view),
            ..Default::default()
        };
        let result = renderer.draw(
            art,
            &mut paint_frame.with_renderers(&mut renderers),
            |env, root| {
                let previous = editor.pending_measurement_size();
                let measured = crate::mod_widgets::text_plate::measure(&mut editor.draft, env.text);
                let caption_measured = editor.measure_caption(env.text);
                editor.finish_measurement(previous);
                if measured || caption_measured {
                    if measured {
                        editor.catalog = None;
                    }
                    let mut data = Arc::clone(&editor.data);
                    let values = Arc::make_mut(&mut data);
                    for (n, card) in editor.draft.cards.iter().enumerate() {
                        crate::mod_widgets::text_plate::data(values, card, n, viewport);
                    }
                    editor.write_geometry(values);
                    editor.data = data;
                }
                if editor.catalog.is_none() {
                    let catalog = match template::catalog(
                        &editor.draft,
                        viewport,
                        editor.history_buttons,
                        editor.chrome_scale,
                    ) {
                        Ok(catalog) => catalog,
                        Err(error) => {
                            tracing::warn!(%error,"HUD editor template rejected");
                            return None;
                        }
                    };
                    editor.catalog = Some(Arc::new(catalog));
                    editor.screen = CachedScreen::default();
                }
                editor.screen.render_shared_with(
                    SCREEN,
                    editor.catalog.as_ref().expect("installed"),
                    &Context::default(),
                    Arc::clone(&editor.data),
                    (root, px, language),
                    env,
                    &view,
                )
            },
        );
        editor.view = view;
        editor.icons = icons;
        editor.keep_out = keep_out;
        match result {
            Ok(Some(frame)) => editor.frame = Some(frame),
            result => {
                paint_frame.nodes.truncate(rollback.0);
                *paint_frame.next = rollback.1;
                editor.finish(false);
                if let Err(error) = result {
                    tracing::warn!(%error, "HUD editor disabled after rendering failure");
                }
            }
        }
    }
}
