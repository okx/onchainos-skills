use anyhow::{anyhow, Result};
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum SkillsCommand {
    /// Check whether the calling skill's version (baked into the preflight
    /// by release CI) matches this CLI binary's version. Ok on match;
    /// Err with a "re-install skills" message on any mismatch.
    Check {
        /// The version the skill was released at, baked into the preflight by CI.
        #[arg(long)]
        expected_version: String,
    },
}

pub async fn execute(cmd: SkillsCommand) -> Result<()> {
    match cmd {
        SkillsCommand::Check { expected_version } => check(&expected_version),
    }
}

fn check(expected: &str) -> Result<()> {
    let cli_version = env!("CARGO_PKG_VERSION");
    if cli_version == expected {
        return Ok(());
    }

    Err(anyhow!(
        "warn: onchainos skill version {expected} does not match \
         CLI version {cli_version}. Re-install skills: \
         https://github.com/okx/onchainos-skills#installation"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_returns_ok() {
        assert!(check(env!("CARGO_PKG_VERSION")).is_ok());
    }

    #[test]
    fn mismatch_reports_skill_reinstall() {
        let err = check("0.0.1").unwrap_err().to_string();
        assert!(err.starts_with("warn: "));
        assert!(err.contains("skill version 0.0.1 does not match"));
        assert!(err.contains("#installation"));
        assert!(!err.contains('⚠'));
        assert!(!err.contains("curl -sSL"));
    }

    #[test]
    fn any_non_equal_version_triggers_warning() {
        assert!(check("999.0.0").is_err());
        assert!(check("0.0.1-beta.0").is_err());
    }
}
