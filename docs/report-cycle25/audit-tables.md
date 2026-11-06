# Full audit after cycle 25

All eight producers completed in 806.440 seconds. These are observed timings, not confidence intervals. Source paths and hashes are preserved in [data.json](data.json).

## Historical audit checkpoints

| Audit after cycle | Reported budget misses | Unmet scale checks | Covered / thin promises | Health | Wall seconds |
|---|---:|---:|---|---|---:|
| 1 | 19 | 7 | 5 / 2 | 6 fail; 1 at risk | 1038.378 |
| 9 | 16 | 6 | 7 / 0 | 6 fail; 1 at risk | 884.236 |
| 16 | 12 | 4 | 7 / 0 | 6 fail; 1 at risk | 834.173 |
| 25 | 11 | 6 | 6 / 1 | 6 fail; 1 at risk | 806.440 |

Counts are historical review outputs, not a count of unique failing raw measurements: some evidence is shared across promises, and capacity failures are represented separately as scale checks. Methods and evidence rules changed. Cycle 1 capacity evidence was partial.

## Current promise verdicts and reasons

| Promise | Health | Coverage | Scale checks not met | Evidence caveats |
|---|---|---|---|---|
| Large Files | fail | covered | GB-class text file sweep (failed_before_promise); Cold reads during saves on a multi-GiB store (missing) | Measurement controls are noisy or incomplete in: capacity |
| Many Files | fail | covered | None | Measurement controls are noisy or incomplete in: resources |
| Search | fail | covered | GB-class search file (failed_before_promise) | None reported |
| Many Tabs | fail | covered | None | Measurement controls are noisy or incomplete in: resources, capacity |
| Many Views | at_risk | covered | None | Measurement controls are noisy or incomplete in: resources |
| Large Text Mutation | fail | covered | Large paste/mutation sweep (failed_before_promise) | Measurement controls are noisy or incomplete in: capacity |
| Session Persistence Restore | fail | thin | 10,000+ tab session restore (missing); Aged multi-GiB store operations (missing) | Missing valid resource evidence: aged_store_forget.; Measurement controls are noisy or incomplete in: resources |

## Resource measurements at each scenario’s largest workload

The value is read from the declared budget metric—not necessarily outer elapsed time. For cold reads it is each launch’s observed p99; for paste it is publication time. The median below is a descriptive median across available observations, not a replacement for the lens verdict. “Pass” in this table is the row budget status; noisy rows do not establish a healthy promise. All original workloads, launch values and named phases remain in data.json.

| Scenario | Workload | Metric | Budget ms | Median ms | Observed min–max ms | Row status | CPU controls |
|---|---:|---|---:|---:|---|---|---|
| aged_store_close | 3221225472 bytes | `/elapsed_ns` | 180.000 | 17.222 | 7.015–43.168 | pass | stable |
| aged_store_forget | 3221225472 bytes | `/elapsed_ns` | 180.000 | 105.174 | 100.031–215.650 | uncertain | stable |
| aged_store_restore | 3221225472 bytes | `/elapsed_ns` | 180.000 | 2.398 | 2.215–9.643 | pass | stable |
| aged_store_save | 3221225472 bytes | `/elapsed_ns` | 180.000 | 1.102 | 0.922–2.700 | pass | stable |
| anchor_heavy_view_editing | 40000 anchors | `/elapsed_ns` | 120.000 | 26.983 | 26.983–26.983 | pass | stable |
| cold_reads_during_save | 3221225472 bytes | `/phase_elapsed_ns/cold_read_p99_ns` | 4.000 | 4.068 | 3.679–4.114 | fail | stable |
| edited_buffer_search_preview_rendering | 10000 pieces | `/elapsed_ns` | 180.000 | 6.526 | 6.526–6.526 | pass | stable |
| file_backed_chunk_cache_tracking | 1073741824 bytes | `/elapsed_ns` | 5000.000 | 1196.657 | 1196.657–1196.657 | pass | stable |
| file_backed_random_window | 1073741824 bytes | `/phase_elapsed_ns/window_p99_ns` | 4.000 | 0.786 | 0.786–0.786 | pass | stable |
| file_backed_search | 67108864 bytes | `/elapsed_ns` | 5000.000 | 168.629 | 165.427–219.641 | pass | stable |
| fragmented_long_session_mutation | 20000 fragments | `/elapsed_ns` | 180.000 | 4.040 | 4.040–4.040 | pass | stable |
| large_dirty_buffer_save | 52428800 bytes | `/elapsed_ns` | 180.000 | 20.460 | 19.221–23.083 | pass | stable |
| large_file_first_visible_paint | 2147483648 bytes | `/elapsed_ns` | 180.000 | 27.873 | 27.873–27.873 | pass | stable |
| large_utf8_load_peak_memory | 2147483648 bytes | `/elapsed_ns` | 5000.000 | 8919.663 | 8919.663–8919.663 | fail | stable |
| many_file_lazy_open_tracking | 10000 files | `/elapsed_ns` | 180.000 | 1013.034 | 1013.034–1013.034 | fail | stable |
| many_file_resource_tracking | 50000 files | `/elapsed_ns` | 180.000 | 432.784 | 432.784–432.784 | fail | stable |
| production_paste | 134217728 bytes | `/phase_elapsed_ns/publication_ns` | 150.000 | 1404.355 | 1404.355–1404.355 | fail | stable |
| provenance_retained_memory | 100000 edits | `/elapsed_ns` | 180.000 | 93.560 | 93.560–93.560 | pass | stable |
| search_app_result_tracking | 1000 tabs | `/elapsed_ns` | 180.000 | 16.245 | 16.245–16.245 | pass | stable |
| search_file_size_resource_tracking | 268435456 bytes | `/elapsed_ns` | 5000.000 | 135.640 | 135.640–135.640 | pass | stable |
| search_target_resource_tracking | 10000 files | `/elapsed_ns` | 180.000 | 2.433 | 2.433–2.433 | pass | stable |
| session_persist_cost | 10000 tabs | `/elapsed_ns` | 180.000 | 1291.642 | 1280.092–1322.585 | fail | noisy |
| session_restore_cost | 10000 tabs | `/elapsed_ns` | 180.000 | 291.930 | 284.833–311.192 | fail | noisy |
| session_steady_persist_cost | 10000 tabs | `/elapsed_ns` | 180.000 | 97.877 | 97.277–101.827 | pass | noisy |
| startup_visible_restore_cost | 10000 tabs | `/elapsed_ns` | 180.000 | 18.804 | 17.557–19.024 | pass | noisy |
| store_import | 1073741824 bytes | `/elapsed_ns` | 5000.000 | 3607.309 | 3607.309–3607.309 | pass | stable |
| tab_build_targeted | 10000 tabs | `/elapsed_ns` | 140.000 | 96.326 | 96.326–96.326 | pass | noisy |
| tab_combine_targeted | 10000 tabs | `/elapsed_ns` | 140.000 | 0.380 | 0.380–0.380 | pass | stable |
| tab_count_resource_tracking | 10000 tabs | `/elapsed_ns` | 140.000 | 116.048 | 116.048–116.048 | pass | stable |
| tab_split_targeted | 10000 tabs | `/elapsed_ns` | 140.000 | 16.083 | 16.083–16.083 | pass | stable |
| tab_strip_frame_rendering | 10000 tabs | `/result_value` | 8.330 | 0.926 | 0.926–0.926 | pass | stable |
| typing_during_save | 52428800 bytes | `/phase_elapsed_ns/typing_p99_ns` | 8.330 | 0.004 | 0.004–0.005 | pass | noisy |
| typing_without_save | 52428800 bytes | `/phase_elapsed_ns/typing_p99_ns` | 8.330 | 0.002 | 0.002–0.004 | pass | stable |
| ui_during_store_writes | 67108864 bytes | `/phase_elapsed_ns/frame_p99_ns` | 12.000 | 2.311 | 2.311–2.311 | pass | stable |
| view_count_resource_tracking | 1000 views | `/elapsed_ns` | 120.000 | 5.070 | 5.070–5.070 | pass | noisy |

## Complete capacity sweeps

The budget gate uses the recorded median. Whisker ranges and CPU controls remain material caveats; “within budget” is not a claim that every repetition met the limit.

| Scenario | Workload | Budget ms | Median ms | Observed min–max ms | Median / budget | CPU controls |
|---|---:|---:|---:|---|---:|---|
| large_file_background_index_ceiling | 1048576 | 5000.000 | 68.695 | 16.845–97.265 | 0.01× | noisy |
| large_file_background_index_ceiling | 134217728 | 5000.000 | 1279.486 | 668.166–2154.364 | 0.26× | noisy |
| large_file_background_index_ceiling | 536870912 | 5000.000 | 4573.651 | 1753.411–5899.874 | 0.91× | noisy |
| large_file_background_index_ceiling | 1073741824 | 5000.000 | 7866.191 | 3620.775–9090.387 | 1.57× | noisy |
| large_file_first_visible_ceiling | 1048576 | 180.000 | 10.787 | 10.514–12.340 | 0.06× | noisy |
| large_file_first_visible_ceiling | 134217728 | 180.000 | 9.342 | 9.052–10.161 | 0.05× | noisy |
| large_file_first_visible_ceiling | 536870912 | 180.000 | 8.744 | 8.419–9.269 | 0.05× | noisy |
| large_file_first_visible_ceiling | 1073741824 | 180.000 | 9.665 | 9.026–12.264 | 0.05× | noisy |
| many_file_background_hydration_ceiling | 2048 | 5000.000 | 197.069 | 193.813–200.502 | 0.04× | stable |
| many_file_background_hydration_ceiling | 10000 | 5000.000 | 953.547 | 947.671–971.293 | 0.19× | stable |
| many_file_background_hydration_ceiling | 50000 | 5000.000 | 4778.657 | 4609.560–4847.438 | 0.96× | stable |
| many_file_first_visible_ceiling | 2048 | 180.000 | 23.999 | 23.187–24.269 | 0.13× | stable |
| many_file_first_visible_ceiling | 10000 | 180.000 | 84.533 | 83.435–86.056 | 0.47× | stable |
| many_file_first_visible_ceiling | 50000 | 180.000 | 397.737 | 397.101–399.216 | 2.21× | stable |
| paste_size_ceiling | 8388608 | 150.000 | 17.045 | 15.057–19.856 | 0.11× | noisy |
| paste_size_ceiling | 67108864 | 150.000 | 384.306 | 129.045–703.157 | 2.56× | noisy |
| paste_size_ceiling | 134217728 | 150.000 | 353.238 | 340.188–704.854 | 2.35× | noisy |
| search_file_size_ceiling | 1048576 | 180.000 | 0.362 | 0.317–1.249 | 0.00× | stable |
| search_file_size_ceiling | 67108864 | 180.000 | 85.865 | 84.897–126.012 | 0.48× | stable |
| search_file_size_ceiling | 268435456 | 180.000 | 339.778 | 331.983–506.067 | 1.89× | stable |
| search_file_size_ceiling | 1073741824 | 180.000 | 1445.451 | 1407.419–2047.781 | 8.03× | stable |
| search_target_count_ceiling | 100 | 180.000 | 2.922 | 2.888–3.159 | 0.02× | stable |
| search_target_count_ceiling | 1000 | 180.000 | 4.151 | 3.885–6.187 | 0.02× | stable |
| search_target_count_ceiling | 10000 | 180.000 | 21.614 | 20.784–28.780 | 0.12× | stable |
| split_count_ceiling | 4 | 120.000 | 0.034 | 0.029–0.099 | 0.00× | stable |
| split_count_ceiling | 32 | 120.000 | 0.025 | 0.020–0.034 | 0.00× | stable |
| split_count_ceiling | 128 | 120.000 | 0.161 | 0.094–0.174 | 0.00× | stable |
| split_count_ceiling | 512 | 120.000 | 0.670 | 0.655–0.785 | 0.01× | stable |
| split_count_ceiling | 1000 | 120.000 | 1.186 | 1.098–1.450 | 0.01× | stable |
| tab_count_ceiling | 32 | 140.000 | 0.009 | 0.008–0.033 | 0.00× | noisy |
| tab_count_ceiling | 512 | 140.000 | 0.611 | 0.586–0.622 | 0.00× | noisy |
| tab_count_ceiling | 4096 | 140.000 | 4.961 | 4.625–6.835 | 0.04× | noisy |
| tab_count_ceiling | 10000 | 140.000 | 13.929 | 12.185–16.232 | 0.10× | noisy |
| tab_count_ceiling | 20000 | 140.000 | 26.236 | 23.664–26.440 | 0.19× | noisy |
| view_count_ceiling | 32 | 120.000 | 0.004 | 0.004–0.075 | 0.00× | stable |
| view_count_ceiling | 128 | 120.000 | 0.010 | 0.010–0.010 | 0.00× | stable |
| view_count_ceiling | 512 | 120.000 | 0.038 | 0.034–0.040 | 0.00× | stable |
| view_count_ceiling | 1000 | 120.000 | 0.067 | 0.065–0.073 | 0.00× | stable |

## Production search workflows

Thirty independent fixtures per mode. Headless CPU layout/tessellation; GPU/display present excluded.

| Workflow | Budget ms | Median ms | Observed max ms | Budget status | Controls |
|---|---:|---:|---:|---|---|
| search_workflow_active_dispatch/1 | 50.000 | 0.009 | 0.018 | pass | stable |
| search_workflow_active_first_result/1 | 100.000 | 0.536 | 1.019 | pass | stable |
| search_workflow_active_first_render/1 | 120.000 | 3.462 | 7.756 | pass | stable |
| search_workflow_active_completion/1 | 500.000 | 3.465 | 7.760 | pass | stable |
| search_workflow_current_dispatch/32 | 50.000 | 0.055 | 0.080 | pass | stable |
| search_workflow_current_first_result/32 | 100.000 | 0.537 | 0.645 | pass | stable |
| search_workflow_current_first_render/32 | 120.000 | 4.278 | 5.702 | pass | stable |
| search_workflow_current_completion/32 | 500.000 | 4.281 | 5.705 | pass | stable |
| search_workflow_all_dispatch/10000 | 50.000 | 11.913 | 15.066 | pass | stable |
| search_workflow_all_first_result/10000 | 100.000 | 16.179 | 19.678 | pass | stable |
| search_workflow_all_first_render/10000 | 120.000 | 26.813 | 29.807 | pass | stable |
| search_workflow_all_completion/10000 | 500.000 | 26.817 | 30.058 | pass | stable |

## Measurement cost

Wall time includes setup, builds where needed, probes and reporting. This is runner cost, not application latency.

| Producer | Seconds | Status |
|---|---:|---|
| slowspots | 320.772 | completed |
| frame-metrics | 8.932 | completed |
| search | 0.298 | completed |
| search-workflows | 11.286 | completed |
| capacity | 259.650 | completed |
| resources | 204.794 | completed |
| speed-report | 0.306 | completed |
| performance-review | 0.383 | completed |

Full-audit wall time is not a controlled runner-speed benchmark: host conditions, launch requirements and fixture sharing changed. Cycle 25 is 806.440 s versus 884.2 s after cycle 9 and 834.173 s after cycle 16; those differences alone cannot establish a causal efficiency gain. The clearest structural saving is shared expensive fixture construction.
