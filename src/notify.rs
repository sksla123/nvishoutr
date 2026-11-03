use anyhow::{Context, Result, bail};
use shoutrrr::transport::ReqwestClient;
use shoutrrr::{Params, Sender};
use tokio::runtime::{Builder, Runtime};

pub struct Notifier {
    sender: Sender,
    http: ReqwestClient,
    runtime: Runtime,
    params: Params,
    labels: Vec<String>,
    retry_count: usize,
    retry_delay: f64,
}

impl Notifier {
    pub fn new(urls: &[String], title: &str, retry_count: usize, retry_delay: f64) -> Result<Self> {
        let urls = urls.iter().map(|url| prepare(url)).collect::<Vec<_>>();

        for (index, url) in urls.iter().enumerate() {
            shoutrrr::router::service_from_url(url)
                .with_context(|| format!("invalid notification URL at notify_urls[{index}]"))?;
        }

        let sender = Sender::from_urls(urls.iter().map(String::as_str))
            .context("failed to build notification sender")?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build notification runtime")?;
        let title = title.trim();
        let params = if title.is_empty() {
            Params::new()
        } else {
            Params::new().with_title(title)
        };
        let labels = urls.iter().map(|url| mask(url)).collect();

        Ok(Self {
            sender,
            http: ReqwestClient::new(),
            runtime,
            params,
            labels,
            retry_count,
            retry_delay,
        })
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn send(&self, message: &str) -> Result<()> {
        let mut attempts = 0;
        loop {
            let results =
                self.runtime
                    .block_on(self.sender.send(&self.http, message, &self.params));
            let failures = results
                .into_iter()
                .enumerate()
                .filter_map(|(index, result)| {
                    result
                        .err()
                        .map(|error| format!("{}: {error}", self.labels[index]))
                })
                .collect::<Vec<_>>();

            if failures.is_empty() {
                return Ok(());
            }

            if attempts >= self.retry_count {
                bail!(
                    "{} notification destination(s) failed after {} retries: {}",
                    failures.len(),
                    self.retry_count,
                    failures.join("; ")
                );
            }

            attempts += 1;
            std::thread::sleep(std::time::Duration::from_secs_f64(self.retry_delay));
        }
    }
}

fn prepare(url: &str) -> String {
    if !url
        .get(..10)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("discord://"))
    {
        return url.to_string();
    }
    if has_query_key(url, "splitlines") {
        return url.to_string();
    }

    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}splitLines=No")
}

fn has_query_key(url: &str, key: &str) -> bool {
    let Some((_head, query)) = url.split_once('?') else {
        return false;
    };
    query.split('&').any(|part| {
        part.split_once('=')
            .map(|(name, _value)| name)
            .unwrap_or(part)
            .eq_ignore_ascii_case(key)
    })
}

pub fn mask(url: &str) -> String {
    let mut value = hide_query(url);
    let Some(scheme_end) = value.find("://") else {
        return value;
    };
    let authority_start = scheme_end + 3;
    let authority_end = value[authority_start..]
        .find('/')
        .map(|offset| authority_start + offset)
        .unwrap_or(value.len());
    let Some(at_offset) = value[authority_start..authority_end].rfind('@') else {
        return value;
    };

    let at = authority_start + at_offset;
    let replacement = secret(&value[authority_start..at]);
    value.replace_range(authority_start..at, &replacement);
    value
}

fn hide_query(url: &str) -> String {
    let Some((head, _query)) = url.split_once('?') else {
        return url.to_string();
    };
    format!("{head}?...")
}

fn secret(value: &str) -> String {
    let prefix = value.chars().take(4).collect::<String>();
    if prefix.chars().count() < 4 {
        "***".to_string()
    } else {
        format!("{prefix}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_notifier_for_supported_urls() {
        let urls = vec![
            "discord://token@123456".to_string(),
            "generic+https://example.com/webhook?template=json".to_string(),
        ];

        let notifier = Notifier::new(&urls, "GPU free", 3, 2.0).unwrap();

        assert_eq!(notifier.len(), 2);
    }

    #[test]
    fn prepares_discord_url_without_line_splitting() {
        assert_eq!(
            prepare("discord://token@123456"),
            "discord://token@123456?splitLines=No"
        );
        assert_eq!(
            prepare("discord://token@123456?thread_id=7"),
            "discord://token@123456?thread_id=7&splitLines=No"
        );
        assert_eq!(
            prepare("discord://token@123456?splitLines=Yes"),
            "discord://token@123456?splitLines=Yes"
        );
    }

    #[test]
    fn prepared_discord_url_keeps_multiline_message_together() {
        let url = prepare("discord://token@123456");
        let message = (0..12)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let params = Params::new().with_title("GPU free");
        let mock = shoutrrr::transport::MockTransport::new(204, "");
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();

        runtime
            .block_on(shoutrrr::send_with_params(&mock, &url, &message, &params))
            .unwrap();

        assert_eq!(mock.request_count(), 1);
    }

    #[test]
    fn rejects_unsupported_urls() {
        let urls = vec!["mailto://user:pass@example.com".to_string()];

        assert!(Notifier::new(&urls, "GPU free", 3, 2.0).is_err());
    }

    #[test]
    fn masks_shoutrrr_discord_url() {
        assert_eq!(mask("discord://abcdef@123456"), "discord://abcd...@123456");
    }

    #[test]
    fn masks_query_values() {
        assert_eq!(
            mask("generic+https://example.com/hook?@Authorization=Bearer%20token"),
            "generic+https://example.com/hook?..."
        );
    }
}
