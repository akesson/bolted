//! The exported classes: the store, the draft, and the discipline that keeps them safe.
//!
//! **The wrapper invariant, generated rather than remembered:** never emit a stream event or invoke a
//! foreign callback while holding the `Mutex`. Every mutation locks, mutates, builds the snapshot,
//! drops the lock, then pushes. `Store::apply_canonical` and `Store::submit` return the ids they moved
//! *as data* (D16), so the emit list is assembled under the lock and flushed after it — a property of
//! the type signature, not of anyone's care.
//!
//! Since step 09 this crate does not re-derive a single judgement. `commit_gates`, `Field::required_error`
//! and `SingleFlight::violation` live in `bolted-core`; the wrapper calls `draft.validate()` and
//! projects what it gets.

use crate::dto::checker_trait_name;
use crate::field::FieldProj;
use bolted_decl::Feature;
use bolted_decl::naming::suffixed;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

pub fn plumbing(feature: &Feature, fields: &[FieldProj<'_>]) -> TokenStream2 {
    let entity = &feature.entity.name;
    let snap = suffixed(entity, "Snapshot");
    let draft_ty = suffixed(entity, "Draft");
    let store_ty = suffixed(entity, "Store");
    let stash_ffi = suffixed(entity, "StashFfi");
    let core_stash = suffixed(entity, "Stash");
    let values = suffixed(entity, "Values");
    let field_id = suffixed(entity, "FieldId");
    let core_field = suffixed(entity, "Field");
    let check_id = suffixed(entity, "Check");

    let d = quote!(draft);
    let states = fields.iter().map(|p| {
        let (ident, expr) = (p.ident(), p.state_expr(&d));
        quote!(#ident: #expr)
    });
    let check_states = fields.iter().filter_map(|p| {
        let check = p.field.check.as_ref()?;
        let (slot, variant) = (format_ident!("{}_check", p.ident()), &check.variant);
        Some(quote!(#slot: bolted_ffi::check_state(draft.check_state(#check_id::#variant))))
    });

    let s = quote!(s);
    let stash_fields = fields.iter().map(|p| {
        let (ident, expr) = (p.ident(), p.stash_expr(&s));
        quote!(#ident: #expr)
    });
    let from_stash_fields = fields.iter().map(|p| {
        let (ident, expr) = (p.ident(), p.core_stash_expr(&s));
        quote!(#ident: #expr)
    });

    // `build_entity`: every field parsed through its real value type, errors collected per field.
    //
    // `v` is taken **by value** and its fields moved out. A uniform `.clone()` here would be free for
    // a `String` raw and a `clippy::clone_on_copy` error for a `Copy` wire type like `AvailabilityRaw`
    // — the same trap D8 names for value objects, one layer down. Not cloning is also just correct.
    let parse_lets = fields.iter().map(|p| {
        let ident = p.ident();
        let ty = &p.field.ty;
        let raw = p.to_core_raw(quote!(v.#ident));
        quote!(let #ident = <#ty as ::bolted_core::Value>::try_new(#raw);)
    });
    let ok_idents = fields.iter().map(|p| p.ident());
    let ok_binds = fields.iter().map(|p| p.ident());
    let push_errs = fields.iter().map(|p| {
        let (ident, variant) = (p.ident(), &p.field.variant);
        quote! {
            if let Err(e) = #ident {
                field_errors.push(FieldErrorFfi {
                    field: #field_id::#variant,
                    error: bolted_ffi::error_data(e),
                });
            }
        }
    });
    let all_idents: Vec<_> = fields.iter().map(|p| p.ident()).collect();

    quote! {
        // -----------------------------------------------------------------------------------------
        // Poison-safe locking (no `unwrap`/`expect`/`panic!` in library code)
        // -----------------------------------------------------------------------------------------

        fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
            m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        /// A batch of stream emissions to flush AFTER the store lock is dropped.
        type Emits = Vec<(Arc<StreamProducer<#snap>>, #snap)>;

        fn flush(emits: Emits) {
            for (producer, snapshot) in emits {
                producer.push(snapshot);
            }
        }

        /// Everything the one `Mutex` protects. Canonical, versions, the draft registry, the rebase
        /// bookkeeping and the submit path all live in `store` — none of it is written here (D16).
        struct FfiState {
            store: #store_ty,
            producers: BTreeMap<DraftId, Arc<StreamProducer<#snap>>>,
            store_producer: Arc<StreamProducer<#snap>>,
        }

        /// Give a freshly registered draft its own snapshot stream.
        fn register_producer(
            g: &mut MutexGuard<'_, FfiState>,
            id: DraftId,
        ) -> Arc<StreamProducer<#snap>> {
            let producer = Arc::new(StreamProducer::new(256));
            g.producers.insert(id, producer.clone());
            producer
        }

        /// One snapshot per draft the core just moved, ready to flush **after** the lock is dropped.
        fn draft_emits(g: &MutexGuard<'_, FfiState>, ids: &[DraftId]) -> Emits {
            ids.iter()
                .filter_map(|id| {
                    let draft = g.store.draft(*id)?;
                    let producer = g.producers.get(id)?;
                    Some((producer.clone(), build_draft_snapshot(draft)))
                })
                .collect()
        }

        fn build_draft_snapshot(draft: &#draft_ty) -> #snap {
            #snap {
                #(#states,)*
                #(#check_states,)*
                any_dirty: !draft.dirty_fields().is_empty(),
                conflicts: draft.conflicts().into_iter().map(to_field_id).collect(),
                status: bolted_ffi::draft_status(draft.status()),
                version: draft.base_version(),
            }
        }

        /// The canonical entity as a snapshot.
        ///
        /// A draft checked out of a canonical has exactly the shape a canonical snapshot needs: every
        /// field `Valid` and `InSync`, nothing dirty, no conflicts, no check run. So it is built by
        /// asking the core, rather than by a second hand-written table that could disagree with the
        /// first. `spike-profile-ffi` wrote that table by hand, per field, twice.
        fn canonical_snapshot(entity: &#entity, version: u64) -> #snap {
            build_draft_snapshot(&#draft_ty::from_canonical(Some(entity), version))
        }

        /// An all-unset snapshot: what a tombstoned draft observes (C17, C18).
        fn tombstone_snapshot() -> #snap {
            build_draft_snapshot(&#draft_ty::from_canonical(None, 0))
        }

        fn to_stash_ffi(s: &#core_stash) -> #stash_ffi {
            #stash_ffi {
                #(#stash_fields,)*
                base_version: s.base_version,
                orphaned: s.orphaned,
            }
        }

        fn to_core_stash(s: &#stash_ffi) -> #core_stash {
            #core_stash {
                #(#from_stash_fields,)*
                base_version: s.base_version,
                orphaned: s.orphaned,
            }
        }

        /// The core's report is keyed by the *core* field id; the wire's is keyed by the `#[data]`
        /// one. They are different types on purpose: only one of them can cross.
        fn project_report(r: &::bolted_core::ValidationReport<#core_field>) -> ValidationReportFfi {
            ValidationReportFfi {
                field_errors: r
                    .field_errors
                    .iter()
                    .map(|(field, error)| FieldErrorFfi {
                        field: to_field_id(*field),
                        error: ErrorData::from(error.clone()),
                    })
                    .collect(),
                rule_errors: r
                    .rule_errors
                    .iter()
                    .map(|v| RuleViolationFfi {
                        rule: v.rule.to_string(),
                        pins: v.pins.iter().map(|f| to_field_id(*f)).collect(),
                        error: ErrorData::from(v.error.clone()),
                    })
                    .collect(),
            }
        }

        fn submit_error_to_dto(e: SubmitError<#core_field>) -> SubmitErrorFfi {
            match e {
                SubmitError::Validation(report) => SubmitErrorFfi::Validation {
                    report: project_report(&report),
                },
                SubmitError::Conflicted { fields } => SubmitErrorFfi::Conflicted {
                    fields: fields.into_iter().map(to_field_id).collect(),
                },
                SubmitError::Orphaned => SubmitErrorFfi::Orphaned,
                SubmitError::AlreadySubmitted => SubmitErrorFfi::AlreadySubmitted,
            }
        }

        fn build_entity(v: #values) -> ::core::result::Result<#entity, ValidationReportFfi> {
            #(#parse_lets)*

            match (#(#all_idents),*) {
                (#(Ok(#ok_binds)),*) => Ok(#entity { #(#ok_idents),* }),
                (#(#all_idents),*) => {
                    let mut field_errors = Vec::new();
                    #(#push_errs)*
                    Err(ValidationReportFfi { field_errors, rule_errors: Vec::new() })
                }
            }
        }
    }
}

pub fn store_class(feature: &Feature) -> TokenStream2 {
    let entity = &feature.entity.name;
    let snap = suffixed(entity, "Snapshot");
    let store_ty = suffixed(entity, "Store");
    let store_ffi = suffixed(entity, "StoreFfi");
    let draft_ffi = suffixed(entity, "DraftFfi");
    let stash_ffi = suffixed(entity, "StashFfi");
    let values = suffixed(entity, "Values");
    let field_id = suffixed(entity, "FieldId");

    let checker_slots = checker_slots(feature);

    quote! {
        pub struct #store_ffi {
            core: Arc<Mutex<FfiState>>,
        }

        impl Default for #store_ffi {
            fn default() -> Self { Self::new() }
        }

        #[export]
        impl #store_ffi {
            pub fn new() -> #store_ffi {
                #store_ffi {
                    core: Arc::new(Mutex::new(FfiState {
                        store: #store_ty::new(None),
                        producers: BTreeMap::new(),
                        store_producer: Arc::new(StreamProducer::new(256)),
                    })),
                }
            }

            /// Set or replace the canonical entity. `Store::apply_canonical` does all of it and
            /// reports which drafts it moved.
            pub fn apply_canonical(&self, values: #values) -> ::core::result::Result<(), SubmitErrorFfi> {
                let entity = build_entity(values)
                    .map_err(|report| SubmitErrorFfi::Validation { report })?;

                let emits = {
                    let mut g = lock(&self.core);
                    let rebased = g.store.apply_canonical(entity.clone());
                    let mut emits = draft_emits(&g, &rebased);
                    emits.push((
                        g.store_producer.clone(),
                        canonical_snapshot(&entity, g.store.version()),
                    ));
                    emits
                };
                flush(emits);
                Ok(())
            }

            /// Check out a draft. Existing-canonical checkouts register for live rebase; create-flow
            /// checkouts do not (C12).
            pub fn checkout(&self) -> #draft_ffi {
                let mut g = lock(&self.core);
                let id = g.store.checkout();
                let producer = register_producer(&mut g, id);
                #draft_ffi { id, core: Arc::clone(&self.core), producer, #checker_slots }
            }

            /// Restore a draft the shell stashed before its process was killed (C21). The rebase
            /// inside `Store::restore` is the point: a field whose canonical moved while the process
            /// was dead comes back **conflicted**, not silently dirty over a base it never saw.
            pub fn restore(&self, stash: #stash_ffi) -> #draft_ffi {
                let mut g = lock(&self.core);
                let id = g.store.restore(&to_core_stash(&stash));
                let producer = register_producer(&mut g, id);
                #draft_ffi { id, core: Arc::clone(&self.core), producer, #checker_slots }
            }

            /// Declared constraints for a field. Pure metadata, so it takes no lock. A shell derives
            /// `maxLength`, counters and required markers from THIS alone — no constraint literal in
            /// Swift or Kotlin.
            pub fn constraints(&self, field: #field_id) -> Vec<ConstraintFfi> {
                to_core_field(field)
                    .constraints()
                    .into_iter()
                    .map(bolted_ffi::constraint)
                    .collect()
            }

            /// Drafts that exist: checked out or restored, not yet submitted or closed.
            pub fn live_draft_count(&self) -> u32 {
                lock(&self.core).store.draft_count() as u32
            }

            /// Drafts the next canonical change would rebase: not create-flow (C12), not orphaned
            /// (C11). **Not** the same question as `live_draft_count` — see C22.
            pub fn rebasing_draft_count(&self) -> u32 {
                lock(&self.core).store.rebasing_draft_count() as u32
            }

            pub fn canonical(&self) -> Option<#snap> {
                let g = lock(&self.core);
                let version = g.store.version();
                g.store.canonical().map(|e| canonical_snapshot(e, version))
            }

            /// Handle round-trip identity: if BoltFFI passes the same Rust object back across the
            /// boundary, `store.same_draft(d) == d.id()`.
            pub fn same_draft(&self, other: &#draft_ffi) -> u64 {
                other.id.as_u64()
            }

            /// A fresh canonical snapshot on every `apply_canonical` and every successful submit.
            #[ffi_stream(item = #snap)]
            pub fn snapshots(&self) -> Arc<EventSubscription<#snap>> {
                let producer = { lock(&self.core).store_producer.clone() };
                producer.subscribe()
            }
        }
    }
}

/// `username_checker: Mutex::new(None), ` — the struct-literal tail shared by `checkout`/`restore`.
fn checker_slots(feature: &Feature) -> TokenStream2 {
    let slots = feature
        .entity
        .fields
        .iter()
        .filter(|f| f.check.is_some())
        .map(|f| {
            let slot = format_ident!("{}_checker", f.ident);
            quote!(#slot: Mutex::new(None))
        });
    quote!(#(#slots),*)
}

pub fn draft_class(feature: &Feature, fields: &[FieldProj<'_>]) -> TokenStream2 {
    let entity = &feature.entity.name;
    let snap = suffixed(entity, "Snapshot");
    let draft_ty = suffixed(entity, "Draft");
    let draft_ffi = suffixed(entity, "DraftFfi");
    let stash_ffi = suffixed(entity, "StashFfi");
    let field_id = suffixed(entity, "FieldId");
    let check_id = suffixed(entity, "Check");

    let checker_decls = fields.iter().filter(|p| p.field.check.is_some()).map(|p| {
        let (slot, trait_name) = (
            format_ident!("{}_checker", p.ident()),
            checker_trait_name(p),
        );
        quote!(#slot: Mutex<Option<Box<dyn #trait_name>>>)
    });

    let setters = fields.iter().map(|p| {
        let (name, wire, error, closed) = (p.setter(), p.wire_ty(), p.error_ty(), p.closed());
        let core_setter = p.setter();
        // The parameter is `raw`, as the core spells it. Swift labels are part of the surface, and
        // `trySetEmail(raw:)` is what four shells already call.
        let raw = p.to_core_raw(quote!(raw));
        let map_err = p.error_from();
        quote! {
            pub fn #name(&self, raw: #wire) -> ::core::result::Result<(), #error> {
                let (producer, snapshot, result) = {
                    let mut g = lock(&self.core);
                    let Some(draft) = g.store.draft_mut(self.id) else {
                        return Err(#closed);
                    };
                    let result = draft.#core_setter(#raw).map_err(#map_err);
                    let snapshot = build_draft_snapshot(draft);
                    (self.producer.clone(), snapshot, result)
                };
                producer.push(snapshot);
                result
            }
        }
    });

    let check_drivers = fields.iter().filter_map(|p| {
        let check = p.field.check.as_ref()?;
        let id = p.ident();
        let (setter, runner) = (
            format_ident!("set_{}_checker", id),
            format_ident!("run_{}_check", id),
        );
        let (slot, trait_name) = (format_ident!("{}_checker", id), checker_trait_name(p));
        let variant = &check.variant;
        let failed_key = &check.failed_key;
        // The checked field's text, for the foreign call. Only `Raw = String` checks are generated
        // this way; a custom checked field would need its own `text_of`, and no feature has one.
        let text = quote!(bolted_ffi::text_of(&draft.#id));
        Some(quote! {
            pub fn #setter(&self, checker: Box<dyn #trait_name>) {
                *lock(&self.#slot) = Some(checker);
            }

            /// Drive one single-flight check: begin (emit a `Pending` snapshot), call the foreign
            /// checker with **no lock held**, complete (emit the verdict). `Ok(false)` means no
            /// checker is set on a *live* draft; a released draft refuses (D23), checker or not.
            ///
            /// The core discards a superseded token, so a verdict that lands after the value moved is
            /// dropped rather than applied (C13).
            pub fn #runner(&self) -> ::core::result::Result<bool, DraftClosedFfi> {
                // D23: a released draft refuses unconditionally. This gate runs BEFORE the
                // no-checker short-circuit, so a dead draft with no checker still refuses (typed)
                // instead of answering `Ok(false)` -- which is reserved for a live draft with no
                // checker. Its own lock scope: the checker outcall below must hold no lock.
                if lock(&self.core).store.draft_mut(self.id).is_none() {
                    return Err(DraftClosedFfi::DraftClosed);
                }
                // Take the checker OUT of its mutex for the whole operation, so the checker lock is
                // never held across the outcall: a foreign checker may reentrantly touch this draft.
                let Some(checker) = lock(&self.#slot).take() else {
                    return Ok(false);
                };

                let begun = {
                    let mut g = lock(&self.core);
                    g.store.draft_mut(self.id).map(|draft| {
                        let value = #text;
                        let token = draft.begin_check(#check_id::#variant);
                        (token, value, build_draft_snapshot(draft))
                    })
                };
                let Some((token, value, pending)) = begun else {
                    *lock(&self.#slot) = Some(checker);
                    return Err(DraftClosedFfi::DraftClosed);
                };
                self.producer.push(pending);

                // No locks held.
                let verdict = checker.check(value);
                *lock(&self.#slot) = Some(checker);
                let verdict: ::core::result::Result<(), CoreErrorData> = match verdict {
                    CheckVerdictFfi::Pass => Ok(()),
                    CheckVerdictFfi::Fail => Err(CoreErrorData::new(#failed_key)),
                };

                let done = {
                    let mut g = lock(&self.core);
                    g.store.draft_mut(self.id).map(|draft| {
                        let _superseded = draft.complete_check(#check_id::#variant, token, verdict);
                        build_draft_snapshot(draft)
                    })
                };
                match done {
                    Some(snapshot) => { self.producer.push(snapshot); Ok(true) }
                    None => Err(DraftClosedFfi::DraftClosed),
                }
            }
        })
    });

    quote! {
        pub struct #draft_ffi {
            id: DraftId,
            core: Arc<Mutex<FfiState>>,
            producer: Arc<StreamProducer<#snap>>,
            #(#checker_decls,)*
        }

        #[export]
        impl #draft_ffi {
            /// Stable per-draft id.
            pub fn id(&self) -> u64 { self.id.as_u64() }

            /// `true` while the draft is present and un-submitted; `false` once submitted (C17) or
            /// closed (C18). **Ask this before you act**: it is the only total observer of a hazard
            /// the mutators now raise as `DraftClosed`.
            pub fn is_live(&self) -> bool {
                lock(&self.core).store.is_live(self.id)
            }

            #(#setters)*

            pub fn resolve_keep_mine(&self, field: #field_id) -> ::core::result::Result<(), DraftClosedFfi> {
                self.resolve(field, true)
            }

            pub fn resolve_take_theirs(&self, field: #field_id) -> ::core::result::Result<(), DraftClosedFfi> {
                self.resolve(field, false)
            }

            #(#check_drivers)*

            /// Total: a tombstoned draft reports an empty report, not an error. A shell renders this
            /// on every keystroke and must never have to catch.
            pub fn validate(&self) -> ValidationReportFfi {
                let g = lock(&self.core);
                match g.store.draft(self.id) {
                    Some(draft) => project_report(&draft.validate()),
                    None => ValidationReportFfi { field_errors: Vec::new(), rule_errors: Vec::new() },
                }
            }

            /// Commit this draft and adopt the result as the new canonical, rebasing every other
            /// registered draft. On success the draft is released and the handle becomes a tombstone
            /// (C17). On refusal it stays put under the same id: the edit session survives.
            pub fn submit(&self) -> ::core::result::Result<(), SubmitErrorFfi> {
                let emits = {
                    let mut g = lock(&self.core);
                    let rebased = g.store.submit(self.id).map_err(submit_error_to_dto)?;
                    let mut emits = draft_emits(&g, &rebased);
                    let version = g.store.version();
                    if let Some(entity) = g.store.canonical() {
                        emits.push((g.store_producer.clone(), canonical_snapshot(entity, version)));
                    }
                    emits
                };
                flush(emits);
                Ok(())
            }

            /// Flatten to serializable data so the shell can persist it across process death (C20).
            pub fn stash(&self) -> #stash_ffi {
                let g = lock(&self.core);
                match g.store.draft(self.id) {
                    Some(draft) => to_stash_ffi(&draft.stash()),
                    None => to_stash_ffi(&#draft_ty::from_canonical(None, 0).stash()),
                }
            }

            /// The draft's current state on demand — the recovery getter that makes drop-newest stream
            /// overflow non-fatal.
            pub fn snapshot(&self) -> #snap {
                let g = lock(&self.core);
                match g.store.draft(self.id) {
                    Some(draft) => build_draft_snapshot(draft),
                    None => tombstone_snapshot(),
                }
            }

            #[ffi_stream(item = #snap)]
            pub fn snapshots(&self) -> Arc<EventSubscription<#snap>> {
                self.producer.subscribe()
            }

            /// A deliberately tiny subscription, for exercising drop-newest overflow.
            #[ffi_stream(item = #snap)]
            pub fn snapshots_small(&self) -> Arc<EventSubscription<#snap>> {
                self.producer.subscribe_with_capacity(4)
            }

            fn resolve(&self, field: #field_id, keep_mine: bool) -> ::core::result::Result<(), DraftClosedFfi> {
                let (producer, snapshot) = {
                    let mut g = lock(&self.core);
                    let Some(draft) = g.store.draft_mut(self.id) else {
                        return Err(DraftClosedFfi::DraftClosed);
                    };
                    let core_field = to_core_field(field);
                    if keep_mine {
                        draft.resolve_keep_mine(core_field);
                    } else {
                        draft.resolve_take_theirs(core_field);
                    }
                    (self.producer.clone(), build_draft_snapshot(draft))
                };
                producer.push(snapshot);
                Ok(())
            }
        }

        impl Drop for #draft_ffi {
            /// Deinit-deregistration: when the foreign handle is released, `Drop` calls `Store::close`
            /// so `apply_canonical` stops rebasing a zombie. This is the *shell* calling close, not
            /// the framework doing it for free — exactly what C18 says. Kotlin's GC never runs it,
            /// which is why `AutoCloseable`/`onCleared()` are mandatory there.
            fn drop(&mut self) {
                let mut g = lock(&self.core);
                g.store.close(self.id);
                g.producers.remove(&self.id);
            }
        }
    }
}
