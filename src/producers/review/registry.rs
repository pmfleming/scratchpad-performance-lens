use crate::artifacts::performance_review::{ProbeClass, PromisedScale};
use crate::shared;
use std::collections::BTreeMap;
pub(super) struct ReviewScenario {
    pub(super) id: &'static str,
    pub(super) title: &'static str,
    pub(super) promise: &'static str,
    pub(super) families: &'static [&'static str],
    pub(super) benchmark_keys: &'static [&'static str],
    pub(super) capacity_scenarios: &'static [&'static str],
    pub(super) resource_scenarios: &'static [&'static str],
    pub(super) profile_ids: &'static [&'static str],
    pub(super) scale_targets: &'static [ScaleTarget],
}

pub(super) struct ScaleTarget {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) minimum: i64,
    pub(super) unit: &'static str,
    pub(super) capacity_scenarios: &'static [&'static str],
    pub(super) resource_scenarios: &'static [&'static str],
}

pub(super) fn review_scenarios() -> Vec<ReviewScenario> {
    vec![
        ReviewScenario {
            id: "large_files",
            title: "Large Files",
            promise: "Load, inspect, scroll, and edit very large text files quickly.",
            families: &["file-load", "scroll", "viewport", "snapshot", "text-layout"],
            benchmark_keys: &[
                "file_load",
                "file_open_latency",
                "ui_render_frame_120hz",
                "ui_render_frame",
                "editor_scroll_frame_120hz",
                "scroll_stress_latency",
                "document_snapshot_creation_latency",
                "viewport_extraction_latency",
            ],
            capacity_scenarios: &[
                "large_file_first_visible_ceiling",
                "large_file_background_index_ceiling",
            ],
            resource_scenarios: &[
                "large_utf8_load_peak_memory",
                "large_file_first_visible_paint",
            ],
            profile_ids: &[
                "scroll_stress_profile",
                "viewport_extraction_profile",
                "document_snapshot_profile",
            ],
            scale_targets: &[ScaleTarget {
                id: "gb_class_files",
                label: "GB-class text file sweep",
                minimum: shared::GB,
                unit: "bytes",
                capacity_scenarios: &[
                    "large_file_first_visible_ceiling",
                    "large_file_background_index_ceiling",
                ],
                resource_scenarios: &[
                    "large_utf8_load_peak_memory",
                    "large_file_first_visible_paint",
                ],
            }],
        },
        ReviewScenario {
            id: "many_files",
            title: "Many Files",
            promise: "Keep workspace and file workflows responsive above 10,000 files.",
            families: &[
                "many-files",
                "file-load",
                "session-persistence",
                "search",
                "search-dispatch",
            ],
            benchmark_keys: &[
                "search_current_completion_aggregate_size",
                "search_all_completion_aggregate_size",
                "search_current_dispatch_aggregate_size",
                "search_all_dispatch_aggregate_size",
            ],
            capacity_scenarios: &[
                "many_file_first_visible_ceiling",
                "many_file_background_hydration_ceiling",
                "search_target_count_ceiling",
            ],
            resource_scenarios: &[
                "many_file_resource_tracking",
                "many_file_lazy_open_tracking",
                "search_target_resource_tracking",
                "session_persist_cost",
                "session_restore_cost",
            ],
            profile_ids: &["search_all_tabs_profile", "search_dispatch_profile"],
            scale_targets: &[ScaleTarget {
                id: "ten_thousand_files",
                label: "10,000+ file workspace",
                minimum: 10_000,
                unit: "files",
                capacity_scenarios: &[
                    "many_file_first_visible_ceiling",
                    "many_file_background_hydration_ceiling",
                    "search_target_count_ceiling",
                ],
                resource_scenarios: &[
                    "many_file_resource_tracking",
                    "many_file_lazy_open_tracking",
                    "search_target_resource_tracking",
                ],
            }],
        },
        ReviewScenario {
            id: "search",
            title: "Search",
            promise:
                "Return first matches quickly and finish searches over huge files and many files.",
            families: &["search", "search-dispatch"],
            benchmark_keys: &[
                "buffer_search_regex",
                "search_active_completion_file_size",
                "search_active_first_response_file_size",
                "search_current_completion_file_size",
                "search_current_first_response_file_size",
                "search_all_completion_file_size",
                "search_all_first_response_file_size",
                "search_current_completion_aggregate_size",
                "search_all_completion_aggregate_size",
                "search_capacity",
            ],
            capacity_scenarios: &["search_file_size_ceiling", "search_target_count_ceiling"],
            resource_scenarios: &[
                "search_file_size_resource_tracking",
                "search_target_resource_tracking",
                "search_app_result_tracking",
                "edited_buffer_search_preview_rendering",
            ],
            profile_ids: &[
                "search_current_app_state_profile",
                "search_all_tabs_profile",
                "search_dispatch_profile",
                "search_capacity_profile",
            ],
            scale_targets: &[
                ScaleTarget {
                    id: "gb_class_search",
                    label: "GB-class search file",
                    minimum: shared::GB,
                    unit: "bytes",
                    capacity_scenarios: &["search_file_size_ceiling"],
                    resource_scenarios: &["search_file_size_resource_tracking"],
                },
                ScaleTarget {
                    id: "ten_thousand_search_targets",
                    label: "10,000+ search targets",
                    minimum: 10_000,
                    unit: "files",
                    capacity_scenarios: &["search_target_count_ceiling"],
                    resource_scenarios: &["search_target_resource_tracking"],
                },
            ],
        },
        ReviewScenario {
            id: "many_tabs",
            title: "Many Tabs",
            promise: "Open, switch, reorder, and manipulate huge tab sets quickly.",
            families: &["tab-management"],
            benchmark_keys: &["tab_stress_operations", "tab_count_scale"],
            capacity_scenarios: &["tab_count_ceiling"],
            resource_scenarios: &[
                "tab_count_resource_tracking",
                "tab_build_targeted",
                "tab_split_targeted",
                "tab_combine_targeted",
                "tab_strip_frame_rendering",
                "session_persist_cost",
                "session_restore_cost",
                "startup_visible_restore_cost",
            ],
            profile_ids: &["tab_operations_profile", "tab_tile_layout_profile"],
            scale_targets: &[ScaleTarget {
                id: "ten_thousand_tabs",
                label: "10,000+ tabs",
                minimum: 10_000,
                unit: "tabs",
                capacity_scenarios: &["tab_count_ceiling"],
                resource_scenarios: &[
                    "tab_count_resource_tracking",
                    "tab_build_targeted",
                    "tab_split_targeted",
                    "tab_combine_targeted",
                    "tab_strip_frame_rendering",
                    "startup_visible_restore_cost",
                ],
            }],
        },
        ReviewScenario {
            id: "many_views",
            title: "Many Views",
            promise: "Keep many views into the same loaded files responsive.",
            families: &["split-layout", "viewport"],
            benchmark_keys: &[
                "split_stress_latency",
                "tile_count_scale",
                "viewport_extraction_latency",
            ],
            capacity_scenarios: &["split_count_ceiling", "view_count_ceiling"],
            resource_scenarios: &["view_count_resource_tracking", "anchor_heavy_view_editing"],
            profile_ids: &[
                "split_stress_profile",
                "tab_tile_layout_profile",
                "view_navigation_profile",
                "viewport_extraction_profile",
            ],
            scale_targets: &[
                ScaleTarget {
                    id: "one_thousand_splits",
                    label: "1,000+ splits",
                    minimum: 1_000,
                    unit: "splits",
                    capacity_scenarios: &["split_count_ceiling"],
                    resource_scenarios: &[],
                },
                ScaleTarget {
                    id: "one_thousand_views",
                    label: "1,000+ views",
                    minimum: 1_000,
                    unit: "views",
                    capacity_scenarios: &["view_count_ceiling"],
                    resource_scenarios: &["view_count_resource_tracking"],
                },
            ],
        },
        ReviewScenario {
            id: "text_mutation",
            title: "Large Text Mutation",
            promise:
                "Paste, cut, undo, redo, and metadata refresh should stay fast on huge buffers.",
            families: &["edit-paste", "anchor-maintenance"],
            benchmark_keys: &["paste_stress_latency", "piece_tree_anchor_remove"],
            capacity_scenarios: &["paste_size_ceiling"],
            resource_scenarios: &[
                "paste_allocation",
                "provenance_retained_memory",
                "fragmented_long_session_mutation",
            ],
            profile_ids: &["paste_stress_profile"],
            scale_targets: &[ScaleTarget {
                id: "large_text_mutation",
                label: "Large paste/mutation sweep",
                minimum: 128 * shared::MB,
                unit: "bytes",
                capacity_scenarios: &["paste_size_ceiling"],
                resource_scenarios: &["paste_allocation"],
            }],
        },
        ReviewScenario {
            id: "session_restore",
            title: "Session Persistence Restore",
            promise: "Persist and restore very large workspaces without startup stalls.",
            families: &["session-persistence"],
            benchmark_keys: &["session_restore_latency", "session_persist_latency"],
            capacity_scenarios: &[],
            resource_scenarios: &[
                "session_persist_cost",
                "session_restore_cost",
                "startup_visible_restore_cost",
            ],
            profile_ids: &[],
            scale_targets: &[ScaleTarget {
                id: "ten_thousand_tab_restore",
                label: "10,000+ tab session restore",
                minimum: 10_000,
                unit: "tabs",
                capacity_scenarios: &[],
                resource_scenarios: &[
                    "session_persist_cost",
                    "session_restore_cost",
                    "startup_visible_restore_cost",
                ],
            }],
        },
    ]
}

pub(super) fn promised_scale_payload(scenario: &ReviewScenario) -> Vec<PromisedScale> {
    scenario
        .scale_targets
        .iter()
        .map(|target| PromisedScale {
            id: target.id.to_string(),
            label: target.label.to_string(),
            minimum: target.minimum,
            unit: target.unit.to_string(),
            capacity_scenarios: target
                .capacity_scenarios
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            resource_scenarios: target
                .resource_scenarios
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        })
        .collect()
}

pub(super) fn probe_classes() -> BTreeMap<String, ProbeClass> {
    BTreeMap::from([
        (
            "ceiling_health".to_string(),
            ProbeClass {
                label: "Ceiling / Promise Health".to_string(),
                purpose: "Shows whether a seven-promise scale boundary still passes.".to_string(),
            },
        ),
        (
            "targeted_path".to_string(),
            ProbeClass {
                label: "Targeted Path".to_string(),
                purpose:
                    "Shows whether a specific implementation path or recent change is working."
                        .to_string(),
            },
        ),
        (
            "diagnostic_profile".to_string(),
            ProbeClass {
                label: "Diagnostic Profile".to_string(),
                purpose:
                    "Explains where time goes after a targeted or ceiling probe finds pressure."
                        .to_string(),
            },
        ),
    ])
}
