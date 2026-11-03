use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Any,
    All,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    pub urls: Vec<String>,
    pub mode: Mode,
    pub busy: bool,
    pub poll: f64,
    pub remind: f64,
    pub title: String,
    pub nvidia: String,
    pub retry_count: usize,
    pub retry_delay: f64,
    pub ignore: Vec<String>,
    pub users: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    notify_urls: Option<Vec<String>>,
    notify_when: Option<Mode>,
    notify_busy_again: Option<bool>,
    poll_interval_seconds: Option<f64>,
    reminder_interval_minutes: Option<f64>,
    notification_title: Option<String>,
    nvidia_smi: Option<String>,
    notify_retry_count: Option<usize>,
    notify_retry_delay_seconds: Option<f64>,
    ignore: Option<Ignore>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ignore {
    process_patterns: Option<Vec<String>>,
    graphics_users: Option<Vec<String>>,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let raw: Raw = serde_yaml::from_str(&text)
            .with_context(|| format!("failed to parse YAML config {}", path.display()))?;

        let urls = urls(raw.notify_urls)?;
        let poll = positive(
            raw.poll_interval_seconds.unwrap_or(60.0),
            "poll_interval_seconds",
        )?;
        let remind = positive(
            raw.reminder_interval_minutes.unwrap_or(30.0),
            "reminder_interval_minutes",
        )? * 60.0;
        let retry_count = raw.notify_retry_count.unwrap_or(3);
        let retry_delay = positive(
            raw.notify_retry_delay_seconds.unwrap_or(2.0),
            "notify_retry_delay_seconds",
        )?;
        let ignore = raw.ignore.unwrap_or_default();

        Ok(Self {
            path,
            urls,
            mode: raw.notify_when.unwrap_or(Mode::Any),
            busy: raw.notify_busy_again.unwrap_or(true),
            poll,
            remind,
            title: clean(raw.notification_title).unwrap_or_else(|| "GPU free".to_string()),
            nvidia: clean(raw.nvidia_smi).unwrap_or_else(|| "nvidia-smi".to_string()),
            retry_count,
            retry_delay,
            ignore: clean_vec(ignore.process_patterns.unwrap_or_default()),
            users: clean_vec(ignore.graphics_users.unwrap_or_default()),
        })
    }
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn urls(values: Option<Vec<String>>) -> Result<Vec<String>> {
    let values =
        values.ok_or_else(|| anyhow::anyhow!("missing required config value: notify_urls"))?;
    let values = clean_vec(values);
    if values.is_empty() {
        bail!("config value must contain at least one URL: notify_urls");
    }
    Ok(values)
}

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn positive(value: f64, key: &str) -> Result<f64> {
    if !value.is_finite() || value <= 0.0 {
        bail!("config value must be a positive number: {key}");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nvishoutr-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn reads_config_with_defaults() {
        let path = write("default", "notify_urls:\n  - discord://token@123\n");

        let loaded = Config::load(path).unwrap();

        assert_eq!(loaded.urls, vec!["discord://token@123"]);
        assert_eq!(loaded.mode, Mode::Any);
        assert!(loaded.busy);
        assert_eq!(loaded.poll, 60.0);
        assert_eq!(loaded.remind, 1800.0);
        assert_eq!(loaded.title, "GPU free");
        assert_eq!(loaded.nvidia, "nvidia-smi");
    }

    #[test]
    fn reads_full_config() {
        let path = write(
            "full",
            r#"
notify_urls:
  - discord://token@123
  - generic+https://example.com/webhook?template=json
notify_when: all
notify_busy_again: false
poll_interval_seconds: 5
reminder_interval_minutes: 0.5
notification_title: Ready
nvidia_smi: /usr/bin/nvidia-smi
ignore:
  process_patterns:
    - ^viewer$
  graphics_users:
    - lightdm
"#,
        );

        let loaded = Config::load(path).unwrap();

        assert_eq!(
            loaded.urls,
            vec![
                "discord://token@123",
                "generic+https://example.com/webhook?template=json"
            ]
        );
        assert_eq!(loaded.mode, Mode::All);
        assert!(!loaded.busy);
        assert_eq!(loaded.poll, 5.0);
        assert_eq!(loaded.remind, 30.0);
        assert_eq!(loaded.title, "Ready");
        assert_eq!(loaded.nvidia, "/usr/bin/nvidia-smi");
        assert_eq!(loaded.ignore, vec!["^viewer$"]);
        assert_eq!(loaded.users, vec!["lightdm"]);
    }

    #[test]
    fn rejects_bad_mode() {
        let path = write(
            "bad",
            "notify_urls:\n  - discord://token@123\nnotify_when: sometimes\n",
        );

        assert!(Config::load(path).is_err());
    }

    #[test]
    fn rejects_empty_urls() {
        let path = write("empty-urls", "notify_urls: []\n");

        assert!(Config::load(path).is_err());
    }

    #[test]
    fn rejects_unknown_key() {
        let path = write(
            "unknown",
            "notify_urls:\n  - discord://token@123\nextra_url: discord://token@123\n",
        );

        assert!(Config::load(path).is_err());
    }
}
