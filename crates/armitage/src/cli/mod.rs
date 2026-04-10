pub mod chart;
pub mod complete;
pub mod config;
pub mod init;
pub mod milestone;
pub mod node;
pub mod pull;
pub mod push;
pub mod resolve;
pub mod status;
pub mod triage;

use crate::error::Result;
use clap::{Parser, Subcommand};

const SKILL_MD: &str = include_str!(concat!(env!("OUT_DIR"), "/SKILL.md"));

#[derive(Parser)]
#[command(
    name = "armitage",
    about = "CLI for project management across GitHub repositories"
)]
struct Cli {
    /// Enable verbose logging (-v for debug, -vv for trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new org directory
    Init {
        name: String,
        #[arg(long)]
        github_org: Vec<String>,
        /// Default repo for issues (e.g. owner/repo)
        #[arg(long)]
        default_repo: Option<String>,
    },
    /// Manage nodes (initiatives, projects, issues)
    Node {
        #[command(subcommand)]
        command: NodeCommands,
    },
    /// Manage milestones
    Milestone {
        #[command(subcommand)]
        command: MilestoneCommands,
    },
    /// Pull changes from GitHub
    Pull {
        path: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Push changes to GitHub
    Push {
        path: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Resolve conflicts
    Resolve {
        path: Option<String>,
        #[arg(long)]
        list: bool,
    },
    /// Show org status
    Status,
    /// View or update org configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Triage GitHub issues with LLM assistance
    Triage {
        #[command(subcommand)]
        command: TriageCommands,
    },
    /// Generate an interactive HTML roadmap chart
    Chart {
        /// Output file path (default: .armitage/chart.html)
        #[arg(long, short)]
        output: Option<String>,
        /// Don't open the chart in the browser after generating
        #[arg(long)]
        no_open: bool,
        /// Embed ECharts JS inline for offline viewing
        #[arg(long)]
        offline: bool,
    },
    /// Self-management commands
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommands,
    },
}

#[derive(Subcommand)]
enum NodeCommands {
    /// Create a new node (interactive when no options given)
    New {
        /// Node path (e.g. backend/auth). Omit for interactive mode.
        path: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        github_issue: Option<String>,
        #[arg(long)]
        labels: Option<String>,
        #[arg(long)]
        repos: Option<String>,
        #[arg(long)]
        owners: Option<String>,
        #[arg(long)]
        status: Option<String>,
        /// Timeline (e.g. "2025-01-01 to 2025-12-31")
        #[arg(long)]
        timeline: Option<String>,
    },
    /// List nodes
    List {
        path: Option<String>,
        #[arg(long, short)]
        recursive: bool,
    },
    /// Show node details
    Show { path: String },
    /// Edit node interactively
    Edit { path: String },
    /// Move a node
    Move { from: String, to: String },
    /// Merge a node into another (reassigns suggestions, moves children, removes source)
    Merge {
        /// Source node to merge from (will be removed)
        from: String,
        /// Target node to merge into (will keep)
        to: String,
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Remove a node
    Remove {
        path: String,
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Set fields on a node without interactive editing
    Set {
        path: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        triage_hint: Option<String>,
        #[arg(long)]
        owners: Option<String>,
        #[arg(long)]
        team: Option<String>,
        #[arg(long)]
        repos: Option<String>,
        #[arg(long)]
        labels: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Display indented tree of all nodes
    Tree {
        /// Maximum depth to display (0 = top-level only, omit for unlimited)
        #[arg(long, short)]
        depth: Option<usize>,
    },
    /// Format node.toml files (re-serialize with multi-line strings)
    Fmt {
        /// Specific node paths to format (omit for all nodes)
        paths: Vec<String>,
    },
    /// Check for timeline violations and other issues
    Check,
}

#[derive(Subcommand)]
enum MilestoneCommands {
    /// Add a milestone to a node
    Add {
        node_path: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        date: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, default_value = "checkpoint")]
        milestone_type: String,
        #[arg(long)]
        expected_progress: Option<f64>,
        #[arg(long)]
        github_issue: Option<String>,
    },
    /// List milestones
    List {
        node_path: Option<String>,
        #[arg(long)]
        milestone_type: Option<String>,
        #[arg(long)]
        quarter: Option<String>,
    },
    /// Remove a milestone
    Remove { node_path: String, name: String },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Set a config value (e.g. org.default_repo, triage.backend, triage.model, triage.effort)
    Set {
        /// Config key (dot notation)
        key: String,
        /// Value to set (use "" to clear)
        value: String,
    },
    /// Store a secret in .armitage/secrets.toml (e.g. gemini-api-key)
    SetSecret {
        /// Secret name
        name: String,
    },
    /// Show current configuration
    Show,
}

#[derive(Subcommand)]
enum SelfCommands {
    /// Print the embedded SKILL.md
    Skill,
    /// Show version and build info
    Info,
    /// Check for a newer version
    Check,
    /// Update to a newer version
    Update { version: Option<String> },
}

#[derive(Subcommand)]
enum TriageCommands {
    /// Fetch issues from GitHub repos into local database
    Fetch {
        #[arg(long)]
        repo: Vec<String>,
        #[arg(long)]
        since: Option<String>,
    },
    /// Import and curate labels for triage
    Labels {
        #[command(subcommand)]
        command: TriageLabelCommands,
    },
    /// Run LLM classification on untriaged issues
    Classify {
        /// LLM backend: "claude", "codex", or "gemini" (overrides [triage].backend in armitage.toml)
        #[arg(long)]
        backend: Option<String>,
        /// Model to use (e.g. "sonnet", "o3", "gemini-2.5-flash") (overrides [triage].model in armitage.toml)
        #[arg(long)]
        model: Option<String>,
        /// Effort level (overrides [triage].effort in armitage.toml)
        #[arg(long)]
        effort: Option<String>,
        /// Number of issues per LLM call (>1 sends multiple issues in one prompt)
        #[arg(long, default_value_t = 10)]
        batch_size: usize,
        /// Number of concurrent LLM calls
        #[arg(long, default_value_t = 1)]
        parallel: usize,
        /// Maximum number of untriaged issues to classify in this run (default: all)
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        repo: Option<String>,
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Review LLM suggestions
    Review {
        #[arg(long, short)]
        interactive: bool,
        #[arg(long)]
        list: bool,
        #[arg(long)]
        auto_approve: Option<f64>,
        /// Only show suggestions with confidence >= this value (0.0-1.0)
        #[arg(long)]
        min_confidence: Option<f64>,
        /// Only show suggestions with confidence <= this value (0.0-1.0)
        #[arg(long)]
        max_confidence: Option<f64>,
        /// Output format: "table" (default) or "json" (only used with --list)
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Push approved label changes to GitHub
    Apply {
        #[arg(long)]
        dry_run: bool,
    },
    /// Submit a review decision for one or more issues (non-interactive)
    Decide {
        /// Issue references (owner/repo#number), one or more
        #[arg(required = true)]
        issue_refs: Vec<String>,
        /// Decision: approve, reject, modify, stale, or inquire
        #[arg(long)]
        decision: String,
        /// Override the suggested node (only with --decision modify)
        #[arg(long)]
        node: Option<String>,
        /// Override the suggested labels, comma-separated (only with --decision modify)
        #[arg(long)]
        labels: Option<String>,
        /// Optional note explaining the decision
        #[arg(long)]
        note: Option<String>,
        /// Clarification question to post (with --decision inquire or --decision stale)
        #[arg(long)]
        question: Option<String>,
    },
    /// Reset (delete) suggestions so issues can be re-classified
    Reset {
        /// Delete suggestions with confidence below this value (0.0-1.0)
        #[arg(long, group = "reset_mode")]
        below: Option<f64>,
        /// Delete suggestions where the suggested node matches or is under this path
        #[arg(long, group = "reset_mode")]
        node: Option<String>,
        /// Delete the suggestion for a specific issue (owner/repo#number)
        #[arg(long, group = "reset_mode")]
        issue: Option<String>,
        /// Delete ALL suggestions
        #[arg(long, group = "reset_mode")]
        all: bool,
        /// Delete unreviewed and rejected suggestions (keep approved/modified ones)
        #[arg(long, group = "reset_mode")]
        unreviewed: bool,
    },
    /// Show triage pipeline status
    Status {
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// List review decisions with filtering
    Decisions {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        unapplied: bool,
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Manage suggested new categories
    Categories {
        #[command(subcommand)]
        command: TriageCategoryCommands,
    },
    /// Manage classification examples (few-shot learning from past decisions)
    Examples {
        #[command(subcommand)]
        command: TriageExampleCommands,
    },
    /// Show classification summary (confidence distribution, node breakdown, suggested categories)
    Summary {
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// List triage suggestions with filtering
    Suggestions {
        /// Filter by issue number(s), comma-separated (e.g. "247,276,32")
        #[arg(long, value_delimiter = ',')]
        issues: Vec<i64>,
        /// Filter by node path prefix (e.g. "flair" matches flair/*)
        #[arg(long)]
        node: Option<String>,
        /// Filter by source repo
        #[arg(long)]
        repo: Option<String>,
        /// Minimum confidence (0.0-1.0)
        #[arg(long)]
        min_confidence: Option<f64>,
        /// Maximum confidence (0.0-1.0)
        #[arg(long)]
        max_confidence: Option<f64>,
        /// Pipeline state: pending, approved, rejected, applied
        #[arg(long)]
        status: Option<String>,
        /// Only show tracking issues
        #[arg(long)]
        tracking_only: bool,
        /// Only show suggestions with no node
        #[arg(long)]
        unclassified: bool,
        /// Only show stale issues
        #[arg(long)]
        stale_only: bool,
        /// Sort by: confidence (default), node, repo
        #[arg(long, default_value = "confidence")]
        sort: String,
        /// Max rows (default 50, 0 = unlimited)
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Output format: "table" (default), "json", or "summary" (grouped by confidence)
        #[arg(long, default_value = "table")]
        format: String,
        /// Truncate issue body in JSON output (default: 500 chars, 0 = unlimited)
        #[arg(long, default_value_t = 500)]
        body_max: usize,
    },
}

#[derive(Subcommand)]
enum TriageCategoryCommands {
    /// List suggested new categories from classification
    List {
        /// Minimum vote count to show (default: 1)
        #[arg(long, default_value_t = 1)]
        min_votes: usize,
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Create a node from a suggested category and reset for reclassification
    Apply {
        /// Category path (e.g. "circuit/emulator")
        path: String,
        /// Display name (required)
        #[arg(long)]
        name: String,
        /// Description (required)
        #[arg(long)]
        description: String,
        /// Immediately reclassify affected issues
        #[arg(long)]
        reclassify: bool,
        /// LLM backend for reclassification
        #[arg(long)]
        reclassify_backend: Option<String>,
        /// Model for reclassification
        #[arg(long)]
        reclassify_model: Option<String>,
    },
    /// Dismiss a suggested category so it no longer appears in listings
    Dismiss {
        /// Category path to dismiss
        path: String,
    },
    /// Consolidate raw category suggestions via LLM and interactively apply
    Refine {
        /// LLM backend (overrides config)
        #[arg(long)]
        backend: Option<String>,
        /// Model (overrides config)
        #[arg(long)]
        model: Option<String>,
        /// Skip interactive review, apply all recommendations
        #[arg(long)]
        auto_accept: bool,
        /// Minimum vote count to include (default: 2)
        #[arg(long, default_value_t = 2)]
        min_votes: usize,
    },
}

#[derive(Subcommand)]
enum TriageExampleCommands {
    /// List current classification examples
    List,
    /// Export reviewed decisions (rejected/modified) as examples
    Export {
        /// Export only decisions with this status (default: rejected,modified)
        #[arg(long)]
        status: Option<String>,
        /// Maximum number of examples to export
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Remove an example by issue reference
    Remove {
        /// Issue reference (e.g. "owner/repo#123")
        issue_ref: String,
    },
}

#[derive(Subcommand)]
enum TriageLabelCommands {
    /// Fetch labels from one or more GitHub repos into a staged import session
    Fetch {
        #[arg(long)]
        repo: Vec<String>,
        /// Fetch from all non-archived repos in the configured github_orgs
        #[arg(long)]
        org: bool,
    },
    /// Merge a staged label import session into labels.toml
    Merge {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        all_new: bool,
        #[arg(long)]
        update_drifted: bool,
        #[arg(long)]
        name: Vec<String>,
        #[arg(long)]
        exclude_name: Vec<String>,
        #[arg(long)]
        prefer_repo: Option<String>,
        #[arg(long)]
        yes: bool,
        /// Skip LLM-based label reconciliation
        #[arg(long)]
        no_llm: bool,
        /// Auto-accept the LLM's recommended choice for each merge group (non-interactive)
        #[arg(long)]
        auto_accept: bool,
        /// Override LLM backend for reconciliation
        #[arg(long)]
        backend: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
    },
    /// Push pending label renames to GitHub repos
    Sync {
        #[arg(long)]
        repo: Vec<String>,
        /// Sync to all non-archived repos in the configured github_orgs
        #[arg(long)]
        org: bool,
        #[arg(long)]
        dry_run: bool,
        /// Remove renames from ledger once synced to all targeted repos
        #[arg(long)]
        prune: bool,
    },
    /// Push labels.toml to GitHub repos (create, update, optionally delete)
    Push {
        #[arg(long)]
        repo: Vec<String>,
        /// Push to all non-archived repos in the configured github_orgs
        #[arg(long)]
        org: bool,
        #[arg(long)]
        dry_run: bool,
        /// Delete labels on remote repos that are not in labels.toml
        #[arg(long)]
        delete_extra: bool,
    },
}

fn run_self(command: SelfCommands) {
    let manager = ionem::self_update::SelfManager::new(
        "user/armitage",
        "armitage",
        "v",
        env!("CARGO_PKG_VERSION"),
        env!("TARGET"),
    );
    match command {
        SelfCommands::Skill => print!("{SKILL_MD}"),
        SelfCommands::Info => manager.print_info(),
        SelfCommands::Check => {
            if let Err(e) = manager.print_check() {
                eprintln!("error: {e}");
            }
        }
        SelfCommands::Update { version } => {
            if let Err(e) = manager.run_update(version.as_deref()) {
                eprintln!("error: {e}");
            }
        }
    }
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;

    // RUST_LOG takes precedence if set; otherwise map -v / -vv to levels
    let filter = if std::env::var("RUST_LOG").is_ok() {
        EnvFilter::from_default_env()
    } else {
        let level = match verbosity {
            0 => "warn",
            1 => "armitage=debug",
            _ => "armitage=trace",
        };
        EnvFilter::new(level)
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .init();
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    // Migrate old flat .armitage/ layout to namespaced directories if inside an org.
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(org_root) = armitage_core::tree::find_org_root(&cwd)
        && let Err(e) = crate::migrate::migrate_dotarmitage(&org_root)
    {
        tracing::warn!("failed to migrate .armitage layout: {e}");
    }

    match cli.command {
        Commands::Init {
            name,
            github_org,
            default_repo,
        } => init::run(name, github_org, default_repo)?,

        Commands::Node { command } => match command {
            NodeCommands::New {
                path,
                name,
                description,
                github_issue,
                labels,
                repos,
                owners,
                status,
                timeline,
            } => {
                node::run_create(
                    path,
                    name,
                    description,
                    github_issue,
                    labels,
                    repos,
                    owners,
                    status,
                    timeline,
                )?;
            }
            NodeCommands::List { path, recursive } => {
                node::run_list(path, recursive)?;
            }
            NodeCommands::Show { path } => {
                node::run_show(path)?;
            }
            NodeCommands::Edit { path } => {
                node::run_edit(path)?;
            }
            NodeCommands::Move { from, to } => {
                node::run_move(from, to)?;
            }
            NodeCommands::Merge { from, to, yes } => {
                node::run_merge(from, to, yes)?;
            }
            NodeCommands::Remove { path, yes } => {
                node::run_remove(path, yes)?;
            }
            NodeCommands::Set {
                path,
                name,
                description,
                triage_hint,
                owners,
                team,
                repos,
                labels,
                status,
            } => {
                node::run_set(
                    path,
                    name,
                    description,
                    triage_hint,
                    owners,
                    team,
                    repos,
                    labels,
                    status,
                )?;
            }
            NodeCommands::Tree { depth } => {
                node::run_tree(depth)?;
            }
            NodeCommands::Fmt { paths } => {
                node::run_fmt(paths)?;
            }
            NodeCommands::Check => {
                node::run_check()?;
            }
        },
        Commands::Milestone { command } => match command {
            MilestoneCommands::Add {
                node_path,
                name,
                date,
                description,
                milestone_type,
                expected_progress,
                github_issue,
            } => {
                milestone::run_add(
                    node_path,
                    name,
                    date,
                    description,
                    milestone_type,
                    expected_progress,
                    github_issue,
                )?;
            }
            MilestoneCommands::List {
                node_path,
                milestone_type,
                quarter,
            } => {
                milestone::run_list(node_path, milestone_type, quarter)?;
            }
            MilestoneCommands::Remove { node_path, name } => {
                milestone::run_remove(node_path, name)?;
            }
        },
        Commands::Pull { path, dry_run } => pull::run(path, dry_run)?,
        Commands::Push { path, dry_run } => push::run(path, dry_run)?,
        Commands::Resolve { path, list } => resolve::run(path, list)?,
        Commands::Status => status::run()?,
        Commands::Chart {
            output,
            no_open,
            offline,
        } => chart::run_chart(output, no_open, offline)?,
        Commands::Config { command } => match command {
            ConfigCommands::Set { key, value } => {
                config::run_set(key, value)?;
            }
            ConfigCommands::SetSecret { name } => {
                config::run_set_secret(name)?;
            }
            ConfigCommands::Show => {
                config::run_show()?;
            }
        },
        Commands::Triage { command } => match command {
            TriageCommands::Fetch { repo, since } => {
                triage::run_fetch(repo, since)?;
            }
            TriageCommands::Labels { command } => match command {
                TriageLabelCommands::Fetch { repo, org } => {
                    triage::run_labels_fetch(repo, org)?;
                }
                TriageLabelCommands::Merge {
                    session,
                    all_new,
                    update_drifted,
                    name,
                    exclude_name,
                    prefer_repo,
                    yes,
                    no_llm,
                    auto_accept,
                    backend,
                    model,
                    effort,
                } => {
                    triage::run_labels_merge(
                        session,
                        all_new,
                        update_drifted,
                        name,
                        exclude_name,
                        prefer_repo,
                        yes,
                        no_llm,
                        auto_accept,
                        backend,
                        model,
                        effort,
                    )?;
                }
                TriageLabelCommands::Sync {
                    repo,
                    org,
                    dry_run,
                    prune,
                } => {
                    triage::run_labels_sync(repo, org, dry_run, prune)?;
                }
                TriageLabelCommands::Push {
                    repo,
                    org,
                    dry_run,
                    delete_extra,
                } => {
                    triage::run_labels_push(repo, org, dry_run, delete_extra)?;
                }
            },
            TriageCommands::Classify {
                backend,
                model,
                effort,
                batch_size,
                parallel,
                limit,
                repo,
                format,
            } => {
                triage::run_classify(
                    backend, model, effort, batch_size, parallel, limit, repo, format,
                )?;
            }
            TriageCommands::Review {
                interactive,
                list,
                auto_approve,
                min_confidence,
                max_confidence,
                format,
            } => {
                triage::run_review(
                    interactive,
                    list,
                    auto_approve,
                    min_confidence,
                    max_confidence,
                    format,
                )?;
            }
            TriageCommands::Apply { dry_run } => {
                triage::run_apply(dry_run)?;
            }
            TriageCommands::Decide {
                issue_refs,
                decision,
                node,
                labels,
                note,
                question,
            } => {
                triage::run_decide(issue_refs, decision, node, labels, note, question)?;
            }
            TriageCommands::Reset {
                below,
                node,
                issue,
                all,
                unreviewed,
            } => {
                triage::run_reset(below, node, issue, all, unreviewed)?;
            }
            TriageCommands::Status { format } => {
                triage::run_status(format)?;
            }
            TriageCommands::Decisions {
                status,
                unapplied,
                node,
                repo,
                limit,
                format,
            } => {
                triage::run_decisions(status, unapplied, node, repo, limit, format)?;
            }
            TriageCommands::Categories { command } => match command {
                TriageCategoryCommands::List { min_votes, format } => {
                    triage::run_categories_list(min_votes, format)?;
                }
                TriageCategoryCommands::Apply {
                    path,
                    name,
                    description,
                    reclassify,
                    reclassify_backend,
                    reclassify_model,
                } => {
                    triage::run_categories_apply(
                        path,
                        name,
                        description,
                        reclassify,
                        reclassify_backend,
                        reclassify_model,
                    )?;
                }
                TriageCategoryCommands::Dismiss { path } => {
                    triage::run_categories_dismiss(path)?;
                }
                TriageCategoryCommands::Refine {
                    backend,
                    model,
                    auto_accept,
                    min_votes,
                } => {
                    triage::run_categories_refine(backend, model, auto_accept, min_votes)?;
                }
            },
            TriageCommands::Examples { command } => match command {
                TriageExampleCommands::List => {
                    triage::run_examples_list()?;
                }
                TriageExampleCommands::Export { status, limit } => {
                    triage::run_examples_export(status, limit)?;
                }
                TriageExampleCommands::Remove { issue_ref } => {
                    triage::run_examples_remove(issue_ref)?;
                }
            },
            TriageCommands::Summary { repo, format } => {
                triage::run_summary(repo, format)?;
            }
            TriageCommands::Suggestions {
                issues,
                node,
                repo,
                min_confidence,
                max_confidence,
                status,
                tracking_only,
                unclassified,
                stale_only,
                sort,
                limit,
                format,
                body_max,
            } => {
                triage::run_suggestions(
                    issues,
                    node,
                    repo,
                    min_confidence,
                    max_confidence,
                    status,
                    tracking_only,
                    unclassified,
                    stale_only,
                    sort,
                    limit,
                    format,
                    body_max,
                )?;
            }
        },
        Commands::SelfCmd { command } => run_self(command),
    }
    Ok(())
}
