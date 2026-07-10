//! The suite stampers.
//!
//! These are `macro_rules!` and they do exactly one thing: turn each generic `cNN_*` function into a
//! `#[test]` in the caller's crate. No logic, no conditionals, no cleverness — the same doctrine
//! ARCHITECTURE §5 sets for `bolted-macros` ("generics carry behavior, macros only stamp names"),
//! for the same reason: macro output is the least verifiable code in the ladder, so it must stay
//! trivial enough to read at a glance.
//!
//! The point of stamping rather than letting each feature write its own `#[test]` wrappers is that a
//! fixture then **cannot skip an ID**. `tests/manifest.rs` checks that every `cNN_*` in this crate
//! appears in exactly one stamper below, so a test cannot exist and quietly never run.

/// The value and field tiers, for one value type. Run it once per `Value` a feature declares.
#[macro_export]
macro_rules! field_suite {
    ($name:ident, $value:ty) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            #[test]
            fn c01_value_raw_roundtrip() {
                $crate::c01_value_raw_roundtrip::<$value>()
            }
            #[test]
            fn c02_a_clean_field_follows_canonical() {
                $crate::c02_a_clean_field_follows_canonical::<$value>()
            }
            #[test]
            fn c03_a_dirty_field_is_never_silently_overwritten() {
                $crate::c03_a_dirty_field_is_never_silently_overwritten::<$value>()
            }
            #[test]
            fn c04_convergent_rebase_is_clean() {
                $crate::c04_convergent_rebase_is_clean::<$value>()
            }
            #[test]
            fn c05_revert_clears_dirty() {
                $crate::c05_revert_clears_dirty::<$value>()
            }
            #[test]
            fn c06_a_failed_set_is_recorded_as_invalid() {
                $crate::c06_a_failed_set_is_recorded_as_invalid::<$value>()
            }
            #[test]
            fn c09_resolution_semantics() {
                $crate::c09_resolution_semantics::<$value>()
            }
            #[test]
            fn c14_editing_to_theirs_auto_converges() {
                $crate::c14_editing_to_theirs_auto_converges::<$value>()
            }
            #[test]
            fn c19_rebase_is_a_three_way_merge_and_idempotent() {
                $crate::c19_rebase_is_a_three_way_merge_and_idempotent::<$value>()
            }
            #[test]
            fn c20_a_field_stashes_to_raw_and_restores() {
                $crate::c20_a_field_stashes_to_raw_and_restores::<$value>()
            }
            #[test]
            fn c23_a_degraded_ancestor_restores_dirty_and_conflicts_on_rebase() {
                $crate::c23_a_degraded_ancestor_restores_dirty_and_conflicts_on_rebase::<$value>()
            }
        }
    };
}

/// The feature tier: every invariant a Bolted feature must satisfy, rule or no rule, check or no
/// check.
#[macro_export]
macro_rules! feature_suite {
    ($name:ident, $fixture:ty) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            #[test]
            fn c06_no_stale_value_submit() {
                $crate::c06_no_stale_value_submit::<$fixture>()
            }
            #[test]
            fn c07_commit_is_the_parse_moment() {
                $crate::c07_commit_is_the_parse_moment::<$fixture>()
            }
            #[test]
            fn c11_deletion_orphans() {
                $crate::c11_deletion_orphans::<$fixture>()
            }
            #[test]
            fn c12_create_flow_never_rebases() {
                $crate::c12_create_flow_never_rebases::<$fixture>()
            }
            #[test]
            fn c12_an_ancestor_in_any_field_means_the_draft_is_entity_backed() {
                $crate::c12_an_ancestor_in_any_field_means_the_draft_is_entity_backed::<$fixture>()
            }
            #[test]
            fn c15_the_base_version_tracks_the_rebase() {
                $crate::c15_the_base_version_tracks_the_rebase::<$fixture>()
            }
            #[test]
            fn c17_submit_releases_the_draft() {
                $crate::c17_submit_releases_the_draft::<$fixture>()
            }
            #[test]
            fn c18_release_is_explicit_and_idempotent() {
                $crate::c18_release_is_explicit_and_idempotent::<$fixture>()
            }
            #[test]
            fn c19_the_store_does_not_conflict_an_unmoved_field() {
                $crate::c19_the_store_does_not_conflict_an_unmoved_field::<$fixture>()
            }
            #[test]
            fn c20_a_draft_stashes_and_restores() {
                $crate::c20_a_draft_stashes_and_restores::<$fixture>()
            }
            #[test]
            fn c20_sync_is_not_stashed_and_re_derives() {
                $crate::c20_sync_is_not_stashed_and_re_derives::<$fixture>()
            }
            #[test]
            fn c21_restore_conflicts_only_the_fields_whose_canonical_moved() {
                $crate::c21_restore_conflicts_only_the_fields_whose_canonical_moved::<$fixture>()
            }
            #[test]
            fn c21_a_resolved_conflict_stays_resolved_across_restore() {
                $crate::c21_a_resolved_conflict_stays_resolved_across_restore::<$fixture>()
            }
            #[test]
            fn c21_restore_into_a_deleted_canonical_orphans_the_draft() {
                $crate::c21_restore_into_a_deleted_canonical_orphans_the_draft::<$fixture>()
            }
            #[test]
            fn c21_a_restored_create_flow_draft_is_never_moved() {
                $crate::c21_a_restored_create_flow_draft_is_never_moved::<$fixture>()
            }
            #[test]
            fn c22_draft_count_and_rebasing_draft_count_are_different_questions() {
                $crate::c22_draft_count_and_rebasing_draft_count_are_different_questions::<$fixture>()
            }
        }
    };
}

/// C08 — for a feature with at least one tier-2 rule.
#[macro_export]
macro_rules! rule_suite {
    ($name:ident, $fixture:ty) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            #[test]
            fn c08_rebase_reruns_tier2() {
                $crate::c08_rebase_reruns_tier2::<$fixture>()
            }
        }
    };
}

/// C10, C13, C16 and C20's verdict clause — for a feature with an async single-flight check.
#[macro_export]
macro_rules! async_check_suite {
    ($name:ident, $fixture:ty) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            #[test]
            fn c10_latest_check_wins() {
                $crate::c10_latest_check_wins::<$fixture>()
            }
            #[test]
            fn c13_verdicts_are_value_bound() {
                $crate::c13_verdicts_are_value_bound::<$fixture>()
            }
            #[test]
            fn c16_an_unrun_check_blocks_a_dirty_field() {
                $crate::c16_an_unrun_check_blocks_a_dirty_field::<$fixture>()
            }
            #[test]
            fn c20_an_async_verdict_does_not_survive_the_stash() {
                $crate::c20_an_async_verdict_does_not_survive_the_stash::<$fixture>()
            }
        }
    };
}
