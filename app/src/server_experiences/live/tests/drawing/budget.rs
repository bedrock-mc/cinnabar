use super::*;
use server_experience::runtime::SceneObject;

/// Declares one legacy object whose asset is admitted by the fixture bundle.
fn legacy(triangles: Option<u32>) -> Command {
    let transform = [0., 0., 0., 0., 0., 0., 1., 1., 1., 1.];
    Command::Scene {
        id: 1,
        object: Some(match triangles {
            Some(triangles) => SceneObject::Mesh {
                asset: "legacy".into(),
                transform,
                triangles,
            },
            None => SceneObject::Quad {
                texture: "legacy".into(),
                transform,
                size: [1.; 2],
            },
        }),
    }
}

#[test]
fn legacy_and_retained_output_share_limits_in_every_completion_order() {
    for (draws, triangles) in [(MAX_DRAWS as usize, None), (1, Some(MAX_TRIANGLES))] {
        for order in 0..3 {
            let mut live = initialized(1);
            let instance = live.instances.get_mut("bundle0").unwrap();
            instance
                .capabilities
                .scope
                .permissions
                .insert(Permission::Scene);
            instance.capabilities.assets.insert("legacy".into());
            let command = legacy(triangles);
            if order == 0 {
                instance.dispatch(None, Event::Frame, 1, None).unwrap();
                instance
                    .helper
                    .as_mut()
                    .unwrap()
                    .response
                    .as_mut()
                    .unwrap()
                    .commands = vec![command.clone()];
                live.poll(1, CALLBACK_INTERVAL_MS, false, |_| true).unwrap();
                assert!(live.instances["bundle0"].stopped.is_none());
            }
            scene(&mut live, "bundle0", draws);
            if order == 2 {
                live.instances
                    .get_mut("bundle0")
                    .unwrap()
                    .helper
                    .as_mut()
                    .unwrap()
                    .response
                    .as_mut()
                    .unwrap()
                    .commands
                    .push(command.clone());
            }
            live.poll(1, 2 * CALLBACK_INTERVAL_MS, false, |_| true)
                .unwrap();
            if order == 1 {
                assert!(live.instances["bundle0"].stopped.is_none());
                let instance = live.instances.get_mut("bundle0").unwrap();
                instance.dispatch(None, Event::Frame, 1, None).unwrap();
                instance
                    .helper
                    .as_mut()
                    .unwrap()
                    .response
                    .as_mut()
                    .unwrap()
                    .commands = vec![command];
                live.poll(1, 3 * CALLBACK_INTERVAL_MS, false, |_| true)
                    .unwrap();
            }
            assert!(
                live.instances["bundle0"].stopped.is_some(),
                "order {order}, triangles {triangles:?}"
            );
            assert!(live.drawing().0.retained.is_empty());
        }
    }
}

#[test]
fn combined_scene_accepts_the_exact_draw_and_triangle_limits() {
    for (draws, triangles) in [(MAX_DRAWS as usize - 1, None), (1, Some(MAX_TRIANGLES - 1))] {
        let mut live = initialized(1);
        scene(&mut live, "bundle0", draws);
        let instance = live.instances.get_mut("bundle0").unwrap();
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::Scene);
        instance.capabilities.assets.insert("legacy".into());
        instance
            .helper
            .as_mut()
            .unwrap()
            .response
            .as_mut()
            .unwrap()
            .commands
            .push(legacy(triangles));
        live.poll(1, CALLBACK_INTERVAL_MS, false, |_| true).unwrap();
        assert!(live.instances["bundle0"].stopped.is_none());
        assert_eq!(live.instances["bundle0"].contributions.scene.len(), 1);
        assert_eq!(live.drawing().0.retained[0].instances.len(), draws);
    }
}
