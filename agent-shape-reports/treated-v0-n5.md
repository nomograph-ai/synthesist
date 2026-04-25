# agent-shape report: synthesist

run_timestamp: `unix:1777093671`
judge_model: `claude-haiku-4-5`

## Tuning battery

- n_trials: 50
- mean_score: 0.640
- completion_rate: 96.0%
- mean_tokens: 2276
- mean_turns: 12.76
- total_invented_commands: 44
- total_fallback_to_sql: 11

## Holdout battery

_empty in v1 (schema supports it; corpus deferred)_

## Per-cell breakdown

| section | task | model | n | score | stddev | tokens | turns | invented | fallback | irr_delta |
|---------|------|-------|---|-------|--------|--------|-------|----------|----------|-----------|
| tuning | cross-tree-query-01 | claude-opus-4-7 | 5 | 0.850 | 0.335 | 1891 | 10.40 | — | 0 | 0.150 |
| tuning | cross-tree-query-01 | claude-sonnet-4-6 | 5 | 0.850 | 0.335 | 1543 | 10.20 | synthesist spec list; synthesist spec list --tree | 1 | 0.150 |
| tuning | exploration-01 | claude-opus-4-7 | 5 | 0.500 | 0.000 | 2244 | 10.80 | phase get; synthesist phase get; synthesist spec list (without tree argument); synthesist tree show; tree show | 0 | 0.200 |
| tuning | exploration-01 | claude-sonnet-4-6 | 5 | 0.500 | 0.306 | 3481 | 23.80 | phase get; spec list; spec list --tree; synthesist phase get --session treatment-design; synthesist spec list --tree | 2 | 0.150 |
| tuning | resume-work-01 | claude-opus-4-7 | 5 | 0.600 | 0.224 | 1061 | 7.00 | synthesist phase get; synthesist phase get --session treatment-design | 0 | 0.200 |
| tuning | resume-work-01 | claude-sonnet-4-6 | 5 | 0.500 | 0.000 | 2075 | 13.60 | phase get; session list --all; session list --closed; spec list; synthesist phase get --session treatment-design | 0 | 0.300 |
| tuning | session-drill-down-01 | claude-opus-4-7 | 5 | 0.900 | 0.224 | 2679 | 10.80 | synthesist session list --all | 0 | 0.200 |
| tuning | session-drill-down-01 | claude-sonnet-4-6 | 5 | 1.000 | 0.000 | 3235 | 14.60 | — | 3 | 0.250 |
| tuning | tree-overview-01 | claude-opus-4-7 | 5 | 0.400 | 0.224 | 1627 | 8.40 | synthesist tree show keaton; tree show; tree show keaton | 1 | 0.000 |
| tuning | tree-overview-01 | claude-sonnet-4-6 | 5 | 0.300 | 0.209 | 2925 | 18.00 | spec list --tree keaton; synthesist spec list --tree keaton; synthesist tree show keaton; tree show; tree show keaton | 4 | 0.050 |
