use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::config::Config;
use crate::gpu::{self, Status};
use crate::screen::Screen;
use crate::state::{Action, Engine};
use crate::{clock, notify, text};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Watch shared NVIDIA GPUs and notify when one becomes available."
)]
struct Args {
    #[arg(long, default_value = "config.yaml", help = "YAML config path")]
    config: PathBuf,
    #[arg(
        long,
        default_value = "all",
        help = "GPU indexes to watch: all, *, or a comma-separated list like 0,1,3"
    )]
    gpus: String,
    #[arg(long, help = "log notifications without sending them")]
    dry_run: bool,
    #[arg(
        long,
        help = "verify GPU detection and test notification URLs, then exit"
    )]
    verify_config: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum Selection {
    All,
    Some(Vec<String>),
}

pub fn run() -> Result<()> {
    let args = Args::parse();

    if args.verify_config {
        let mut ignored = Vec::new();
        if args.dry_run {
            ignored.push("--dry-run");
        }

        if !ignored.is_empty() {
            eprintln!(
                "{} [WARN] --verify-config is active. Ignoring execution options: {}",
                stamp(),
                ignored.join(", ")
            );
        }

        let config = Config::load(&args.config)?;
        let notifier = notify::Notifier::new(&config.urls, &config.title)?;
        let selection = Selection::parse(&args.gpus)?;

        return verify_config(&config, &notifier, &selection);
    }

    let config = Config::load(&args.config)?;
    let notifier = notify::Notifier::new(&config.urls, &config.title)?;
    let selection = Selection::parse(&args.gpus)?;
    watch(config, notifier, selection, args.dry_run)
}

fn verify_config(
    config: &Config,
    notifier: &notify::Notifier,
    selection: &Selection,
) -> Result<()> {
    let host = hostname();
    println!("{} === nvishoutr Configuration Verification ===", stamp());
    println!("{} Host: {}", stamp(), host);
    println!("{} Config path: {}", stamp(), config.path.display());
    println!("{} Notification title: {}", stamp(), config.title);
    println!("{} Target mode: {:?}", stamp(), config.mode);
    println!(
        "{} Polling interval: {:.1}s / Reminder: {:.1}s",
        stamp(),
        config.poll,
        config.remind
    );

    // 1. GPU 탐지 및 타겟 매핑 검증
    println!(
        "{} [1/3] Checking GPU availability with '{}'...",
        stamp(),
        config.nvidia
    );
    let filter = gpu::Filter::new(&config.ignore, &config.users)?;
    let selected = poll(config, &filter, selection)?;

    println!(
        "{} -> Successfully identified {} GPU(s) matching selection '{}':",
        stamp(),
        selected.len(),
        selection.label()
    );
    for status in &selected {
        let mem_str = match (status.gpu.memory_used, status.gpu.memory_total) {
            (Some(used), Some(total)) => format!("{used}MiB/{total}MiB"),
            _ => "N/A".to_string(),
        };
        let util_str = match (status.gpu.gpu_util, status.gpu.memory_util) {
            (Some(g), Some(m)) => format!("GPU {g}% | MEM {m}%"),
            _ => "N/A".to_string(),
        };
        println!(
            "{}    * GPU {}: {} | {} | {} (Active compute: {}, Ignored GUI: {})",
            stamp(),
            status.gpu.index,
            status.gpu.name,
            mem_str,
            util_str,
            status.active.len(),
            status.ignored.len()
        );
    }

    // 2. 웹훅 URL 설정 검증
    println!(
        "{} [2/3] Checking registered webhook destinations...",
        stamp()
    );
    println!(
        "{} -> Found {} destination(s) ready to send.",
        stamp(),
        notifier.len()
    );

    // 3. 테스트 알림 전송 검증
    println!(
        "{} [3/3] Sending test verification notification...",
        stamp()
    );
    let mut details = format!("🔔 **[Verification]** Config & GPU Check from `{host}`\n\n");
    details.push_str(&format!(
        "• **Watched GPUs:** `{}` (Total: {} detected)\n",
        selection.label(),
        selected.len()
    ));
    details.push_str(&format!(
        "• **Trigger Mode:** `notify_when: {:?}`\n",
        config.mode
    ));
    details.push_str("• **Current Status:**\n");
    for s in &selected {
        details.push_str(&format!(
            "  - GPU `{}`: {} (Active processes: {})\n",
            s.gpu.index,
            s.gpu.name,
            s.active.len()
        ));
    }

    notifier
        .send(&details)
        .context("Failed to deliver test notification to webhook(s)")?;

    println!("{} -> All notifications delivered successfully.", stamp());
    println!("{} === Verification Completed: All systems OK ===", stamp());
    Ok(())
}

fn watch(
    config: Config,
    notifier: notify::Notifier,
    selection: Selection,
    dry: bool,
) -> Result<()> {
    let filter = gpu::Filter::new(&config.ignore, &config.users)?;
    let mut engine = Engine::new(config.mode, config.busy, config.remind);
    let mut screen = Screen::new();
    let host = hostname();

    println!(
        "{} watching gpus={} poll={:.1}s remind={:.1}s config={}",
        stamp(),
        selection.label(),
        config.poll,
        config.remind,
        config.path.display(),
    );

    loop {
        let selected = match poll(&config, &filter, &selection) {
            Ok(selected) => selected,
            Err(error) => {
                screen.close();
                println!("{} error {error:#}", stamp());
                thread::sleep(Duration::from_secs_f64(config.poll));
                continue;
            }
        };

        let summary = text::summary(&selected);
        screen.update(&summary);
        let actions = engine.evaluate(&selected, Instant::now());
        for action in actions {
            screen.close();
            if let Err(error) = dispatch(&notifier, &selected, &action, &host, dry) {
                eprintln!("{} error: {error:#}", stamp());
            }
        }

        thread::sleep(Duration::from_secs_f64(config.poll));
    }
}

fn poll(config: &Config, filter: &gpu::Filter, selection: &Selection) -> Result<Vec<Status>> {
    let snapshot = gpu::collect(&config.nvidia)?;
    let statuses = gpu::statuses(&snapshot, filter);
    select(&statuses, selection)
}

fn dispatch(
    notifier: &notify::Notifier,
    selected: &[Status],
    action: &Action,
    host: &str,
    dry: bool,
) -> Result<()> {
    let message = match action {
        Action::Free(index) => text::free(&selected[*index], host),
        Action::Busy(index) => text::busy(&selected[*index], host),
        Action::FreeAll => text::freeall(selected, host),
        Action::BusyAll => text::busyall(selected, host),
    };

    if dry {
        println!("{} dry run; would notify\n{}", stamp(), message);
    } else {
        notifier
            .send(&message)
            .context("failed to notify configured destination(s)")?;
        println!(
            "{} notification sent to {} destination(s)",
            stamp(),
            notifier.len()
        );
    }

    Ok(())
}

fn select(
    items: &std::collections::BTreeMap<String, Status>,
    selection: &Selection,
) -> Result<Vec<Status>> {
    match selection {
        Selection::All => Ok(items.values().cloned().collect()),
        Selection::Some(indexes) => {
            let mut selected = Vec::new();
            for index in indexes {
                let status = items.get(index).with_context(|| {
                    let available = items.keys().cloned().collect::<Vec<_>>().join(", ");
                    format!("requested GPU not found: {index}. Available GPU(s): {available}")
                })?;
                selected.push(status.clone());
            }
            Ok(selected)
        }
    }
}

impl Selection {
    fn parse(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.eq_ignore_ascii_case("all") || trimmed == "*" {
            return Ok(Self::All);
        }

        let indexes = trimmed
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();

        if indexes.is_empty() {
            bail!("--gpus must be all or a comma-separated list like 0,1,3");
        }
        if let Some(invalid) = indexes
            .iter()
            .find(|index| !index.chars().all(|item| item.is_ascii_digit()))
        {
            bail!("invalid GPU index value: {invalid}");
        }

        let mut seen = std::collections::HashSet::new();
        for index in &indexes {
            if !seen.insert(index) {
                bail!("duplicate GPU index in --gpus: {value}");
            }
        }

        Ok(Self::Some(indexes))
    }

    fn label(&self) -> String {
        match self {
            Selection::All => "all".to_string(),
            Selection::Some(indexes) => indexes.join(","),
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|item| !item.is_empty())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn stamp() -> String {
    clock::stamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::Gpu;

    #[test]
    fn parses_selection() {
        assert_eq!(Selection::parse("all").unwrap(), Selection::All);
        assert_eq!(
            Selection::parse("0, 2,3").unwrap(),
            Selection::Some(vec!["0".to_string(), "2".to_string(), "3".to_string()])
        );
        assert!(Selection::parse("1,1").is_err());
    }

    #[test]
    fn selects_requested_gpus() {
        let items = [
            (
                "0".to_string(),
                Status {
                    gpu: Gpu {
                        index: "0".to_string(),
                        name: "A".to_string(),
                        uuid: None,
                        gpu_util: None,
                        memory_util: None,
                        memory_total: None,
                        memory_used: None,
                        memory_free: None,
                    },
                    active: vec![],
                    ignored: vec![],
                },
            ),
            (
                "1".to_string(),
                Status {
                    gpu: Gpu {
                        index: "1".to_string(),
                        name: "B".to_string(),
                        uuid: None,
                        gpu_util: None,
                        memory_util: None,
                        memory_total: None,
                        memory_used: None,
                        memory_free: None,
                    },
                    active: vec![],
                    ignored: vec![],
                },
            ),
        ]
        .into_iter()
        .collect();

        let selected = select(&items, &Selection::Some(vec!["1".to_string()])).unwrap();
        assert_eq!(selected[0].gpu.index, "1");
    }
}
