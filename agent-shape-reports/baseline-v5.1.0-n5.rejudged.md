# agent-shape report: synthesist

run_timestamp: `unix:1777084824`
judge_model: `claude-haiku-4-5`

## Tuning battery

- n_trials: 50
- mean_score: 0.770
- completion_rate: 94.0%
- mean_tokens: 2499
- mean_turns: 14.18
- total_invented_commands: 19
- total_fallback_to_sql: 5

## Holdout battery

_empty in v1 (schema supports it; corpus deferred)_

## Per-cell breakdown

| section | task | model | n | score | stddev | tokens | turns | invented | fallback | irr_delta |
|---------|------|-------|---|-------|--------|--------|-------|----------|----------|-----------|
| tuning | cross-tree-query-01 | claude-opus-4-7 | 5 | 1.000 | 0.000 | 1844 | 11.20 | synthesist spec list --tree "$tree" | 0 | 0.100 |
| tuning | cross-tree-query-01 | claude-sonnet-4-6 | 5 | 0.900 | 0.224 | 2199 | 13.80 | — | 1 | 0.300 |
| tuning | exploration-01 | claude-opus-4-7 | 5 | 0.700 | 0.447 | 1792 | 8.80 | synthesist spec list (without tree argument, commands 4 and 5) | 0 | 0.200 |
| tuning | exploration-01 | claude-sonnet-4-6 | 5 | 0.900 | 0.224 | 4571 | 27.20 | ./synthesist phase get; phase get | 0 | 0.100 |
| tuning | resume-work-01 | claude-opus-4-7 | 5 | 0.800 | 0.274 | 1321 | 8.40 | phase get; phase get --session treatment-design; synthesist phase get --session treatment-design | 0 | 0.100 |
| tuning | resume-work-01 | claude-sonnet-4-6 | 5 | 0.550 | 0.274 | 2029 | 15.00 | phase get; phase list; synthesist phase get --session treatment-design | 1 | 0.150 |
| tuning | session-drill-down-01 | claude-opus-4-7 | 5 | 0.550 | 0.411 | 2019 | 10.00 | — | 3 | 0.200 |
| tuning | session-drill-down-01 | claude-sonnet-4-6 | 5 | 1.000 | 0.000 | 4458 | 20.80 | — | 0 | 0.200 |
| tuning | tree-overview-01 | claude-opus-4-7 | 5 | 0.500 | 0.500 | 1514 | 8.00 | spec list --tree; tree --help; tree show | 0 | 0.150 |
| tuning | tree-overview-01 | claude-sonnet-4-6 | 5 | 0.800 | 0.274 | 3242 | 18.60 | spec list --tree keaton | 0 | 0.150 |
