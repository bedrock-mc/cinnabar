//! Converts launcher purchase state to the existing JSON-UI modal presentation.

use json_ui::ModalForm;
pub use launcher::store::flow::{DISABLED_BODY, DISABLED_TITLE, PurchaseDialog, PurchaseFlow};

/// Builds the modal associated with a purchase flow.
pub trait PurchaseFlowPresentation {
    /// Localizes the current purchase state into a modal, when it has one.
    fn modal(&self, tr: &dyn Fn(&str) -> String) -> Option<ModalForm>;
}

impl PurchaseFlowPresentation for PurchaseFlow {
    /// The modal for the current state; `tr` maps a vanilla lang key to text. Success shows as a toast.
    fn modal(&self, tr: &dyn Fn(&str) -> String) -> Option<ModalForm> {
        match self {
            Self::Idle => None,
            Self::Confirming { title, body, .. } => Some(ModalForm {
                title: title.clone(),
                body: body.clone(),
                button1: tr("store.purchase.bundle.confirm"),
                button2: tr("gui.cancel"),
            }),
            Self::InProgress { .. } => Some(ModalForm {
                title: tr("store.popup.purchaseInProgress.title"),
                body: tr("store.popup.purchaseInProgress.msg"),
                ..ModalForm::default()
            }),
            Self::Done(dialog) => dialog.modal(tr),
        }
    }
}

/// Builds modal and toast content for a completed purchase.
pub trait PurchaseDialogPresentation {
    /// Localizes a completed purchase result into a modal, when it has one.
    fn modal(&self, tr: &dyn Fn(&str) -> String) -> Option<ModalForm>;
    /// Localizes the successful-purchase toast, when it has one.
    #[cfg(test)]
    fn toast(&self, tr: &dyn Fn(&str) -> String) -> Option<String>;
}

impl PurchaseDialogPresentation for PurchaseDialog {
    fn modal(&self, tr: &dyn Fn(&str) -> String) -> Option<ModalForm> {
        let close = tr("gui.close");
        Some(match self {
            Self::Success { .. } => return None,
            Self::InsufficientFunds { .. } => ModalForm {
                title: tr("store.popup.purchaseFailedInsufficientFunds.title"),
                body: tr("store.popup.purchaseFailedInsufficientFunds.msg"),
                button1: tr("store.popup.purchaseFailedInsufficientFunds.buyButton"),
                button2: close,
            },
            Self::PriceMismatch => ModalForm {
                title: tr("store.popup.purchaseFailed.title"),
                body: tr("store.popup.purchasePriceMismatch.msg"),
                button1: close,
                ..ModalForm::default()
            },
            Self::Pending => ModalForm {
                title: tr("store.popup.purchasePending.title"),
                body: tr("store.popup.purchasePending.msg"),
                button1: close,
                ..ModalForm::default()
            },
            Self::Failed {
                marketplace_error_code,
                correlation_id,
            } => {
                let mut body = tr("store.popup.purchaseFailed.msg");
                if let Some(code) = marketplace_error_code {
                    body.push_str(&format!(
                        "\n{}",
                        tr("store.csb.purchaseErrorDialog.errorCode")
                            .replace("%s", &code.to_string())
                    ));
                }
                if let Some(id) = correlation_id {
                    body.push_str(&format!(
                        "\n{}",
                        tr("store.csb.purchaseErrorDialog.correlationId").replace("%s", id)
                    ));
                }
                ModalForm {
                    title: tr("store.popup.purchaseFailed.title"),
                    body,
                    button1: close,
                    ..ModalForm::default()
                }
            }
            Self::Disabled => ModalForm {
                title: DISABLED_TITLE.to_owned(),
                body: DISABLED_BODY.to_owned(),
                button1: close,
                ..ModalForm::default()
            },
            Self::SignedOut => ModalForm {
                title: tr("store.popup.xblRequired.title"),
                body: tr("store.popup.xblRequired.message"),
                button1: tr("store.popup.xblRequired.button1"),
                button2: tr("store.popup.xblRequired.button2"),
            },
        })
    }

    /// The success toast (`store.purchase.success`).
    #[cfg(test)]
    fn toast(&self, tr: &dyn Fn(&str) -> String) -> Option<String> {
        match self {
            Self::Success { title } => Some(tr("store.purchase.success").replace("%s", title)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::store::flow::Begin;
    use launcher::store::worker::StoreError;
    use protocol::store_control::{
        PurchaseOutcome, PurchaseStatus, StoreBalance, StoreOffer, StorePrice,
    };

    fn offer() -> StoreOffer {
        StoreOffer {
            id: "o1".into(),
            title: "Castle".into(),
            creator: None,
            content_type: None,
            thumbnail_url: None,
            store_id: Some("s1".into()),
            prices: vec![],
            rating: None,
            tags: vec![],
            owned: false,
        }
    }

    fn price(amount: i64) -> StorePrice {
        StorePrice {
            currency: "mc".into(),
            amount,
        }
    }

    fn balances(amount: i64) -> Vec<StoreBalance> {
        vec![StoreBalance {
            currency: "mc".into(),
            amount,
        }]
    }

    fn outcome(status: PurchaseStatus) -> PurchaseOutcome {
        PurchaseOutcome {
            status,
            http_status: 200,
            marketplace_error_code: 0,
            correlation_id: "c1".into(),
            inventory_version: None,
            replayed: false,
        }
    }

    fn tr(key: &str) -> String {
        format!("<{key}>")
    }

    #[test]
    fn a_bundle_waits_for_confirmation_and_cancel_sends_nothing() {
        let confirm = Some(("Unlock 2 of 3 Packs?".to_owned(), "You'll get".to_owned()));
        let mut flow = PurchaseFlow::Idle;
        assert_eq!(
            flow.begin(
                &offer(),
                &price(320),
                &balances(500),
                confirm.clone(),
                "id".into(),
                true
            ),
            Begin::Showing
        );
        let modal = flow.modal(&tr).expect("confirmation modal");
        assert_eq!(modal.button1, "<store.purchase.bundle.confirm>");
        flow.dismiss();
        assert!(flow.is_idle() && flow.confirm("x".into(), true).is_none());

        flow.begin(
            &offer(),
            &price(320),
            &balances(500),
            confirm,
            "id".into(),
            true,
        );
        let purchase = flow.confirm("id-2".into(), true).expect("confirmed");
        assert_eq!(purchase.purchase_id(), "id-2");
        assert!(matches!(flow, PurchaseFlow::InProgress { .. }));
    }

    #[test]
    fn outcomes_map_to_the_vanilla_dialogs() {
        let cases = [
            (
                PurchaseStatus::PriceMismatch,
                "<store.popup.purchasePriceMismatch.msg>",
            ),
            (PurchaseStatus::Unknown, "<store.popup.purchasePending.msg>"),
        ];
        for (status, body) in cases {
            let mut flow = PurchaseFlow::Idle;
            flow.begin(&offer(), &price(1), &balances(5), None, "id".into(), true);
            flow.finish("id", Ok(outcome(status)));
            assert_eq!(flow.modal(&tr).expect("modal").body, body);
        }
        let mut flow = PurchaseFlow::Idle;
        flow.begin(&offer(), &price(1), &balances(5), None, "id".into(), true);
        flow.finish("stale", Ok(outcome(PurchaseStatus::Purchased)));
        assert!(
            matches!(flow, PurchaseFlow::InProgress { .. }),
            "another attempt's answer is dropped"
        );
        flow.finish("id", Ok(outcome(PurchaseStatus::Purchased)));
        let PurchaseFlow::Done(dialog) = &flow else {
            panic!("not done: {flow:?}");
        };
        assert_eq!(
            dialog.toast(&tr).as_deref(),
            Some("<store.purchase.success>")
        );
        assert!(flow.modal(&tr).is_none());
    }

    #[test]
    fn failures_carry_codes_and_a_dead_session_asks_to_sign_in() {
        let mut flow = PurchaseFlow::Idle;
        flow.begin(&offer(), &price(1), &balances(5), None, "id".into(), true);
        let mut failed = outcome(PurchaseStatus::Failed);
        failed.marketplace_error_code = 1502;
        flow.finish("id", Ok(failed));
        let body = flow.modal(&tr).expect("modal").body;
        assert!(
            body.contains("<store.csb.purchaseErrorDialog.errorCode>")
                && body.contains("<store.popup.purchaseFailed.msg>")
        );
        flow.dismiss();
        flow.begin(&offer(), &price(1), &balances(5), None, "id2".into(), true);
        flow.finish("id2", Err(StoreError::SignedOut));
        assert_eq!(flow, PurchaseFlow::Done(PurchaseDialog::SignedOut));
    }

    #[test]
    fn disabled_purchases_show_the_notice_and_never_send() {
        let mut flow = PurchaseFlow::Idle;
        let begun = flow.begin(&offer(), &price(5), &balances(10), None, "id".into(), false);
        assert_eq!(begun, Begin::Showing);
        assert_eq!(flow, PurchaseFlow::Done(PurchaseDialog::Disabled));
        assert_eq!(flow.modal(&tr).expect("modal").body, DISABLED_BODY);
        flow.dismiss();

        // A confirmation that is answered after purchases were switched off is stopped too.
        let confirm = Some(("t".to_owned(), "b".to_owned()));
        flow.begin(
            &offer(),
            &price(5),
            &balances(10),
            confirm,
            "id".into(),
            true,
        );
        assert!(flow.confirm("id".into(), false).is_none());
        assert_eq!(flow, PurchaseFlow::Done(PurchaseDialog::Disabled));
    }
}
