pub mod error;
pub mod issue;

/// Re-export the Gh type that downstream crates need.
pub use ionem::shell::gh::Gh;

/// Create a Gh instance, verifying gh CLI is installed and authenticated.
pub fn require_gh() -> std::result::Result<Gh, ionem::shell::CliError> {
    ionem::shell::gh::require()
}
