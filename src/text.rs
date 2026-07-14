use crate::clock;
use crate::gpu::{Gpu, Process, Status};

pub fn free(status: &Status, host: &str) -> String {
    format!(
        "✅ **GPU is free**\n\
         `GPU:{}` · `{}` · `{}`\n\n\
         **GPU status**\n```text\n\
         Name      {}\n\
         Util      GPU {} | MEM {}\n\
         VRAM      {}\n\
         Compute   No running processes\n\
         ```\n\
         **Ignored GPU commands**\n```text\n{}\n```",
        status.gpu.index,
        host,
        timestamp(),
        status.gpu.name,
        pct(status.gpu.gpu_util),
        pct(status.gpu.memory_util),
        vram(&status.gpu),
        process(&status.ignored),
    )
}

pub fn busy(status: &Status, host: &str) -> String {
    format!(
        "⚠️ **GPU is busy again**\n\
         `GPU:{}` · `{}` · `{}`\n\n\
         **GPU status**\n```text\n\
         Name      {}\n\
         Util      GPU {} | MEM {}\n\
         VRAM      {}\n\
         Compute   Running processes\n\
         ```\n\
         **Running GPU commands**\n```text\n{}\n```\n\
         **Ignored GPU commands**\n```text\n{}\n```",
        status.gpu.index,
        host,
        timestamp(),
        status.gpu.name,
        pct(status.gpu.gpu_util),
        pct(status.gpu.memory_util),
        vram(&status.gpu),
        process(&status.active),
        process(&status.ignored),
    )
}

pub fn freeall(statuses: &[Status], host: &str) -> String {
    if statuses.len() == 1 {
        return free(&statuses[0], host);
    }

    let indexes = statuses
        .iter()
        .map(|status| status.gpu.index.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let ignored = statuses
        .iter()
        .flat_map(|status| status.ignored.iter().cloned())
        .collect::<Vec<_>>();

    format!(
        "✅ **All watched GPUs are free**\n\
         `GPU:{}` · `{}` · `{}`\n\n\
         **GPU status**\n```text\n{}\n```\n\
         **Ignored GPU commands**\n```text\n{}\n```",
        indexes,
        host,
        timestamp(),
        gpus(statuses, false),
        process(&ignored),
    )
}

pub fn busyall(statuses: &[Status], host: &str) -> String {
    let indexes = statuses
        .iter()
        .map(|status| status.gpu.index.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let running = statuses
        .iter()
        .flat_map(|status| status.active.iter().cloned())
        .collect::<Vec<_>>();
    let ignored = statuses
        .iter()
        .flat_map(|status| status.ignored.iter().cloned())
        .collect::<Vec<_>>();

    format!(
        "⚠️ **Watched GPUs are busy again**\n\
         `GPU:{}` · `{}` · `{}`\n\n\
         **Watched GPU status**\n```text\n{}\n```\n\
         **Running GPU commands**\n```text\n{}\n```\n\
         **Ignored GPU commands**\n```text\n{}\n```",
        indexes,
        host,
        timestamp(),
        gpus(statuses, true),
        process(&running),
        process(&ignored),
    )
}

pub fn summary(statuses: &[Status]) -> String {
    statuses
        .iter()
        .map(|status| {
            format!(
                "GPU:{}={}",
                status.gpu.index,
                if status.free() { "free" } else { "busy" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn timestamp() -> String {
    clock::stamp()
}

fn gpus(statuses: &[Status], state: bool) -> String {
    let mut rows = Vec::new();
    if state {
        rows.push("GPU  STATE  GPU%  MEM%  VRAM used/total       Free       Name".to_string());
    } else {
        rows.push("GPU  GPU%  MEM%  VRAM used/total       Free       Name".to_string());
    }

    for status in statuses {
        if state {
            rows.push(format!(
                "{:<4} {:<6} {:<5} {:<5} {:<21} {:<10} {}",
                status.gpu.index,
                if status.free() { "free" } else { "busy" },
                pct(status.gpu.gpu_util),
                pct(status.gpu.memory_util),
                used(&status.gpu),
                mib(status.gpu.memory_free),
                status.gpu.name,
            ));
        } else {
            rows.push(format!(
                "{:<4} {:<5} {:<5} {:<21} {:<10} {}",
                status.gpu.index,
                pct(status.gpu.gpu_util),
                pct(status.gpu.memory_util),
                used(&status.gpu),
                mib(status.gpu.memory_free),
                status.gpu.name,
            ));
        }
    }

    rows.join("\n")
}

fn process(processes: &[Process]) -> String {
    if processes.is_empty() {
        return "none".to_string();
    }

    let mut rows = vec!["GPU  PID       USER        TYPE  VRAM     COMMAND".to_string()];
    for item in processes.iter().take(12) {
        rows.push(format!(
            "{:<4} {:<9} {:<11} {:<5} {:<8} {}",
            item.gpu,
            item.pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "?".to_string()),
            cut(item.user.as_deref().unwrap_or("?"), 10),
            if item.kind.is_empty() {
                "?"
            } else {
                &item.kind
            },
            mib(item.memory),
            cut(
                item.command
                    .as_deref()
                    .filter(|item| !item.is_empty())
                    .unwrap_or(&item.name),
                92,
            ),
        ));
    }
    if processes.len() > 12 {
        rows.push(format!("... and {} more", processes.len() - 12));
    }
    rows.join("\n")
}

fn pct(value: Option<u32>) -> String {
    value
        .map(|item| format!("{item}%"))
        .unwrap_or_else(|| "?".to_string())
}

fn mib(value: Option<u32>) -> String {
    value
        .map(|item| format!("{item}MiB"))
        .unwrap_or_else(|| "?".to_string())
}

fn used(gpu: &Gpu) -> String {
    format!("{}/{}", mib(gpu.memory_used), mib(gpu.memory_total))
}

fn vram(gpu: &Gpu) -> String {
    format!("{} ({} free)", used(gpu), mib(gpu.memory_free))
}

fn cut(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }

    value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_group_message() {
        let statuses = vec![
            Status {
                gpu: Gpu {
                    index: "0".to_string(),
                    name: "A100".to_string(),
                    uuid: None,
                    gpu_util: Some(0),
                    memory_util: Some(0),
                    memory_total: Some(80),
                    memory_used: Some(10),
                    memory_free: Some(70),
                },
                active: vec![],
                ignored: vec![],
            },
            Status {
                gpu: Gpu {
                    index: "2".to_string(),
                    name: "V100".to_string(),
                    uuid: None,
                    gpu_util: Some(1),
                    memory_util: Some(2),
                    memory_total: Some(90),
                    memory_used: Some(20),
                    memory_free: Some(70),
                },
                active: vec![],
                ignored: vec![],
            },
        ];

        let message = freeall(&statuses, "host");
        assert!(message.contains("**All watched GPUs are free**"));
        assert!(message.contains("`GPU:0,2`"));
        assert!(message.contains("10MiB/80MiB"));
        assert!(message.contains("20MiB/90MiB"));
    }
}
