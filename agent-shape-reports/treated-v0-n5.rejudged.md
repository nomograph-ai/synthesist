# agent-shape report: synthesist

run_timestamp: `unix:1777098113`
judge_model: `claude-haiku-4-5`

## Tuning battery

- n_trials: 50
- mean_score: 0.770
- completion_rate: 96.0%
- mean_tokens: 2276
- mean_turns: 12.76
- total_invented_commands: 17
- total_fallback_to_sql: 13

## Holdout battery

_empty in v1 (schema supports it; corpus deferred)_

## Per-cell breakdown

| section | task | model | n | score | stddev | tokens | turns | invented | fallback | irr_delta |
|---------|------|-------|---|-------|--------|--------|-------|----------|----------|-----------|
| tuning | cross-tree-query-01 | claude-opus-4-7 | 5 | 1.000 | 0.000 | 1891 | 10.40 | — | 0 | 0.000 |
| tuning | cross-tree-query-01 | claude-sonnet-4-6 | 5 | 0.850 | 0.335 | 1543 | 10.20 | — | 2 | 0.150 |
| tuning | exploration-01 | claude-opus-4-7 | 5 | 0.900 | 0.224 | 2244 | 10.80 | synthesist spec list --tree keaton --tree; synthesist spec list --tree keaton --tree-view; synthesist spec list --tree nomograph-release --tree; synthesist spec list --tree nomograph-release --tree-view; synthesist spec list --tree outreach --tree; synthesist spec list --tree outreach --tree-view; synthesist task list keaton/lever-compliance; synthesist task list keaton/tool-surface-conformity | 0 | 0.300 |
| tuning | exploration-01 | claude-sonnet-4-6 | 5 | 0.600 | 0.379 | 3481 | 23.80 | synthesist phase get --session treatment-design; synthesist spec list --tree | 2 | 0.400 |
| tuning | resume-work-01 | claude-opus-4-7 | 5 | 1.000 | 0.000 | 1061 | 7.00 | — | 0 | 0.000 |
| tuning | resume-work-01 | claude-sonnet-4-6 | 5 | 0.700 | 0.447 | 2075 | 13.60 | phase get --session treatment-design; session list --all; session list --closed; task list --session treatment-design | 0 | 0.200 |
| tuning | session-drill-down-01 | claude-opus-4-7 | 5 | 0.700 | 0.411 | 2679 | 10.80 | synthesist session list --all | 3 | 0.150 |
| tuning | session-drill-down-01 | claude-sonnet-4-6 | 5 | 0.700 | 0.411 | 3235 | 14.60 | — | 3 | 0.350 |
| tuning | tree-overview-01 | claude-opus-4-7 | 5 | 0.700 | 0.447 | 1627 | 8.40 | synthesist --session=read-only tree show keaton | 0 | 0.000 |
| tuning | tree-overview-01 | claude-sonnet-4-6 | 5 | 0.550 | 0.411 | 2925 | 18.00 | — | 3 | 0.350 |
