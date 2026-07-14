use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use regex::Regex;
use roxmltree::{Document, Node};

pub const DEFAULT_IGNORE: &[&str] = &[
    r"^Xorg$",
    r"^Xwayland$",
    r"^gnome-shell$",
    r"^gdm-x-session$",
    r"^gdm-wayland-session$",
    r"^kwin_x11$",
    r"^kwin_wayland$",
    r"^plasmashell$",
];
pub const DEFAULT_USERS: &[&str] = &["gdm"];

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Gpu {
    pub index: String,
    pub name: String,
    pub uuid: Option<String>,
    pub gpu_util: Option<u32>,
    pub memory_util: Option<u32>,
    pub memory_total: Option<u32>,
    pub memory_used: Option<u32>,
    pub memory_free: Option<u32>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Process {
    pub gpu: String,
    pub name: String,
    pub pid: Option<u32>,
    pub kind: String,
    pub memory: Option<u32>,
    pub user: Option<String>,
    pub command: Option<String>,
}

impl Process {
    fn base(&self) -> &str {
        Path::new(&self.name)
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub gpus: Vec<Gpu>,
    pub processes: Vec<Process>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Status {
    pub gpu: Gpu,
    pub active: Vec<Process>,
    pub ignored: Vec<Process>,
}

impl Status {
    pub fn free(&self) -> bool {
        self.active.is_empty()
    }
}

pub struct Filter {
    patterns: Vec<Regex>,
    users: HashSet<String>,
}

impl Filter {
    pub fn new(patterns: &[String], users: &[String]) -> Result<Self> {
        let mut all_patterns: Vec<String> =
            DEFAULT_IGNORE.iter().map(|item| item.to_string()).collect();
        all_patterns.extend(patterns.iter().cloned());
        let patterns = all_patterns
            .iter()
            .map(|pattern| Regex::new(&format!("(?i:{pattern})")))
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to compile ignore process regex")?;

        let mut all_users: HashSet<String> =
            DEFAULT_USERS.iter().map(|item| item.to_string()).collect();
        all_users.extend(users.iter().cloned());

        Ok(Self {
            patterns,
            users: all_users,
        })
    }

    fn ignored(&self, process: &Process) -> bool {
        self.patterns
            .iter()
            .any(|pattern| pattern.is_match(process.base()))
            || (process.kind.eq_ignore_ascii_case("G")
                && process
                    .user
                    .as_ref()
                    .is_some_and(|user| self.users.contains(user)))
    }
}

pub fn collect(binary: &str) -> Result<Snapshot> {
    let output = Command::new(binary)
        .args(["-q", "-x"])
        .output()
        .with_context(|| format!("failed to execute {binary}"))?;

    if !output.status.success() {
        bail!(
            "{} failed: {}",
            binary,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let text = String::from_utf8(output.stdout).context("nvidia-smi output was not UTF-8")?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Snapshot> {
    let sanitized = strip_doctype(text);
    let doc = Document::parse(&sanitized).context("failed to parse nvidia-smi XML")?;
    let mut gpus = Vec::new();
    let mut processes = Vec::new();

    for (fallback, node) in doc
        .descendants()
        .filter(|node| node.has_tag_name("gpu"))
        .enumerate()
    {
        let index = child(&node, "minor_number").unwrap_or_else(|| fallback.to_string());
        let gpu = Gpu {
            index,
            name: child(&node, "product_name").unwrap_or_else(|| format!("GPU {fallback}")),
            uuid: child(&node, "uuid"),
            gpu_util: number(path(&node, &["utilization", "gpu_util"]).as_deref()),
            memory_util: number(path(&node, &["utilization", "memory_util"]).as_deref()),
            memory_total: number(path(&node, &["fb_memory_usage", "total"]).as_deref()),
            memory_used: number(path(&node, &["fb_memory_usage", "used"]).as_deref()),
            memory_free: number(path(&node, &["fb_memory_usage", "free"]).as_deref()),
        };

        for item in node
            .descendants()
            .filter(|node| node.has_tag_name("process_info"))
        {
            let pid = number(child(&item, "pid").as_deref());
            processes.push(Process {
                gpu: gpu.index.clone(),
                name: child(&item, "process_name").unwrap_or_default(),
                pid,
                kind: child(&item, "type").unwrap_or_default(),
                memory: number(child(&item, "used_memory").as_deref()),
                user: pid.and_then(user),
                command: pid.and_then(command),
            });
        }

        gpus.push(gpu);
    }

    Ok(Snapshot { gpus, processes })
}

fn strip_doctype(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("<!DOCTYPE"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn statuses(snapshot: &Snapshot, filter: &Filter) -> BTreeMap<String, Status> {
    let mut map: BTreeMap<String, Status> = snapshot
        .gpus
        .iter()
        .map(|gpu| {
            (
                gpu.index.clone(),
                Status {
                    gpu: gpu.clone(),
                    active: Vec::new(),
                    ignored: Vec::new(),
                },
            )
        })
        .collect();

    for process in &snapshot.processes {
        if let Some(status) = map.get_mut(&process.gpu) {
            if filter.ignored(process) {
                status.ignored.push(process.clone());
            } else {
                status.active.push(process.clone());
            }
        }
    }

    map
}

fn child(node: &Node<'_, '_>, tag: &str) -> Option<String> {
    node.children()
        .find(|child| child.has_tag_name(tag))
        .and_then(|child| child.text())
        .and_then(clean)
}

fn path(node: &Node<'_, '_>, tags: &[&str]) -> Option<String> {
    let mut cursor = *node;
    for tag in tags {
        cursor = cursor.children().find(|child| child.has_tag_name(*tag))?;
    }
    cursor.text().and_then(clean)
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "N/A" {
        None
    } else {
        Some(value.to_string())
    }
}

fn number(value: Option<&str>) -> Option<u32> {
    let text = value?;
    let number: String = text
        .chars()
        .skip_while(|item| !item.is_ascii_digit())
        .take_while(|item| item.is_ascii_digit())
        .collect();
    number.parse().ok()
}

fn user(pid: u32) -> Option<String> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let uid = status
        .lines()
        .find(|line| line.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse::<u32>()
        .ok()?;

    passwd_user(uid).or_else(|| Some(uid.to_string()))
}

fn passwd_user(uid: u32) -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let mut parts = line.split(':');
        let name = parts.next()?;
        let _password = parts.next()?;
        let current = parts.next()?.parse::<u32>().ok()?;
        if current == uid {
            return Some(name.to_string());
        }
    }
    None
}

fn command(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let text = raw
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part))
        .collect::<Vec<_>>()
        .join(" ");
    if !text.trim().is_empty() {
        return Some(text);
    }

    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
<nvidia_smi_log>
  <gpu>
    <product_name>Tesla V100</product_name>
    <minor_number>0</minor_number>
    <uuid>GPU-0</uuid>
    <fb_memory_usage>
      <total>32768 MiB</total>
      <used>1032 MiB</used>
      <free>31736 MiB</free>
    </fb_memory_usage>
    <utilization>
      <gpu_util>92 %</gpu_util>
      <memory_util>15 %</memory_util>
    </utilization>
    <processes>
      <process_info>
        <pid>999999999</pid>
        <type>G</type>
        <process_name>/usr/lib/xorg/Xorg</process_name>
        <used_memory>8 MiB</used_memory>
      </process_info>
      <process_info>
        <pid>999999998</pid>
        <type>C</type>
        <process_name>python</process_name>
        <used_memory>1024 MiB</used_memory>
      </process_info>
    </processes>
  </gpu>
</nvidia_smi_log>
"#;

    #[test]
    fn parses_gpu_metrics() {
        let snapshot = parse(SAMPLE).unwrap();
        let gpu = &snapshot.gpus[0];
        assert_eq!(gpu.gpu_util, Some(92));
        assert_eq!(gpu.memory_util, Some(15));
        assert_eq!(gpu.memory_total, Some(32768));
        assert_eq!(gpu.memory_used, Some(1032));
        assert_eq!(gpu.memory_free, Some(31736));
        assert_eq!(snapshot.processes.len(), 2);
    }

    #[test]
    fn splits_active_and_ignored() {
        let snapshot = parse(SAMPLE).unwrap();
        let filter = Filter::new(&[], &[]).unwrap();
        let statuses = statuses(&snapshot, &filter);
        let status = statuses.get("0").unwrap();
        assert!(!status.free());
        assert_eq!(status.active[0].name, "python");
        assert_eq!(status.ignored[0].name, "/usr/lib/xorg/Xorg");
    }
}
