//! The purchase flow as the player sees it: an affordability check, an optional confirmation, a
//! progress modal, then a result. Only a confirmed purchase is ever handed to the core, and a second
//! press while one is running is ignored.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use protocol::store_control::{
    ConfirmedPurchase, PendingPurchase, PurchaseOutcome, PurchaseStatus, StoreBalance, StoreOffer,
    StorePrice,
};
use sha2::{Digest, Sha256};

use super::worker::StoreError;

/// A modal or toast the flow wants shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurchaseDialog {
    /// "You just bought: %s".
    Success { title: String },
    /// The balance cannot cover the price; offers the coin top-up.
    InsufficientFunds { missing: i64 },
    /// The service refused the shown price.
    PriceMismatch,
    /// No definitive answer yet; balance and entitlements must be re-read before trying again.
    Pending,
    /// The service refused the purchase; codes help support find the attempt.
    Failed {
        marketplace_error_code: Option<u32>,
        correlation_id: Option<String>,
    },
    /// The account is no longer signed in.
    SignedOut,
    /// Purchases are switched off until the owner verifies them; nothing was sent.
    Disabled,
}

/// Shown when a confirmed purchase is stopped by the `store_purchases_enabled` setting; not a
/// vanilla string.
pub const DISABLED_TITLE: &str = "Purchases disabled";
pub const DISABLED_BODY: &str = "Purchases disabled until verified";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurchaseFlow {
    Idle,
    /// Waiting for the player to confirm (bundles ask first); the texts are already localized.
    Confirming {
        pending: PendingPurchase,
        title: String,
        body: String,
        offer_title: String,
    },
    InProgress {
        purchase_id: String,
        offer_title: String,
    },
    Done(PurchaseDialog),
}

/// What pressing the purchase button did.
#[derive(Debug, PartialEq, Eq)]
pub enum Begin {
    /// Ignored: a purchase is already active or the price is not payable.
    Ignored,
    /// A confirmation or result modal is now showing.
    Showing,
    /// Send this to the core.
    Send(ConfirmedPurchase),
}

/// Whether `balances` cover `amount` of `currency`; `None` when that balance is unknown.
pub fn balance_covers(balances: &[StoreBalance], currency: &str, amount: i64) -> Option<bool> {
    balances
        .iter()
        .find(|balance| balance.currency == currency)
        .map(|balance| balance.amount >= amount)
}

impl PurchaseFlow {
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// The player pressed the purchase button for `price`. `confirm` carries the bundle confirmation
    /// texts (`title`, `body`) when vanilla asks first; otherwise the press itself is the confirmation.
    pub fn begin(
        &mut self,
        offer: &StoreOffer,
        price: &StorePrice,
        balances: &[StoreBalance],
        confirm: Option<(String, String)>,
        purchase_id: String,
        purchases_enabled: bool,
    ) -> Begin {
        if !self.is_idle() {
            return Begin::Ignored;
        }
        let Some(pending) = PendingPurchase::for_offer(offer, price) else {
            return Begin::Ignored;
        };
        match balance_covers(balances, &price.currency, price.amount) {
            Some(true) => {}
            Some(false) => {
                let have = balances
                    .iter()
                    .find(|balance| balance.currency == price.currency)
                    .map_or(0, |balance| balance.amount);
                *self = Self::Done(PurchaseDialog::InsufficientFunds {
                    missing: price.amount - have,
                });
                return Begin::Showing;
            }
            None => {
                *self = Self::Done(PurchaseDialog::Failed {
                    marketplace_error_code: None,
                    correlation_id: None,
                });
                return Begin::Showing;
            }
        }
        if let Some((title, body)) = confirm {
            *self = Self::Confirming {
                pending,
                title,
                body,
                offer_title: offer.title.clone(),
            };
            return Begin::Showing;
        }
        if !purchases_enabled {
            *self = Self::Done(PurchaseDialog::Disabled);
            return Begin::Showing;
        }
        Begin::Send(self.start(pending, offer.title.clone(), purchase_id))
    }

    /// The player confirmed the modal; returns the purchase to send, or `None` when nothing is
    /// awaiting confirmation or purchases are disabled.
    pub fn confirm(
        &mut self,
        purchase_id: String,
        purchases_enabled: bool,
    ) -> Option<ConfirmedPurchase> {
        let Self::Confirming {
            pending,
            offer_title,
            ..
        } = std::mem::replace(self, Self::Idle)
        else {
            return None;
        };
        if !purchases_enabled {
            *self = Self::Done(PurchaseDialog::Disabled);
            return None;
        }
        Some(self.start(pending, offer_title, purchase_id))
    }

    fn start(
        &mut self,
        pending: PendingPurchase,
        offer_title: String,
        purchase_id: String,
    ) -> ConfirmedPurchase {
        let confirmed = pending.confirm(purchase_id.clone());
        *self = Self::InProgress {
            purchase_id,
            offer_title,
        };
        confirmed
    }

    /// The player backed out of a confirmation or dismissed a result.
    pub fn dismiss(&mut self) {
        if matches!(self, Self::Confirming { .. } | Self::Done(_)) {
            *self = Self::Idle;
        }
    }

    /// The core answered `purchase_id`; answers for any other attempt are dropped.
    pub fn finish(&mut self, purchase_id: &str, result: Result<PurchaseOutcome, StoreError>) {
        let Self::InProgress {
            purchase_id: active,
            offer_title,
        } = self
        else {
            return;
        };
        if active.as_str() != purchase_id {
            return;
        }
        let dialog = match result {
            Ok(outcome) => match outcome.status {
                PurchaseStatus::Purchased => PurchaseDialog::Success {
                    title: std::mem::take(offer_title),
                },
                PurchaseStatus::PriceMismatch => PurchaseDialog::PriceMismatch,
                PurchaseStatus::Unknown => PurchaseDialog::Pending,
                PurchaseStatus::PreconditionFailed | PurchaseStatus::Failed => {
                    PurchaseDialog::Failed {
                        marketplace_error_code: Some(outcome.marketplace_error_code)
                            .filter(|code| *code != 0),
                        correlation_id: Some(outcome.correlation_id).filter(|id| !id.is_empty()),
                    }
                }
            },
            Err(StoreError::SignedOut) => PurchaseDialog::SignedOut,
            Err(StoreError::Busy) => PurchaseDialog::Pending,
            Err(_) => PurchaseDialog::Failed {
                marketplace_error_code: None,
                correlation_id: None,
            },
        };
        *self = Self::Done(dialog);
    }
}

static PURCHASE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh idempotency key: 32 hex characters, unique per call within and across runs.
pub fn new_purchase_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let count = PURCHASE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(count.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher
        .finalize()
        .iter()
        .take(16)
        .fold(String::with_capacity(32), |mut out, byte| {
            out.push_str(&format!("{byte:02x}"));
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_covered_price_sends_once_and_ignores_a_second_press() {
        let mut flow = PurchaseFlow::Idle;
        let sent = flow.begin(
            &offer(),
            &price(320),
            &balances(500),
            None,
            "id-1".into(),
            true,
        );
        let Begin::Send(purchase) = sent else {
            panic!("expected a send, got {sent:?}");
        };
        assert_eq!(purchase.purchase_id(), "id-1");
        assert_eq!(
            flow.begin(
                &offer(),
                &price(320),
                &balances(500),
                None,
                "id-2".into(),
                true
            ),
            Begin::Ignored
        );
        assert!(matches!(flow, PurchaseFlow::InProgress { .. }));
    }

    #[test]
    fn an_uncovered_or_unknown_balance_never_sends() {
        let mut flow = PurchaseFlow::Idle;
        assert_eq!(
            flow.begin(
                &offer(),
                &price(320),
                &balances(100),
                None,
                "id".into(),
                true
            ),
            Begin::Showing
        );
        assert_eq!(
            flow,
            PurchaseFlow::Done(PurchaseDialog::InsufficientFunds { missing: 220 })
        );
        flow.dismiss();
        assert_eq!(
            flow.begin(&offer(), &price(320), &[], None, "id".into(), true),
            Begin::Showing
        );
        assert!(matches!(
            flow,
            PurchaseFlow::Done(PurchaseDialog::Failed { .. })
        ));
        flow.dismiss();
        assert_eq!(
            flow.begin(&offer(), &price(0), &balances(9), None, "id".into(), true),
            Begin::Ignored
        );
    }

    #[test]
    fn purchase_ids_are_unique_hex() {
        let a = new_purchase_id();
        let b = new_purchase_id();
        assert_ne!(a, b);
        assert!(a.len() == 32 && a.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
