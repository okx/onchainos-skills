use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

const OKX_A2A: &str = "okx-a2a";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct JobProviderPreBind {
    job_id: String,
    provider: String,
    created: bool,
}

impl JobProviderPreBind {
    pub async fn rollback_if_created(&self) {
        if !self.created || self.job_id.is_empty() || self.provider.is_empty() {
            return;
        }
        let args = [
            "job-provider",
            "unset",
            "--job-id",
            self.job_id.as_str(),
            "--provider",
            self.provider.as_str(),
            "--json",
        ];
        match run_okx_a2a(&args).await {
            Ok(output) if output.status.success() => {
                eprintln!(
                    "[a2a-binding] rolled back pre-broadcast job provider binding: jobId={} provider={}",
                    self.job_id, self.provider
                );
            }
            Ok(output) => {
                eprintln!(
                    "[a2a-binding] WARN: rollback failed: jobId={} provider={} exit={:?} stderr={} stdout={}",
                    self.job_id,
                    self.provider,
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim(),
                    String::from_utf8_lossy(&output.stdout).trim()
                );
            }
            Err(e) => {
                eprintln!(
                    "[a2a-binding] WARN: rollback unavailable: jobId={} provider={}: {e}",
                    self.job_id, self.provider
                );
            }
        }
    }
}

/// Ask okx-a2a to bind a newly-created on-chain job to the current AI runtime.
/// Runtime detection intentionally lives in okx-a2a so onchainos does not need
/// to know Hermes/OpenClaw/Codex/Claude platform markers.
pub async fn bind_job_provider_to_current_runtime(job_id: &str) -> Option<JobProviderPreBind> {
    let job_id = job_id.trim();
    if job_id.is_empty() || job_id == "?" {
        return None;
    }
    if is_truthy_env("OKX_A2A_DISABLE_JOB_PROVIDER_BINDING") {
        return None;
    }

    let args = ["job-provider", "bind-current", "--job-id", job_id, "--json"];
    match run_okx_a2a(&args).await {
        Ok(output) if output.status.success() => {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                let provider = value
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let created = value
                    .get("created")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                eprintln!("[a2a-binding] job provider bind-current ok: jobId={job_id} provider={provider} created={created}");
                if provider != "unknown" {
                    return Some(JobProviderPreBind {
                        job_id: job_id.to_string(),
                        provider: provider.to_string(),
                        created,
                    });
                }
            } else {
                eprintln!("[a2a-binding] job provider bind-current ok: jobId={job_id}");
            }
        }
        Ok(output) => {
            eprintln!(
                "[a2a-binding] WARN: job provider bind-current failed: jobId={job_id} exit={:?} stderr={} stdout={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim(),
                String::from_utf8_lossy(&output.stdout).trim()
            );
        }
        Err(e) => {
            eprintln!(
                "[a2a-binding] WARN: job provider bind-current unavailable for jobId={job_id}: {e}"
            );
        }
    }
    None
}

async fn run_okx_a2a(args: &[&str]) -> Result<std::process::Output, String> {
    let fut = Command::new(OKX_A2A)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match timeout(COMMAND_TIMEOUT, fut).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(format!("spawn `{OKX_A2A}` failed: {e}")),
        Err(_) => Err(format!("`{OKX_A2A} {}` timed out", args.join(" "))),
    }
}

fn is_truthy_env(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}
