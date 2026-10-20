//! Renders the same settings controller before and after a tab round trip.

use std::path::Path;

use client_ui::ui_runtime::{UiRuntime, forms::FormValue};
use protocol::{CustomForm, CustomFormElement, FormKind, FormRequestEvent, ServerFormModel};

use crate::{carriers::Error, fixtures::State};

/// Edits settings, visits another tab and paints the reopened draft through production code.
pub(super) fn png(state: &State, dir: &Path) -> Result<Vec<u8>, Error> {
    let mut runtime = UiRuntime::new(1);
    let store = runtime.server_forms_mut();
    store.flush_settings_request(true, || true);
    store.select_settings_form(true);
    store.admit(
        FormRequestEvent {
            form_id: 7,
            kind: FormKind::ServerSettings,
            title: Some("Server Settings".into()),
            json: "{}".into(),
            model: ServerFormModel::Custom(CustomForm {
                title: "Server Settings".into(),
                icon: None,
                submit: None,
                elements: vec![
                    CustomFormElement::Toggle {
                        text: "Enable feature".into(),
                        default: false,
                        tooltip: None,
                    },
                    CustomFormElement::Input {
                        text: "Message".into(),
                        placeholder: "".into(),
                        default: "Initial value".into(),
                        tooltip: None,
                    },
                ]
                .into(),
            }),
        },
        1,
        1,
        false,
    );
    store.engine_mut().values[0] = FormValue::Toggle(true);
    store.engine_mut().values[1] = FormValue::Text("Edited value".into());
    store.select_settings_form(false);
    store.select_settings_form(true);
    super::form_overlap::png_for(state, dir, &runtime)
}
