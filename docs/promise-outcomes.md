# Promise outcomes

Generated from the project manifest. Review runs attach provenance-validated evidence; the standalone description has no measurements. Unmeasured is not a pass. CPU render endpoints exclude display presentation. These are measured-sweep budget verdicts; promise health also requires scale, coverage and noise checks. The JSON matrix contains one row per gate; the tables repeat shared memberships for readability.

## Large Files

| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| cold_reads_during_save | Physically verified cold reads sampled during saves; gate uses p99 | cold_reads_complete | InteractionDuringBackground | StoreCold | NotRequired | 4000000 ns | unmeasured (missing) |
| file_backed_chunk_cache_tracking | File-backed cache exercise complete | cache_cycle_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 5000000000 ns | unmeasured (missing) |
| file_backed_random_window | Random text windows returned; gate uses p99 | random_windows_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 4000000 ns | unmeasured (missing) |
| large_file_first_visible_paint | First bounded file window prepared for display (not display presentation) | first_window_prepared | ForegroundInteraction | NotApplicable | NotRequired | 180000000 ns | unmeasured (missing) |
| large_utf8_load_peak_memory | UTF-8 load and metadata construction complete | load_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 5000000000 ns | unmeasured (missing) |
| store_import | Streaming store import exercise complete | store_import_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 5000000000 ns | unmeasured (missing) |
| ui_during_store_writes | CPU frames sampled during writes; gate uses frame p99 | frames_during_writes_complete | InteractionDuringBackground | StoreUncontrolled | NotRequired | 12000000 ns | unmeasured (missing) |
| large_file_background_index_ceiling | Large-file background indexing complete | index_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 5000 ms | unmeasured (missing) |
| large_file_first_visible_ceiling | First file prefix available | first_visible | ForegroundInteraction | NotApplicable | NotRequired | 180 ms | unmeasured (missing) |
| editor_scroll_event_to_tessellation | Scroll CPU frame tessellated; display presentation excluded | tessellation_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 8.33 ms | unmeasured (missing) |
| editor_scroll_frame_120hz | Measured CPU frame path complete; display presentation excluded | measured_frame_path_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 8.33 ms | unmeasured (missing) |

## Many Files

| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| many_file_lazy_open_tracking | Lazy-open tracking operation complete | lazy_open_complete | BackgroundCompletion | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| many_file_resource_tracking | Many-file component operation complete | file_cycle_complete | BackgroundCompletion | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| search_target_resource_tracking | Prepared multi-target search component complete | target_search_complete | BackgroundCompletion | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| session_persist_cost | First session persistence request complete | persist_complete | BackgroundCompletion | StoreUncontrolled | CommittedFull | 180000000 ns | unmeasured (missing) |
| session_restore_cost | Full restored session reconstructed and retained | fully_reconstructed_session_retained | BackgroundCompletion | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| session_steady_persist_cost | Unchanged session persistence request complete (may reuse committed snapshot) | steady_persist_complete | BackgroundCompletion | StoreUncontrolled | CommittedFull | 180000000 ns | unmeasured (missing) |
| many_file_background_hydration_ceiling | All declared files hydrated | hydration_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 5000 ms | unmeasured (missing) |
| many_file_first_visible_ceiling | Selected file first visible | first_visible | ForegroundInteraction | NotApplicable | NotRequired | 180 ms | unmeasured (missing) |
| search_target_count_ceiling | Search over all prepared targets complete | target_search_complete | BackgroundCompletion | Owned | NotRequired | 180 ms | unmeasured (missing) |
| search_workflow_all_completion | all search: completion | completion | BackgroundCompletion | Owned | NotRequired | 500 ms | unmeasured (missing) |
| search_workflow_all_dispatch | all search: dispatch | dispatch | ForegroundInteraction | Owned | NotRequired | 50 ms | unmeasured (missing) |
| search_workflow_all_first_render | all search: first render | first_render | ForegroundInteraction | Owned | NotRequired | 120 ms | unmeasured (missing) |

## Search

| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| edited_buffer_search_preview_rendering | Edited-buffer result preview rendering complete | preview_render_complete | ForegroundInteraction | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| file_backed_search | Serial file-backed search scan complete | serial_scan_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 5000000000 ns | unmeasured (missing) |
| search_app_result_tracking | Completed production search results retained | results_retained | BackgroundCompletion | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| search_file_size_resource_tracking | Prepared search component scan complete | search_component_complete | BackgroundCompletion | Owned | NotRequired | 5000000000 ns | unmeasured (missing) |
| search_target_resource_tracking | Prepared multi-target search component complete | target_search_complete | BackgroundCompletion | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| search_file_size_ceiling | All production search ranges returned | all_ranges_returned | BackgroundCompletion | StoreUncontrolled | NotRequired | 180 ms | unmeasured (missing) |
| search_target_count_ceiling | Search over all prepared targets complete | target_search_complete | BackgroundCompletion | Owned | NotRequired | 180 ms | unmeasured (missing) |
| search_workflow_active_completion | active search: completion | completion | BackgroundCompletion | Owned | NotRequired | 500 ms | unmeasured (missing) |
| search_workflow_active_first_render | active search: first render | first_render | ForegroundInteraction | Owned | NotRequired | 120 ms | unmeasured (missing) |
| search_workflow_active_first_result | active search: first result | first_result | ForegroundInteraction | Owned | NotRequired | 100 ms | unmeasured (missing) |
| search_workflow_all_completion | all search: completion | completion | BackgroundCompletion | Owned | NotRequired | 500 ms | unmeasured (missing) |
| search_workflow_all_first_render | all search: first render | first_render | ForegroundInteraction | Owned | NotRequired | 120 ms | unmeasured (missing) |
| search_workflow_all_first_result | all search: first result | first_result | ForegroundInteraction | Owned | NotRequired | 100 ms | unmeasured (missing) |
| search_workflow_current_completion | current search: completion | completion | BackgroundCompletion | Owned | NotRequired | 500 ms | unmeasured (missing) |
| search_workflow_current_first_render | current search: first render | first_render | ForegroundInteraction | Owned | NotRequired | 120 ms | unmeasured (missing) |
| search_workflow_current_first_result | current search: first result | first_result | ForegroundInteraction | Owned | NotRequired | 100 ms | unmeasured (missing) |

## Many Tabs

| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| session_persist_cost | First session persistence request complete | persist_complete | BackgroundCompletion | StoreUncontrolled | CommittedFull | 180000000 ns | unmeasured (missing) |
| session_restore_cost | Full restored session reconstructed and retained | fully_reconstructed_session_retained | BackgroundCompletion | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| session_steady_persist_cost | Unchanged session persistence request complete (may reuse committed snapshot) | steady_persist_complete | BackgroundCompletion | StoreUncontrolled | CommittedFull | 180000000 ns | unmeasured (missing) |
| startup_visible_restore_cost | Startup-visible session subset restored | visible_restore_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| tab_build_targeted | Prepared tabs constructed | tabs_constructed | ForegroundInteraction | Owned | NotRequired | 140000000 ns | unmeasured (missing) |
| tab_combine_targeted | Prepared tabs combined | tabs_combined | ForegroundInteraction | Owned | NotRequired | 140000000 ns | unmeasured (missing) |
| tab_count_resource_tracking | Prepared tab construction/manipulation cycle complete | tab_cycle_complete | ForegroundInteraction | Owned | NotRequired | 140000000 ns | unmeasured (missing) |
| tab_split_targeted | Prepared tabs split | tabs_split | ForegroundInteraction | Owned | NotRequired | 140000000 ns | unmeasured (missing) |
| tab_strip_frame_rendering | Tab strip CPU frame rendered; excludes display presentation | tab_frame_complete | ForegroundInteraction | NotApplicable | NotRequired | 8330000 ns | unmeasured (missing) |
| tab_count_ceiling | Prepared tab operation cycle complete | tab_cycle_complete | ForegroundInteraction | Owned | NotRequired | 140 ms | unmeasured (missing) |

## Many Views

| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| anchor_heavy_view_editing | Shared-text edit and anchor resolution complete | anchors_resolved | ForegroundInteraction | Owned | NotRequired | 120000000 ns | unmeasured (missing) |
| view_count_resource_tracking | Prepared view operation complete | view_cycle_complete | ForegroundInteraction | Owned | NotRequired | 120000000 ns | unmeasured (missing) |
| split_count_ceiling | Prepared split cycle complete | split_cycle_complete | ForegroundInteraction | Owned | NotRequired | 120 ms | unmeasured (missing) |
| view_count_ceiling | Prepared view cycle complete | view_cycle_complete | ForegroundInteraction | Owned | NotRequired | 120 ms | unmeasured (missing) |

## Large Text Mutation

| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| fragmented_long_session_mutation | Fragmented paste/cut/undo/redo component cycle complete | mutation_cycle_complete | ForegroundInteraction | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| production_paste | Accepted paste visible and editable; later durability is separate | paste_published | ForegroundInteraction | StoreUncontrolled | PublishedRam | 150000000 ns | unmeasured (missing) |
| provenance_retained_memory | History exercise complete and retained | history_retained | BackgroundCompletion | Owned | NotRequired | 180000000 ns | unmeasured (missing) |
| paste_size_ceiling | Production paste published | paste_published | ForegroundInteraction | StoreUncontrolled | PublishedRam | 150 ms | unmeasured (missing) |

## Session Persistence Restore

| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| aged_store_close | Aged-store close/archive operation complete | aged_close_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| aged_store_forget | Aged-store forget-history operation complete | aged_forget_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| aged_store_restore | Aged-store restore operation complete | aged_restore_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| aged_store_save | Aged-store save operation complete | aged_save_complete | BackgroundCompletion | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| large_dirty_buffer_save | Large dirty buffer and history durably persisted | persist_complete | BackgroundCompletion | StoreUncontrolled | CommittedFull | 180000000 ns | unmeasured (missing) |
| session_persist_cost | First session persistence request complete | persist_complete | BackgroundCompletion | StoreUncontrolled | CommittedFull | 180000000 ns | unmeasured (missing) |
| session_restore_cost | Full restored session reconstructed and retained | fully_reconstructed_session_retained | BackgroundCompletion | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| session_steady_persist_cost | Unchanged session persistence request complete (may reuse committed snapshot) | steady_persist_complete | BackgroundCompletion | StoreUncontrolled | CommittedFull | 180000000 ns | unmeasured (missing) |
| startup_visible_restore_cost | Startup-visible session subset restored | visible_restore_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 180000000 ns | unmeasured (missing) |
| typing_during_save | Typing cycle complete while persistence runs; gate uses p99 | typing_cycle_complete | InteractionDuringBackground | StoreUncontrolled | NotRequired | 8330000 ns | unmeasured (missing) |
| typing_without_save | Typing cycle complete; gate uses p99 | typing_cycle_complete | ForegroundInteraction | StoreUncontrolled | NotRequired | 8330000 ns | unmeasured (missing) |
| ui_during_store_writes | CPU frames sampled during writes; gate uses frame p99 | frames_during_writes_complete | InteractionDuringBackground | StoreUncontrolled | NotRequired | 12000000 ns | unmeasured (missing) |

## Supporting diagnostics without required promise membership

- paste_allocation: Prepared paste component mutation complete (paste_component_complete; unmeasured)
- in_memory_text_ingest_ceiling: In-memory ingest diagnostic complete (ingest_complete; unmeasured)
- text_layout_ceiling: CPU text layout complete (layout_complete; unmeasured)
- search_workflow_active_dispatch: active search: dispatch (dispatch; unmeasured)
- search_workflow_current_dispatch: current search: dispatch (dispatch; unmeasured)
## Shared gate membership

- resource_budgets / search_target_resource_tracking: many_files, search
- resource_budgets / session_persist_cost: many_files, many_tabs, session_restore
- resource_budgets / session_restore_cost: many_files, many_tabs, session_restore
- resource_budgets / session_steady_persist_cost: many_files, many_tabs, session_restore
- resource_budgets / startup_visible_restore_cost: many_tabs, session_restore
- resource_budgets / ui_during_store_writes: large_files, session_restore
- capacity_budgets / search_target_count_ceiling: many_files, search
- probe_measurements / search_workflow_all_completion: many_files, search
- probe_measurements / search_workflow_all_first_render: many_files, search

## Memory and durability observations

Pending queue size is not total accepted uncommitted text. A NORMAL body flush does not establish FULL durability. Missing exposure or residency observations remain unknown.

| Gate | Observed samples | Pending policy changed | FULL exposure observed | Missing memory metrics |
|---|---|---|---|---|
