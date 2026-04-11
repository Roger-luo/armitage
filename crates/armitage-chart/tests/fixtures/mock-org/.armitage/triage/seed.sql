-- Schema matches armitage-triage db.rs SCHEMA_V8
PRAGMA user_version = 8;

CREATE TABLE IF NOT EXISTS issues (
    id                INTEGER PRIMARY KEY,
    repo              TEXT NOT NULL,
    number            INTEGER NOT NULL,
    title             TEXT NOT NULL,
    body              TEXT NOT NULL DEFAULT '',
    state             TEXT NOT NULL,
    labels_json       TEXT NOT NULL DEFAULT '[]',
    updated_at        TEXT NOT NULL,
    fetched_at        TEXT NOT NULL,
    sub_issues_count  INTEGER NOT NULL DEFAULT 0,
    author            TEXT NOT NULL DEFAULT '',
    assignees_json    TEXT NOT NULL DEFAULT '[]',
    UNIQUE(repo, number)
);

CREATE TABLE IF NOT EXISTS triage_suggestions (
    id                        INTEGER PRIMARY KEY,
    issue_id                  INTEGER NOT NULL REFERENCES issues(id),
    suggested_node            TEXT,
    suggested_labels          TEXT NOT NULL DEFAULT '[]',
    confidence                REAL,
    reasoning                 TEXT NOT NULL DEFAULT '',
    llm_backend               TEXT NOT NULL,
    created_at                TEXT NOT NULL,
    is_tracking_issue         INTEGER NOT NULL DEFAULT 0,
    suggested_new_categories  TEXT NOT NULL DEFAULT '[]',
    is_stale                  INTEGER NOT NULL DEFAULT 0,
    UNIQUE(issue_id)
);

CREATE TABLE IF NOT EXISTS review_decisions (
    id            INTEGER PRIMARY KEY,
    suggestion_id INTEGER NOT NULL REFERENCES triage_suggestions(id),
    decision      TEXT NOT NULL,
    final_node    TEXT,
    final_labels  TEXT NOT NULL DEFAULT '[]',
    decided_at    TEXT NOT NULL,
    applied_at    TEXT,
    question      TEXT NOT NULL DEFAULT '',
    UNIQUE(suggestion_id)
);

CREATE TABLE IF NOT EXISTS issue_project_items (
    id           INTEGER PRIMARY KEY,
    issue_id     INTEGER NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    project_url  TEXT NOT NULL,
    target_date  TEXT,
    start_date   TEXT,
    status       TEXT,
    fetched_at   TEXT NOT NULL,
    UNIQUE(issue_id, project_url)
);

-- Issues: aurora initiative
INSERT INTO issues (id, repo, number, title, body, state, updated_at, fetched_at, assignees_json) VALUES
(1,  'NexusLabs/photon-core', 101, 'Implement channel multiplexer', '', 'OPEN', '2026-03-01', '2026-03-15', '["agarcia"]'),
(2,  'NexusLabs/photon-core', 102, 'Add throughput monitoring dashboard', '', 'OPEN', '2026-03-01', '2026-03-15', '["bkim","czhang"]'),
(3,  'NexusLabs/photon-core', 103, 'Constant folding optimization', '', 'OPEN', '2026-02-01', '2026-03-15', '[]'),
(4,  'NexusLabs/photon-core', 104, 'Loop unrolling pass', '', 'OPEN', '2026-02-15', '2026-03-15', '[]'),
(5,  'NexusLabs/photon-core', 105, 'Dead code elimination', '', 'CLOSED', '2026-03-01', '2026-03-15', '[]'),
(6,  'NexusLabs/photon-core', 106, 'Vectorization pass for batch operations', '', 'OPEN', '2026-03-01', '2026-03-15', '[]'),
(7,  'NexusLabs/photon-core', 107, 'Memory layout optimizer', '', 'OPEN', '2026-04-01', '2026-04-15', '[]'),
(8,  'NexusLabs/photon-core', 108, 'Priority queue implementation', '', 'OPEN', '2026-04-01', '2026-04-15', '[]'),
(9,  'NexusLabs/photon-core', 109, 'Deadline-aware preemption', '', 'OPEN', '2026-04-15', '2026-04-15', '[]'),
(10, 'NexusLabs/photon-core', 110, 'Worker pool scaling', '', 'OPEN', '2026-05-01', '2026-05-15', '[]'),
(11, 'NexusLabs/photon-core', 111, 'Format adapter for WAV files', '', 'OPEN', '2026-02-01', '2026-03-15', '[]'),
(12, 'NexusLabs/photon-core', 112, 'MATLAB bridge integration', '', 'OPEN', '2026-02-15', '2026-03-15', '[]'),
(13, 'NexusLabs/photon-core', 113, 'HDF5 reader implementation', '', 'OPEN', '2026-03-01', '2026-03-15', '[]'),
(14, 'NexusLabs/photon-core', 114, 'CSV streaming parser', '', 'CLOSED', '2026-03-15', '2026-03-15', '[]'),
(15, 'NexusLabs/photon-core', 115, 'Protocol buffer serialization', '', 'OPEN', '2026-04-01', '2026-04-15', '[]'),
(16, 'NexusLabs/photon-core', 116, 'Apache Arrow interop', '', 'OPEN', '2026-04-15', '2026-04-15', '[]'),
(17, 'NexusLabs/photon-core', 117, 'JSON schema validation', '', 'OPEN', '2026-05-01', '2026-05-15', '[]'),
(18, 'NexusLabs/photon-core', 118, 'Binary format documentation', '', 'OPEN', '2026-05-15', '2026-05-15', '[]');

-- Issues: beacon initiative
INSERT INTO issues (id, repo, number, title, body, state, updated_at, fetched_at, assignees_json) VALUES
(19, 'NexusLabs/waveform-engine', 201, 'Ring buffer implementation', '', 'OPEN', '2026-01-15', '2026-02-01', '[]'),
(20, 'NexusLabs/waveform-engine', 202, 'Sliding window aggregator', '', 'OPEN', '2026-02-01', '2026-02-15', '[]'),
(21, 'NexusLabs/waveform-engine', 203, 'Sorted merge iterator', '', 'OPEN', '2026-02-15', '2026-03-01', '[]'),
(22, 'NexusLabs/waveform-engine', 204, 'Concurrent hash map', '', 'CLOSED', '2026-03-01', '2026-03-15', '[]'),
(23, 'NexusLabs/waveform-engine', 205, 'Future combinator library', '', 'OPEN', '2026-03-15', '2026-04-01', '[]'),
(24, 'NexusLabs/waveform-engine', 206, 'Async channel implementation', '', 'OPEN', '2026-04-01', '2026-04-15', '[]'),
(25, 'NexusLabs/waveform-engine', 207, 'Backpressure mechanism', '', 'OPEN', '2026-04-15', '2026-05-01', '[]');

-- Issues: compass initiative
INSERT INTO issues (id, repo, number, title, body, state, updated_at, fetched_at, assignees_json) VALUES
(26, 'Nexus-QC/test-infra', 301, 'Multi-node test harness', '', 'OPEN', '2026-04-01', '2026-04-15', '[]'),
(27, 'Nexus-QC/test-infra', 302, 'Network partition simulator', '', 'OPEN', '2026-04-15', '2026-05-01', '[]'),
(28, 'Nexus-QC/test-infra', 303, 'Chaos testing framework', '', 'CLOSED', '2026-05-01', '2026-05-15', '[]');

-- Issues: echo initiative
INSERT INTO issues (id, repo, number, title, body, state, updated_at, fetched_at, assignees_json) VALUES
(29, 'NexusLabs/photon-core', 119, 'Anomaly detection algorithm', '', 'OPEN', '2026-06-01', '2026-06-15', '[]'),
(30, 'NexusLabs/photon-core', 120, 'Metrics aggregation pipeline', '', 'OPEN', '2026-06-15', '2026-07-01', '[]');

-- Project items: on-track issues (target_date within node timeline)
INSERT INTO issue_project_items (issue_id, project_url, start_date, target_date, status, fetched_at) VALUES
(1,  'https://github.com/orgs/NexusLabs/projects/1', '2026-01-15', '2026-05-31', 'IN_PROGRESS', '2026-03-15'),
(2,  'https://github.com/orgs/NexusLabs/projects/1', '2026-02-01', '2026-08-31', 'IN_PROGRESS', '2026-03-15'),
(3,  'https://github.com/orgs/NexusLabs/projects/1', '2026-01-15', '2026-04-30', 'IN_PROGRESS', '2026-03-15'),
(4,  'https://github.com/orgs/NexusLabs/projects/1', '2026-02-01', '2026-05-31', 'IN_PROGRESS', '2026-03-15'),
(6,  'https://github.com/orgs/NexusLabs/projects/1', '2026-03-15', '2026-06-15', 'IN_PROGRESS', '2026-03-15'),
(8,  'https://github.com/orgs/NexusLabs/projects/1', '2026-04-15', '2026-07-31', 'IN_PROGRESS', '2026-04-15'),
(9,  'https://github.com/orgs/NexusLabs/projects/1', '2026-05-01', '2026-08-31', 'IN_PROGRESS', '2026-04-15'),
(11, 'https://github.com/orgs/NexusLabs/projects/1', '2026-02-15', '2026-05-31', 'IN_PROGRESS', '2026-03-15'),
(12, 'https://github.com/orgs/NexusLabs/projects/1', '2026-03-01', '2026-06-30', 'IN_PROGRESS', '2026-03-15'),
(13, 'https://github.com/orgs/NexusLabs/projects/1', '2026-03-15', '2026-07-31', 'IN_PROGRESS', '2026-03-15'),
(15, 'https://github.com/orgs/NexusLabs/projects/1', '2026-04-15', '2026-08-15', 'IN_PROGRESS', '2026-04-15'),
(16, 'https://github.com/orgs/NexusLabs/projects/1', '2026-05-01', '2026-08-31', 'IN_PROGRESS', '2026-04-15'),
(19, 'https://github.com/orgs/NexusLabs/projects/1', '2026-01-15', '2026-04-30', 'IN_PROGRESS', '2026-02-01'),
(20, 'https://github.com/orgs/NexusLabs/projects/1', '2026-02-15', '2026-06-30', 'IN_PROGRESS', '2026-02-15'),
(21, 'https://github.com/orgs/NexusLabs/projects/1', '2026-03-01', '2026-07-31', 'IN_PROGRESS', '2026-03-01'),
(23, 'https://github.com/orgs/NexusLabs/projects/1', '2026-04-01', '2026-08-31', 'IN_PROGRESS', '2026-04-01'),
(24, 'https://github.com/orgs/NexusLabs/projects/1', '2026-04-15', '2026-09-30', 'IN_PROGRESS', '2026-04-15'),
(26, 'https://github.com/orgs/NexusLabs/projects/1', '2026-04-15', '2026-08-31', 'IN_PROGRESS', '2026-04-15'),
(27, 'https://github.com/orgs/NexusLabs/projects/1', '2026-05-01', '2026-09-30', 'IN_PROGRESS', '2026-05-01'),
(29, 'https://github.com/orgs/NexusLabs/projects/1', '2026-06-15', '2026-10-31', 'IN_PROGRESS', '2026-06-15');

-- Overdue issues (target_date exceeds parent node end)
-- #7: optimizer node end 2026-06-30, target +3 days overdue
INSERT INTO issue_project_items (issue_id, project_url, start_date, target_date, status, fetched_at) VALUES
(7,  'https://github.com/orgs/NexusLabs/projects/1', '2026-04-01', '2026-07-03', 'IN_PROGRESS', '2026-04-15');
-- #10: scheduler node end 2026-09-30, target +10 days overdue
INSERT INTO issue_project_items (issue_id, project_url, start_date, target_date, status, fetched_at) VALUES
(10, 'https://github.com/orgs/NexusLabs/projects/1', '2026-05-15', '2026-10-10', 'IN_PROGRESS', '2026-05-15');
-- #17: extern node end 2026-08-31, target +3 weeks overdue
INSERT INTO issue_project_items (issue_id, project_url, start_date, target_date, status, fetched_at) VALUES
(17, 'https://github.com/orgs/NexusLabs/projects/1', '2026-05-15', '2026-09-21', 'IN_PROGRESS', '2026-05-15');
-- #18: extern node end 2026-08-31, target +8 weeks overdue
INSERT INTO issue_project_items (issue_id, project_url, start_date, target_date, status, fetched_at) VALUES
(18, 'https://github.com/orgs/NexusLabs/projects/1', '2026-06-01', '2026-10-26', 'IN_PROGRESS', '2026-05-15');
-- #25: async node end 2026-12-31, target +3 weeks overdue
INSERT INTO issue_project_items (issue_id, project_url, start_date, target_date, status, fetched_at) VALUES
(25, 'https://github.com/orgs/NexusLabs/projects/1', '2026-05-01', '2027-01-21', 'IN_PROGRESS', '2026-05-01');
-- #30: analysis node end 2026-12-31, target +10 days overdue
INSERT INTO issue_project_items (issue_id, project_url, start_date, target_date, status, fetched_at) VALUES
(30, 'https://github.com/orgs/NexusLabs/projects/1', '2026-07-01', '2027-01-10', 'IN_PROGRESS', '2026-07-01');
