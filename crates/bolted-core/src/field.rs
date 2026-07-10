//! `Field` — the workhorse. Two INDEPENDENT dimensions: validity and sync.

use crate::report::ErrorData;
use crate::value::Value;

/// The validity dimension. Independent of [`SyncState`].
#[derive(Debug, Clone, PartialEq)]
pub enum Validity<V: Value> {
    /// No value entered yet (create flow, or a never-touched field).
    Unset,
    /// A valid, parsed value.
    Valid(V),
    /// The last input failed validation. The raw attempt is retained so the UI can keep showing
    /// it, and so submit is blocked — no silent fallback to a stale valid value (C06).
    Invalid { raw: V::Raw, error: V::Error },
}

/// The sync dimension. Independent of [`Validity`].
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState<V: Value> {
    /// The field rebases cleanly onto canonical.
    InSync,
    /// Canonical moved under a dirty edit. `theirs` is the incoming canonical value; *yours* is the
    /// field's own [`Validity`]; the common ancestor is [`Field::base`], which does not move while
    /// conflicted. Those three are enough for a 3-way merge UI — but field-level keep/take is the
    /// framework ceiling (no text/CRDT merge).
    ///
    /// The ancestor is deliberately *not* duplicated into this variant: while conflicted it is
    /// always exactly `Field::base()`, and two copies of one fact are two facts to keep consistent
    /// (ARCHITECTURE §8, step-01 friction F7).
    Conflicted { theirs: V },
}

/// One editable field of a draft: a [`Validity`] and a [`SyncState`] over a shared `base` value.
#[derive(Debug, Clone, PartialEq)]
pub struct Field<V: Value> {
    validity: Validity<V>,
    sync: SyncState<V>,
    base: Option<V>,
}

impl<V: Value> Field<V> {
    /// Create flow: no base, no value.
    pub fn new_unset() -> Self {
        Field {
            validity: Validity::Unset,
            sync: SyncState::InSync,
            base: None,
        }
    }

    /// Checkout of an existing entity's field: value == base, clean, in sync.
    pub fn from_base(base: V) -> Self {
        Field {
            validity: Validity::Valid(base.clone()),
            sync: SyncState::InSync,
            base: Some(base),
        }
    }

    /// Record an input attempt. ALWAYS records: `Ok` → `Valid(v)`, `Err` → `Invalid { raw, error }`
    /// (returning the verdict either way).
    ///
    /// A successful set that lands exactly on `theirs` **resolves the conflict** — adopt theirs as
    /// the base, clean, `InSync` (conformance C14). This is the same judgement C04 makes when the
    /// canonical change arrives second: two edits that agree are not a conflict, regardless of which
    /// one the store saw first. Leaving it conflicted made the running web shell show a "keep mine /
    /// take theirs" banner whose two buttons did visibly the same thing (step-04 F6 verdict).
    pub fn try_set(&mut self, raw: V::Raw) -> Result<(), V::Error> {
        match V::try_new(raw.clone()) {
            Ok(v) => {
                if let SyncState::Conflicted { theirs } = &self.sync
                    && *theirs == v
                {
                    self.base = Some(v.clone());
                    self.sync = SyncState::InSync;
                }
                self.validity = Validity::Valid(v);
                Ok(())
            }
            Err(error) => {
                let reported = error.clone();
                self.validity = Validity::Invalid { raw, error };
                Err(reported)
            }
        }
    }

    /// The current valid value, if any (`None` for `Unset`/`Invalid`).
    pub fn value(&self) -> Option<&V> {
        match &self.validity {
            Validity::Valid(v) => Some(v),
            _ => None,
        }
    }

    /// The agreed base (ancestor) value. `None` in create flow. While `Conflicted` this is the
    /// common ancestor of *yours* and *theirs*.
    pub fn base(&self) -> Option<&V> {
        self.base.as_ref()
    }

    pub fn validity(&self) -> &Validity<V> {
        &self.validity
    }

    pub fn sync(&self) -> &SyncState<V> {
        &self.sync
    }

    /// The incoming canonical value, iff this field is conflicted.
    pub fn theirs(&self) -> Option<&V> {
        match &self.sync {
            SyncState::Conflicted { theirs } => Some(theirs),
            SyncState::InSync => None,
        }
    }

    pub fn is_conflicted(&self) -> bool {
        matches!(self.sync, SyncState::Conflicted { .. })
    }

    /// This field's tier-1 error as shell-ready data, iff the last input was rejected. `Unset` is
    /// *not* an error here: whether an empty field is a failure is a field-level (`Required`)
    /// concern the entity layer owns, not a value-level one.
    ///
    /// Exists because `Value::Error: Into<ErrorData>` is now a trait bound — before that, every
    /// consumer wrote this three-line match behind a restated `where` clause of its own.
    pub fn invalid_error(&self) -> Option<ErrorData> {
        match &self.validity {
            Validity::Invalid { error, .. } => Some(error.clone().into()),
            _ => None,
        }
    }

    /// VALUE-based dirtiness: `Valid(v)` ⇔ `v != base`; `Invalid` ⇔ always; `Unset` ⇔ `base` set.
    /// Editing a field back to its base value makes it clean again (revert-for-free).
    pub fn is_dirty(&self) -> bool {
        match &self.validity {
            Validity::Valid(v) => self.base.as_ref() != Some(v),
            Validity::Invalid { .. } => true,
            Validity::Unset => self.base.is_some(),
        }
    }

    /// Keep your value; adopt theirs as the new base. Stays dirty (your value still differs from
    /// the new base), returns to `InSync`. No-op if not conflicted.
    pub fn resolve_keep_mine(&mut self) {
        if let SyncState::Conflicted { theirs } = &self.sync {
            let theirs = theirs.clone();
            self.base = Some(theirs);
            self.sync = SyncState::InSync;
        }
    }

    /// Take theirs as both value and base: clean, `InSync`. No-op if not conflicted.
    pub fn resolve_take_theirs(&mut self) {
        if let SyncState::Conflicted { theirs } = &self.sync {
            let theirs = theirs.clone();
            self.validity = Validity::Valid(theirs.clone());
            self.base = Some(theirs);
            self.sync = SyncState::InSync;
        }
    }

    /// Rebase this field onto a new canonical value `theirs`:
    /// - already `Conflicted` → update `theirs` only (the recorded ancestor, `base`, does not move);
    /// - not dirty → adopt theirs (value + base), `InSync`;
    /// - dirty and `value == theirs` → convergent: adopt as base, clean, `InSync`;
    /// - dirty otherwise → `Conflicted { theirs }`, your value preserved.
    pub fn rebase(&mut self, theirs: V) {
        if self.is_conflicted() {
            self.sync = SyncState::Conflicted { theirs };
            return;
        }

        if !self.is_dirty() {
            self.validity = Validity::Valid(theirs.clone());
            self.base = Some(theirs);
            self.sync = SyncState::InSync;
        } else if self.value() == Some(&theirs) {
            self.base = Some(theirs);
            self.sync = SyncState::InSync;
        } else {
            self.sync = SyncState::Conflicted { theirs };
        }
    }

    /// Consume the field, yielding its valid value if any. Used by `commit` to move values out.
    pub fn into_valid(self) -> Option<V> {
        match self.validity {
            Validity::Valid(v) => Some(v),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::Constraint;

    #[derive(Debug, Clone, PartialEq)]
    struct Toy(i32);
    #[derive(Debug, Clone, PartialEq)]
    struct ToyErr;

    impl From<ToyErr> for ErrorData {
        fn from(_: ToyErr) -> Self {
            ErrorData::new("toy_negative")
        }
    }

    impl Value for Toy {
        type Raw = i32;
        type Error = ToyErr;
        fn try_new(raw: i32) -> Result<Self, ToyErr> {
            if raw >= 0 { Ok(Toy(raw)) } else { Err(ToyErr) }
        }
        fn into_raw(self) -> i32 {
            self.0
        }
        fn constraints() -> &'static [Constraint] {
            &[]
        }
    }

    #[test]
    fn dirty_is_value_based() {
        let mut f = Field::from_base(Toy(1));
        assert!(!f.is_dirty());
        f.try_set(2).expect("valid");
        assert!(f.is_dirty());
        f.try_set(1).expect("valid"); // revert to base
        assert!(!f.is_dirty());
        assert!(f.try_set(-1).is_err()); // invalid is recorded...
        assert!(matches!(f.validity(), Validity::Invalid { .. }));
        assert!(f.is_dirty()); // ...and invalid is always dirty
    }

    #[test]
    fn rebase_conflict_then_resolution() {
        let mut f = Field::from_base(Toy(1));
        f.try_set(2).expect("valid");
        f.rebase(Toy(3)); // dirty, yours(2) != theirs(3) -> conflict
        assert!(f.is_conflicted());
        assert_eq!(f.value(), Some(&Toy(2))); // yours preserved
        assert_eq!(f.base(), Some(&Toy(1))); // the ancestor does not move
        assert_eq!(f.theirs(), Some(&Toy(3)));

        f.resolve_take_theirs();
        assert_eq!(f.value(), Some(&Toy(3)));
        assert!(!f.is_dirty());
        assert!(matches!(f.sync(), SyncState::InSync));
    }

    #[test]
    fn convergent_rebase_is_clean() {
        let mut f = Field::from_base(Toy(1));
        f.try_set(5).expect("valid");
        f.rebase(Toy(5)); // yours == theirs
        assert!(!f.is_dirty());
        assert!(matches!(f.sync(), SyncState::InSync));
    }

    /// C14: the mirror image of `convergent_rebase_is_clean` — the edit arrives second.
    #[test]
    fn editing_to_theirs_auto_converges() {
        let mut f = Field::from_base(Toy(1));
        f.try_set(2).expect("valid");
        f.rebase(Toy(3)); // conflict
        assert!(f.is_conflicted());

        f.try_set(3).expect("valid"); // type their value
        assert!(matches!(f.sync(), SyncState::InSync));
        assert!(!f.is_dirty());
        assert_eq!(f.base(), Some(&Toy(3)));
    }

    /// An edit that does not land on `theirs` leaves the conflict standing (C03).
    #[test]
    fn editing_to_a_third_value_stays_conflicted() {
        let mut f = Field::from_base(Toy(1));
        f.try_set(2).expect("valid");
        f.rebase(Toy(3));
        f.try_set(4).expect("valid");
        assert!(f.is_conflicted());
        assert_eq!(f.value(), Some(&Toy(4)));
    }

    #[test]
    fn invalid_error_projects_to_data() {
        let mut f = Field::from_base(Toy(1));
        assert_eq!(f.invalid_error(), None);
        assert!(f.try_set(-1).is_err());
        assert_eq!(f.invalid_error(), Some(ErrorData::new("toy_negative")));
    }
}
