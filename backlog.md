# Backlog

## Near-term follow-ups (from MVP implementation)

### Asset Upload/Download
- During push: upload images from `assets/` to GitHub, rewrite relative links in issue body to GitHub URLs
- During pull: download referenced images to `assets/`, rewrite GitHub URLs back to relative paths
- Requires GitHub file attachment API or repo-based image hosting

### GitHub Sub-Issue Sync
- GitHub issues support sub-issues — map these to the recursive node hierarchy
- During pull: discover sub-issues and create child nodes automatically
- During push: create sub-issue relationships when pushing child nodes

---

## Post-MVP features

### Reports & Dashboards
- Progress summaries per initiative/project
- Timeline views and Gantt-style output
- OKR roll-up reports across quarters

## Multi-Repo Issue Aggregation
- Track issues across multiple GitHub repos for a single node
- Cross-repo search and filtering

## LLM-Based Auto-Triage
- Automatically classify incoming external feature requests and bug reports
- Suggest labels, parent nodes, and initiative/project assignments
- On-demand via CLI command (e.g. `armitage triage`)
- User confirms before applying suggestions

## Database-Backed Discussion Tracking
- Track GitHub issue comments/discussions locally
- Searchable discussion history
- Comment sync (currently only issue body is synced)

## Jira Integration
- Read issues from Jira projects
- Bidirectional sync with Jira (similar to GitHub sync)
- Map Jira statuses to armitage node statuses
