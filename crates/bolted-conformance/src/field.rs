//! The field tier: C02–C06, C09, C14, C19, C20 — everything that is a claim about `Field<V>`.
//!
//! None of these needs a feature, a draft or a store. They are generic over `V: ValueFixture` and
//! therefore run once per value type. The spike's four value types now exercise nine invariants
//! each; before the extraction, one type exercised them once.

use crate::check;
use crate::value::{RawOf, ValueFixture, ValueOf, parse};
use bolted_core::{Field, SyncState, Validity, Value};
use proptest::prelude::*;

/// C02 — a non-dirty field must adopt `theirs` on rebase and stay `InSync`.
pub fn c02_a_clean_field_follows_canonical<F: ValueFixture>() {
    check((F::valid_raw(), F::valid_raw()), |(base, theirs)| {
        let base = parse::<F>(base)?;
        let theirs = parse::<F>(theirs)?;

        let mut f = Field::from_base(base);
        prop_assert!(!f.is_dirty());
        f.rebase(theirs.clone());
        prop_assert!(f.value() == Some(&theirs));
        prop_assert!(matches!(f.sync(), SyncState::InSync));
        prop_assert!(!f.is_dirty());
        Ok(())
    });
}

/// C03 — rebase over a dirty field **whose canonical value moved** must preserve your value, enter
/// `Conflicted { theirs }`, and leave the recorded ancestor where it was.
///
/// `theirs != base` is the precondition this property was missing until step 07. Note that all three
/// assumptions compare **values**, not raws: `"  alice "` and `"alice"` are different raws and the
/// same `Username`, and a property that assumed on raws would sample a case where `mine == base` as
/// values while believing the field to be dirty.
pub fn c03_a_dirty_field_is_never_silently_overwritten<F: ValueFixture>() {
    check(
        (F::valid_raw(), F::valid_raw(), F::valid_raw()),
        |(base, mine, theirs)| {
            let base = parse::<F>(base)?;
            let mine = parse::<F>(mine)?;
            let theirs = parse::<F>(theirs)?;
            prop_assume!(mine != base);
            prop_assume!(theirs != mine);
            prop_assume!(theirs != base);

            let mut f = Field::from_base(base.clone());
            prop_assert!(f.try_set(mine.clone().into_raw()).is_ok());
            prop_assert!(f.is_dirty());
            f.rebase(theirs.clone());

            prop_assert!(f.value() == Some(&mine)); // yours preserved
            prop_assert!(f.base() == Some(&base)); // the ancestor is still the ancestor
            prop_assert!(f.theirs() == Some(&theirs));
            prop_assert!(f.is_conflicted());
            Ok(())
        },
    );
}

/// C04 — if a dirty field's value already equals `theirs`, rebase adopts it as the base and lands
/// clean and `InSync`. Two edits that agree are not a conflict.
pub fn c04_convergent_rebase_is_clean<F: ValueFixture>() {
    check((F::valid_raw(), F::valid_raw()), |(base, edit)| {
        let base = parse::<F>(base)?;
        let edit = parse::<F>(edit)?;
        prop_assume!(edit != base);

        let mut f = Field::from_base(base);
        prop_assert!(f.try_set(edit.clone().into_raw()).is_ok());
        prop_assert!(f.is_dirty());
        f.rebase(edit.clone()); // theirs == yours

        prop_assert!(!f.is_dirty());
        prop_assert!(matches!(f.sync(), SyncState::InSync));
        prop_assert!(f.value() == Some(&edit));
        prop_assert!(f.base() == Some(&edit));
        Ok(())
    });
}

/// C05 — setting a field back to its base value clears dirty. Dirtiness is a pure function of the
/// data, never of touch history.
pub fn c05_revert_clears_dirty<F: ValueFixture>() {
    check((F::valid_raw(), F::valid_raw()), |(base, edit)| {
        let base = parse::<F>(base)?;
        let edit = parse::<F>(edit)?;
        prop_assume!(edit != base);

        let mut f = Field::from_base(base.clone());
        prop_assert!(!f.is_dirty());
        prop_assert!(f.try_set(edit.into_raw()).is_ok());
        prop_assert!(f.is_dirty());
        prop_assert!(f.try_set(base.clone().into_raw()).is_ok());
        prop_assert!(!f.is_dirty());
        prop_assert!(f.value() == Some(&base));
        Ok(())
    });
}

/// C06, field half — a failed `try_set` is recorded as `Invalid { raw, error }`, and the previous
/// valid value does not survive as the field's value. (Its half about *submit* is C06 in the feature
/// tier: a `Field` cannot refuse a commit.)
pub fn c06_a_failed_set_is_recorded_as_invalid<F: ValueFixture>() {
    check(F::valid_raw(), |base| {
        let base = parse::<F>(base)?;
        let bad: RawOf<F> = F::invalid_raw();

        let mut f = Field::from_base(base);
        prop_assert!(f.try_set(bad.clone()).is_err());
        match f.validity() {
            Validity::Invalid { raw, .. } => prop_assert_eq!(raw, &bad),
            other => {
                return Err(proptest::test_runner::TestCaseError::fail(format!(
                    "invalid_raw() must be rejected, got {other:?}"
                )));
            }
        }
        prop_assert!(f.value().is_none(), "no stale valid value survives");
        prop_assert!(f.invalid_error().is_some());
        prop_assert!(f.is_dirty(), "an invalid field is always dirty");
        Ok(())
    });
}

/// C09 — `resolve_keep_mine`: value stays yours, base becomes theirs, dirty, `InSync`.
/// `resolve_take_theirs`: value and base become theirs, clean, `InSync`.
pub fn c09_resolution_semantics<F: ValueFixture>() {
    check(
        (F::valid_raw(), F::valid_raw(), F::valid_raw()),
        |(base, mine, theirs)| {
            let base = parse::<F>(base)?;
            let mine = parse::<F>(mine)?;
            let theirs = parse::<F>(theirs)?;
            prop_assume!(mine != base && theirs != mine && theirs != base);

            let mut keep = Field::from_base(base.clone());
            prop_assert!(keep.try_set(mine.clone().into_raw()).is_ok());
            keep.rebase(theirs.clone());
            prop_assert!(keep.is_conflicted());
            keep.resolve_keep_mine();
            prop_assert!(keep.value() == Some(&mine));
            prop_assert!(keep.base() == Some(&theirs));
            prop_assert!(keep.is_dirty());
            prop_assert!(matches!(keep.sync(), SyncState::InSync));

            let mut take = Field::from_base(base);
            prop_assert!(take.try_set(mine.into_raw()).is_ok());
            take.rebase(theirs.clone());
            take.resolve_take_theirs();
            prop_assert!(take.value() == Some(&theirs));
            prop_assert!(take.base() == Some(&theirs));
            prop_assert!(!take.is_dirty());
            prop_assert!(matches!(take.sync(), SyncState::InSync));
            Ok(())
        },
    );
}

/// C14 — editing a conflicted field to a value equal to `theirs` resolves the conflict. This is C04
/// with the two events in the other order, and it must reach the same state.
pub fn c14_editing_to_theirs_auto_converges<F: ValueFixture>() {
    check(
        (F::valid_raw(), F::valid_raw(), F::valid_raw()),
        |(base, mine, theirs)| {
            let base = parse::<F>(base)?;
            let mine = parse::<F>(mine)?;
            let theirs = parse::<F>(theirs)?;
            prop_assume!(mine != base && theirs != mine && theirs != base);

            // the C04 state, reached the other way round
            let mut converged = Field::from_base(base.clone());
            prop_assert!(converged.try_set(theirs.clone().into_raw()).is_ok());
            converged.rebase(theirs.clone());

            let mut f = Field::from_base(base);
            prop_assert!(f.try_set(mine.into_raw()).is_ok());
            f.rebase(theirs.clone());
            prop_assert!(f.is_conflicted());
            prop_assert!(f.try_set(theirs.clone().into_raw()).is_ok());

            prop_assert!(!f.is_conflicted());
            prop_assert!(!f.is_dirty());
            prop_assert!(f.value() == Some(&theirs));
            prop_assert!(f.base() == Some(&theirs));
            prop_assert!(f == converged, "C04 and C14 must reach the same state");
            Ok(())
        },
    );
}

/// C19 — rebase is a three-way merge, and idempotent. A field whose incoming canonical equals its
/// recorded ancestor must not be conflicted, whatever its dirty state. A canonical that moves *back*
/// to the ancestor must clear an existing conflict. Rebasing twice equals rebasing once.
pub fn c19_rebase_is_a_three_way_merge_and_idempotent<F: ValueFixture>() {
    check(
        (F::valid_raw(), F::valid_raw(), F::valid_raw()),
        |(base, mine, theirs)| {
            let base = parse::<F>(base)?;
            let mine = parse::<F>(mine)?;
            let theirs = parse::<F>(theirs)?;
            prop_assume!(mine != base && theirs != mine && theirs != base);

            let mut f = Field::from_base(base.clone());
            prop_assert!(f.try_set(mine.clone().into_raw()).is_ok());
            prop_assert!(f.is_dirty());

            // nobody else moved this field: rebasing onto its own ancestor must not conflict it
            f.rebase(base.clone());
            prop_assert!(!f.is_conflicted());
            prop_assert!(f.value() == Some(&mine));
            prop_assert!(f.base() == Some(&base));

            // a real conflict, and rebasing onto the same canonical again changes nothing at all
            f.rebase(theirs.clone());
            prop_assert!(f.is_conflicted());
            let once = f.clone();
            f.rebase(theirs.clone());
            prop_assert!(f == once, "rebase must be idempotent");

            // the other side stopped disagreeing: canonical returns to the ancestor
            f.rebase(base.clone());
            prop_assert!(!f.is_conflicted());
            prop_assert!(f.value() == Some(&mine));
            prop_assert!(f.base() == Some(&base));
            prop_assert!(f.is_dirty());

            // and a clean field rebased onto its own ancestor is a no-op too
            let clean = Field::from_base(base.clone());
            let mut c2 = clean.clone();
            c2.rebase(base);
            prop_assert!(c2 == clean);
            Ok(())
        },
    );
}

/// C20, field half — the stash carries each field's last input attempt and its ancestor, both raw,
/// and restoring reproduces value, ancestor, validity (including `Invalid { raw }`) and dirtiness.
/// It must **not** carry `sync`.
pub fn c20_a_field_stashes_to_raw_and_restores<F: ValueFixture>() {
    check(
        (F::valid_raw(), F::valid_raw(), F::valid_raw()),
        |(base, mine, theirs)| {
            let base = parse::<F>(base)?;
            let mine = parse::<F>(mine)?;
            let theirs = parse::<F>(theirs)?;
            prop_assume!(mine != base && theirs != mine && theirs != base);

            // a dirty, valid field: value and ancestor both intact
            let mut dirty = Field::from_base(base.clone());
            prop_assert!(dirty.try_set(mine.clone().into_raw()).is_ok());
            let restored: Field<ValueOf<F>> = Field::from_stash(&dirty.stash());
            prop_assert!(restored.value() == Some(&mine));
            prop_assert!(restored.base() == Some(&base));
            prop_assert!(restored.is_dirty());

            // an invalid field: the user's rejected text is still the user's rejected text
            let mut invalid = Field::from_base(base.clone());
            let _ = invalid.try_set(F::invalid_raw());
            let restored: Field<ValueOf<F>> = Field::from_stash(&invalid.stash());
            match restored.validity() {
                Validity::Invalid { raw, .. } => prop_assert_eq!(raw, &F::invalid_raw()),
                other => {
                    return Err(proptest::test_runner::TestCaseError::fail(format!(
                        "the raw attempt must survive the stash, got {other:?}"
                    )));
                }
            }
            prop_assert!(restored.is_dirty());

            // a clean field stays clean
            let clean: Field<ValueOf<F>> =
                Field::from_stash(&Field::from_base(base.clone()).stash());
            prop_assert!(!clean.is_dirty());
            prop_assert!(clean.value() == Some(&base));

            // `sync` is NOT stashed: `theirs` names a canonical the server may no longer hold
            let mut conflicted = Field::from_base(base.clone());
            prop_assert!(conflicted.try_set(mine.clone().into_raw()).is_ok());
            conflicted.rebase(theirs);
            prop_assert!(conflicted.is_conflicted());
            let restored: Field<ValueOf<F>> = Field::from_stash(&conflicted.stash());
            prop_assert!(!restored.is_conflicted());
            prop_assert!(restored.theirs().is_none());
            prop_assert!(restored.value() == Some(&mine));
            prop_assert!(
                restored.base() == Some(&base),
                "the ancestor carries resolutions"
            );
            Ok(())
        },
    );
}
