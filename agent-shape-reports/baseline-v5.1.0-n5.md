# agent-shape report: synthesist

run_timestamp: `unix:1777077964`
judge_model: `claude-haiku-4-5`

## Tuning battery

- n_trials: 50
- mean_score: 0.600
- completion_rate: 96.0%
- mean_tokens: 2499
- mean_turns: 14.18
- total_invented_commands: 80
- total_fallback_to_sql: 7

## Holdout battery

_empty in v1 (schema supports it; corpus deferred)_

## Per-cell breakdown

| section | task | model | n | score | stddev | tokens | turns | invented | fallback | irr_delta |
|---------|------|-------|---|-------|--------|--------|-------|----------|----------|-----------|
| tuning | cross-tree-query-01 | claude-opus-4-7 | 5 | 0.450 | 0.112 | 1844 | 11.20 | synthesist sql; synthesist sql "PRAGMA table_info(claims)" 2>&1; synthesist sql "SELECT * FROM claims LIMIT 1"; synthesist sql "SELECT name FROM sqlite_master WHERE type='table'"; synthesist sql "SELECT name FROM sqlite_master WHERE type='table'" 2>&1; synthesist sql "SELECT tree, name FROM specs WHERE name LIKE '%gkg%' ORDER BY tree, name"; synthesist sql "SELECT tree_id, name FROM specs WHERE name LIKE '%gkg%' ORDER BY tree_id, name" 2>&1 | 2 | 0.050 |
| tuning | cross-tree-query-01 | claude-sonnet-4-6 | 5 | 0.500 | 0.000 | 2199 | 13.80 | synthesist sql; synthesist sql (15 invocations with various SQL queries); synthesist sql (with PRAGMA table_info(claims)...); synthesist sql (with SELECT * FROM claims...); synthesist sql (with SELECT COUNT(*) as total...); synthesist sql (with SELECT DISTINCT claim_type...); synthesist sql (with SELECT id, tree_id, name, status...); synthesist sql (with SELECT name FROM sqlite_master...) | 2 | 0.100 |
| tuning | exploration-01 | claude-opus-4-7 | 5 | 0.700 | 0.447 | 1792 | 8.80 | synthesist phase show; synthesist spec list; synthesist sql | 0 | 0.200 |
| tuning | exploration-01 | claude-sonnet-4-6 | 5 | 0.500 | 0.000 | 4571 | 27.20 | phase get; synthesist phase get; synthesist phase show; synthesist sql | 0 | 0.100 |
| tuning | resume-work-01 | claude-opus-4-7 | 5 | 1.000 | 0.000 | 1321 | 8.40 | phase get --session treatment-design; phase show; phase show --session treatment-design; synthesist phase get --session treatment-design; synthesist phase get --session treatment-design | jq; synthesist phase show --session treatment-design; synthesist phase show --session treatment-design | jq | 0 | 0.200 |
| tuning | resume-work-01 | claude-sonnet-4-6 | 5 | 0.700 | 0.274 | 2029 | 15.00 | phase get; phase get --session treatment-design; phase show; phase show --session treatment-design; sql; synthesist phase --help; synthesist phase get --session treatment-design; synthesist phase list; synthesist phase show; synthesist phase show --session treatment-design; synthesist sql; synthesist task list --spec lever-compliance; synthesist task list keaton/lever-compliance | 0 | 0.200 |
| tuning | session-drill-down-01 | claude-opus-4-7 | 5 | 0.500 | 0.000 | 2019 | 10.00 | synthesist sql; synthesist sql "PRAGMA table_info(claims)"; synthesist sql "SELECT * FROM claims LIMIT 1"; synthesist sql "SELECT id, claim_type, asserted_at, asserted_by, props, supersedes FROM claims WHERE asserted_by LIKE '%lever-audit%' ORDER BY asserted_at"; synthesist sql "SELECT id, claim_type, props, asserted_at, valid_until FROM claims WHERE claim_type='session' AND props LIKE '%lever-audit%'"; synthesist sql "SELECT id, claim_type, props, asserted_by, asserted_at FROM claims WHERE asserted_by LIKE '%lever-audit%' ORDER BY asserted_at"; synthesist sql "SELECT name FROM sqlite_master WHERE type='table'" | 0 | 0.100 |
| tuning | session-drill-down-01 | claude-sonnet-4-6 | 5 | 0.500 | 0.000 | 4458 | 20.80 | synthesist sql | 2 | 0.000 |
| tuning | tree-overview-01 | claude-opus-4-7 | 5 | 0.650 | 0.487 | 1514 | 8.00 | tree --help; tree show; tree show keaton | 1 | 0.250 |
| tuning | tree-overview-01 | claude-sonnet-4-6 | 5 | 0.500 | 0.000 | 3242 | 18.60 | sql; synthesist spec list keaton; synthesist sql | 0 | 0.000 |
