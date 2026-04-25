# agent-shape comparison: synthesist vs synthesist

before: `/Users/andrewdunn/gitlab.com/nomograph/synthesist/agent-shape-reports/baseline-v5.1.0-n5.rejudged.json`
after:  `/Users/andrewdunn/gitlab.com/nomograph/synthesist/agent-shape-reports/treated-v0-n5.rejudged.json`

## Aggregate (tuning battery)

| metric | before | after | delta |
|---|---:|---:|---:|
| mean_score | 0.770 | 0.770 | +0.000 |
| completion_rate | 94.0% | 96.0% | +2.0pp |
| mean_tokens | 2499 | 2276 | -223 |
| mean_turns | 14.18 | 12.76 | -1.42 |
| total_invented | 19 | 17 | -2 |
| total_fallback_to_sql | 5 | 13 | +8 |

## Per-cell deltas

| section | task | model | before | after | delta | invented Δ |
|---|---|---|---:|---:|---:|---:|
| tuning | cross-tree-query-01 | claude-opus-4-7 | 1.00 | 1.00 | +0.00 | -1 |
| tuning | cross-tree-query-01 | claude-sonnet-4-6 | 0.90 | 0.85 | -0.05 | +0 |
| tuning | exploration-01 | claude-opus-4-7 | 0.70 | 0.90 | +0.20 | +7 |
| tuning | exploration-01 | claude-sonnet-4-6 | 0.90 | 0.60 | -0.30 | +0 |
| tuning | resume-work-01 | claude-opus-4-7 | 0.80 | 1.00 | +0.20 | -3 |
| tuning | resume-work-01 | claude-sonnet-4-6 | 0.55 | 0.70 | +0.15 | +1 |
| tuning | session-drill-down-01 | claude-opus-4-7 | 0.55 | 0.70 | +0.15 | +1 |
| tuning | session-drill-down-01 | claude-sonnet-4-6 | 1.00 | 0.70 | -0.30 | +0 |
| tuning | tree-overview-01 | claude-opus-4-7 | 0.50 | 0.70 | +0.20 | -2 |
| tuning | tree-overview-01 | claude-sonnet-4-6 | 0.80 | 0.55 | -0.25 | -1 |
