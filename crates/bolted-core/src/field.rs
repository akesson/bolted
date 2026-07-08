//! `Field` — the workhorse. Two INDEPENDENT dimensions: validity and sync.

use crate::value::Value;

/// The validity dimension. Independent of [`SyncState`].
#[derive(Debug, Clone, PartialEq)]
pub enum Validity<V: Value> {
    /// No value entered yet (create flow, or a never-touched field).
    Unset,
    /// A valid, parsed value.
    Valid(V),
    /// The last input failed validation. The raw attempt is retained so the UI can keep showing
    /// it, and so submit is blocked — no silent fallback to a stale valid value (invariant I6).
    Invalid { raw: V::Raw, error: V::Error },
}

/// The sync dimension. Independent of [`Validity`].
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState<V: Value> {
    /// The field rebases cleanly onto canonical.
    InSync,
    /// Canonical moved under a dirty edit. `base` is the common ancestor and `theirs` the incoming
    /// canonical value; *yours* is the field's own [`Validity`]. Together enough for a 3-way merge
    /// UI — but field-level keep/take is the framework ceiling (no text/CRDT merge).
    Conflicted { base: Option<V>, theirs: V },
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
    /// (returning the verdict either way). Does not touch the sync dimension.
    pub fn try_set(&mut self, raw: V::Raw) -> Result<(), V::Error> {
        match V::try_new(raw.clone()) {
            Ok(v) => {
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

    /// The agreed base (ancestor) value. `None` in create flow.
    pub fn base(&self) -> Option<&V> {
        self.base.as_ref()
    }

    pub fn validity(&self) -> &Validity<V> {
        &self.validity
    }

    pub fn sync(&self) -> &SyncState<V> {
        &self.sync
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
        if let SyncState::Conflicted { theirs, .. } = &self.sync {
            let theirs = theirs.clone();
            self.base = Some(theirs);
            self.sync = SyncState::InSync;
        }
    }

    /// Take theirs as both value and base: clean, `InSync`. No-op if not conflicted.
    pub fn resolve_take_theirs(&mut self) {
        if let SyncState::Conflicted { theirs, .. } = &self.sync {
            let theirs = theirs.clone();
            self.validity = Validity::Valid(theirs.clone());
            self.base = Some(theirs);
            self.sync = SyncState::InSync;
        }
    }

    /// Rebase this field onto a new canonical value `theirs`:
    /// - already `Conflicted` → update `theirs` only (keep the recorded ancestor);
    /// - not dirty → adopt theirs (value + base), `InSync`;
    /// - dirty and `value == theirs` → convergent: adopt as base, clean, `InSync`;
    /// - dirty otherwise → `Conflicted { base: <old base>, theirs }`, your value preserved.
    pub fn rebase(&mut self, theirs: V) {
        if let SyncState::Conflicted { base, .. } = &self.sync {
            let base = base.clone();
            self.sync = SyncState::Conflicted { base, theirs };
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
            self.sync = SyncState::Conflicted {
                base: self.base.clone(),
                theirs,
            };
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
        assert!(matches!(f.sync(), SyncState::Conflicted { .. }));
        assert_eq!(f.value(), Some(&Toy(2))); // yours preserved

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
}
