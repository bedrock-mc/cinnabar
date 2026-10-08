# Bare block stack identity and crafting admission

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Stack capacity | A descriptor-dependent maximum below two is not stackable; damaged damageable items have additional damage/Unbreakable conditions. |
| Occupied-stack compatibility | Compare item definition, conditional aux, structural user data, restriction hashes and the additional identity field. Nonzero aux and block identity are not blanket refusals. |
| Full-stack matching | Allow wildcard aux `0x7fff`; a present left-hand block identity must match the right-hand identity. |
| Recipe descriptor comparison | Compare item definition and aux/state identity when requested, permitting wildcard aux and resolved block states. |
| Stack-to-descriptor construction | Handle block-backed and wildcard block-type descriptors before item/aux fallback. |

Vanilla occupied-stack compatibility does not require zero aux or zero block
identity:

- Full-stack matching compares item definition, aux (either `0x7fff` is a
  wildcard), user data, both restriction hashes and one further identity field.
  A left-hand block identity, when present, must match the right-hand one;
  merely being present is not a rejection. Charged items have further checks
  outside this fix.
- Stack compatibility requires the same item definition and a stackable other
  stack. It compares aux when the item's variant flag requires it, then user
  data, the restriction hashes and the further identity field. It does not
  compare block identity.
- Stackability asks the item for its descriptor-dependent maximum stack size.
  A maximum below two is not stackable. Damaged damageable items have additional
  damage/Unbreakable conditions; a nonzero aux alone is not a universal refusal.
- User data compares present compound tags structurally. Missing user data and
  an empty compound tag can compare equal, which is broader than admitting only
  empty serialized data.

The restriction hashes are CanDestroy and CanPlaceOn. For ordinary occupied
stacks, count and server/sparse stack-network IDs are not semantic item
equality keys; they remain necessary for quantities and request authority.

## Recipe identity is a separate check

The crafting-input and recipe-select controllers call descriptor comparison with
aux checking enabled. Recipe selection separately uses full-stack matching
when merging items into an occupied grid slot.

The concrete descriptor comparison compares the item
definition ID. With aux checking enabled, either aux `0x7fff` accepts the
other aux; otherwise aux/state identity must match. Block-backed descriptors
can resolve block states before comparison.

Stack-to-descriptor construction explicitly handles a
present block identity, including a wildcard block-type descriptor, before
falling back to an item/aux descriptor. None of these ingredient descriptor
comparisons requires zero block identity or universally zero aux, nor do they
compare a full stack's serialized NBT. This does not imply every NBT-bearing
stack is supported by Cinnabar's crafting prediction.

## Observed failure and scoped correction

The offline loopback vanilla BDS baseline reproduced two local refusals:

- Taking 32 blocks received an Accepted response. Left-clicking to restack
  them then issued no request; this was client admission, not a rejected merge.
- A grid containing eight oak logs showed no crafting output.

Both paths shared the old `plain_stack` guard, which required aux and block
runtime identity to be zero as well as empty user data. Ordinary block stacks
carry nonzero block runtime identity, so they were incorrectly classified as
unsupported for both occupied-stack gestures and recipe matching.

`app/src/ui_runtime/inventory_ledger/registry.rs` now separates these concerns:

- `plain_stack` checks only the two already supported empty extra-data
  encodings (empty bytes or ten zero bytes) and their valid SHA-256 digest.
  Nonzero aux or block runtime identity does not make user data non-plain.
- `occupied_stack_relation` compares source/destination aux and block runtime
  identity for equality, permitting equal nonzero values. It retains negotiated
  identifier/capacity checks and sparse/server authority validation.
- Manual grid matching and auto-craft admission continue to check recipe item
  or tag identity and the ingredient's aux rule independently. The change
  admits bare block ingredients and bare nonzero-aux ingredients only where
  that separate recipe rule accepts them.

The existing no-meaningful-overlay guard on compatible merges remains in place.
Named, enchanted or otherwise nonempty user data remains unsupported for this
merge prediction path, even when vanilla structural comparison would accept it.
The scoped equality checks also do not reproduce vanilla variant-flag exceptions,
wildcard full-stack matching, descriptor-dependent capacity/damage behavior or
charged-item comparison. This is not a claim of complete inventory parity.

## Verification status

Regressions in `app/src/ui_runtime/inventory_ledger/merge_tests/blocks.rs`
cover accepted and still-pending block split/restack, preservation of nonzero
runtime identity, equal nonzero aux, refusal to merge different aux/runtime
identities, and the two supported empty user-data encodings. These are focused
contract tests, not a substitute for live server acceptance. Post-fix live
acceptance and final verification are recorded by the task owner separately.

The Accepted BDS split/restack requests and independent server quantity checks
are recorded in [the correction acceptance record](../reviews/inventory-hud-crafting-fixes.md).
