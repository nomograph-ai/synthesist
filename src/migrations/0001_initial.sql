-- synthesist v1.0.0 initial schema (16 tables)

-- Estate
CREATE TABLE IF NOT EXISTS trees (
    name    TEXT PRIMARY KEY,
    status  TEXT NOT NULL DEFAULT 'active',
    description TEXT NOT NULL DEFAULT ''
);

-- Specs
CREATE TABLE IF NOT EXISTS specs (
    tree        TEXT NOT NULL,
    id          TEXT NOT NULL,
    goal        TEXT,
    constraints TEXT,
    decisions   TEXT,
    status      TEXT NOT NULL DEFAULT 'active',
    outcome     TEXT,
    created     TEXT,
    PRIMARY KEY (tree, id),
    FOREIGN KEY (tree) REFERENCES trees(name)
);

-- Task DAG
CREATE TABLE IF NOT EXISTS tasks (
    tree         TEXT NOT NULL,
    spec         TEXT NOT NULL,
    id           TEXT NOT NULL,
    summary      TEXT NOT NULL,
    description  TEXT,
    status       TEXT NOT NULL DEFAULT 'pending',
    gate         TEXT,
    owner        TEXT,
    created      TEXT NOT NULL,
    completed    TEXT,
    failure_note TEXT,
    wait_reason  TEXT,
    PRIMARY KEY (tree, spec, id),
    FOREIGN KEY (tree, spec) REFERENCES specs(tree, id)
);

CREATE TABLE IF NOT EXISTS task_deps (
    tree       TEXT NOT NULL,
    spec       TEXT NOT NULL,
    task_id    TEXT NOT NULL,
    depends_on TEXT NOT NULL,
    PRIMARY KEY (tree, spec, task_id, depends_on),
    FOREIGN KEY (tree, spec, task_id) REFERENCES tasks(tree, spec, id),
    FOREIGN KEY (tree, spec, depends_on) REFERENCES tasks(tree, spec, id)
);

CREATE TABLE IF NOT EXISTS task_files (
    tree    TEXT NOT NULL,
    spec    TEXT NOT NULL,
    task_id TEXT NOT NULL,
    path    TEXT NOT NULL,
    PRIMARY KEY (tree, spec, task_id, path),
    FOREIGN KEY (tree, spec, task_id) REFERENCES tasks(tree, spec, id)
);

CREATE TABLE IF NOT EXISTS acceptance (
    tree      TEXT NOT NULL,
    spec      TEXT NOT NULL,
    task_id   TEXT NOT NULL,
    seq       INTEGER NOT NULL,
    criterion TEXT NOT NULL,
    verify_cmd TEXT NOT NULL,
    PRIMARY KEY (tree, spec, task_id, seq),
    FOREIGN KEY (tree, spec, task_id) REFERENCES tasks(tree, spec, id)
);

-- Discoveries
CREATE TABLE IF NOT EXISTS discoveries (
    tree    TEXT NOT NULL,
    spec    TEXT NOT NULL,
    id      TEXT NOT NULL,
    date    TEXT NOT NULL,
    author  TEXT,
    finding TEXT NOT NULL,
    impact  TEXT,
    action  TEXT,
    PRIMARY KEY (tree, spec, id)
);

-- Disposition graph
CREATE TABLE IF NOT EXISTS stakeholders (
    tree    TEXT NOT NULL,
    id      TEXT NOT NULL,
    name    TEXT,
    context TEXT NOT NULL,
    PRIMARY KEY (tree, id)
);

CREATE TABLE IF NOT EXISTS stakeholder_orgs (
    tree           TEXT NOT NULL,
    stakeholder_id TEXT NOT NULL,
    org            TEXT NOT NULL,
    PRIMARY KEY (tree, stakeholder_id, org),
    FOREIGN KEY (tree, stakeholder_id) REFERENCES stakeholders(tree, id)
);

CREATE TABLE IF NOT EXISTS dispositions (
    tree               TEXT NOT NULL,
    spec               TEXT NOT NULL,
    id                 TEXT NOT NULL,
    stakeholder_id     TEXT NOT NULL,
    topic              TEXT NOT NULL,
    stance             TEXT NOT NULL,
    preferred_approach TEXT,
    detail             TEXT,
    confidence         TEXT NOT NULL,
    valid_from         TEXT NOT NULL,
    valid_until        TEXT,
    superseded_by      TEXT,
    PRIMARY KEY (tree, spec, id),
    FOREIGN KEY (tree, stakeholder_id) REFERENCES stakeholders(tree, id)
);

CREATE TABLE IF NOT EXISTS signals (
    tree           TEXT NOT NULL,
    spec           TEXT NOT NULL,
    id             TEXT NOT NULL,
    stakeholder_id TEXT NOT NULL,
    date           TEXT NOT NULL,
    recorded_date  TEXT NOT NULL,
    source         TEXT NOT NULL,
    source_type    TEXT NOT NULL,
    content        TEXT NOT NULL,
    interpretation TEXT,
    our_action     TEXT,
    PRIMARY KEY (tree, spec, id),
    FOREIGN KEY (tree, stakeholder_id) REFERENCES stakeholders(tree, id)
);

-- Campaigns
CREATE TABLE IF NOT EXISTS campaign_active (
    tree    TEXT NOT NULL,
    spec_id TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    phase   TEXT,
    PRIMARY KEY (tree, spec_id)
);

CREATE TABLE IF NOT EXISTS campaign_backlog (
    tree    TEXT NOT NULL,
    spec_id TEXT NOT NULL,
    title   TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (tree, spec_id)
);

CREATE TABLE IF NOT EXISTS campaign_blocked_by (
    tree       TEXT NOT NULL,
    spec_id    TEXT NOT NULL,
    blocked_by TEXT NOT NULL,
    PRIMARY KEY (tree, spec_id, blocked_by)
);

-- Sessions
CREATE TABLE IF NOT EXISTS session_meta (
    id      TEXT PRIMARY KEY,
    started TEXT NOT NULL,
    owner   TEXT,
    tree    TEXT,
    spec    TEXT,
    summary TEXT,
    status  TEXT NOT NULL DEFAULT 'active'
);

-- Workflow
CREATE TABLE IF NOT EXISTS phase (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    name TEXT NOT NULL DEFAULT 'orient'
);

INSERT OR IGNORE INTO phase (id, name) VALUES (1, 'orient');

-- Config
CREATE TABLE IF NOT EXISTS config (
    key_name TEXT PRIMARY KEY,
    value    TEXT NOT NULL
);

INSERT OR IGNORE INTO config (key_name, value) VALUES ('schema_version', '1');
INSERT OR IGNORE INTO config (key_name, value) VALUES ('auto_commit', 'true');

-- Performance indexes
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_owner ON tasks(owner) WHERE owner IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_dispositions_stakeholder ON dispositions(tree, stakeholder_id, valid_until);
CREATE INDEX IF NOT EXISTS idx_signals_stakeholder ON signals(tree, stakeholder_id);
