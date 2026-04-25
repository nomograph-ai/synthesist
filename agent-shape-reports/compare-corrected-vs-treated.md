# agent-shape comparison: synthesist vs synthesist

before: `/Users/andrewdunn/gitlab.com/nomograph/synthesist/agent-shape-reports/baseline-v5.1.0-n5.rejudged.json`
after:  `/Users/andrewdunn/gitlab.com/nomograph/synthesist/agent-shape-reports/treated-v0-n5.json`

## Aggregate (tuning battery)

| metric | before | after | delta |
|---|---:|---:|---:|
| mean_score | 0.770 | 0.640 | -0.130 |
| completion_rate | 94.0% | 96.0% | +2.0pp |
| mean_tokens | 2499 | 2276 | -223 |
| mean_turns | 14.18 | 12.76 | -1.42 |
| total_invented | 19 | 44 | +25 |
| total_fallback_to_sql | 5 | 11 | +6 |

## Per-cell deltas

| section | task | model | before | after | delta | invented Δ |
|---|---|---|---:|---:|---:|---:|
| tuning | cross-tree-query-01 | claude-opus-4-7 | 1.00 | 0.85 | -0.15 | -1 |
| tuning | cross-tree-query-01 | claude-sonnet-4-6 | 0.90 | 0.85 | -0.05 | +2 |
| tuning | exploration-01 | claude-opus-4-7 | 0.70 | 0.50 | -0.20 | +4 |
| tuning | exploration-01 | claude-sonnet-4-6 | 0.90 | 0.50 | -0.40 | +3 |
| tuning | resume-work-01 | claude-opus-4-7 | 0.80 | 0.60 | -0.20 | -1 |
| tuning | resume-work-01 | claude-sonnet-4-6 | 0.55 | 0.50 | -0.05 | +2 |
| tuning | session-drill-down-01 | claude-opus-4-7 | 0.55 | 0.90 | +0.35 | +1 |
| tuning | session-drill-down-01 | claude-sonnet-4-6 | 1.00 | 1.00 | +0.00 | +0 |
| tuning | tree-overview-01 | claude-opus-4-7 | 0.50 | 0.40 | -0.10 | +0 |
| tuning | tree-overview-01 | claude-sonnet-4-6 | 0.80 | 0.30 | -0.50 | +4 |
