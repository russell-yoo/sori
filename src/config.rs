use serde::Deserialize;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;

pub(crate) struct AppConfig {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) api_token: Option<String>,
    pub(crate) destinations: HashMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Destination {
    name: String,
    web_hook_url: String,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ServerConfig {
    host: IpAddr,
    port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 44334,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthConfig {
    api_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    auth: AuthConfig,
    destinations: Vec<Destination>,
}

impl Config {
    fn into_app_config(self) -> Result<AppConfig, Box<dyn Error>> {
        if self
            .auth
            .api_token
            .as_deref()
            .is_some_and(|token| token.trim().is_empty())
        {
            return Err("auth.api_token must not be empty".into());
        }
        if self.destinations.is_empty() {
            return Err("At least one destination is required".into());
        }
        let mut destinations: HashMap<String, String> = HashMap::new();

        for destination in self.destinations {
            if destination.name.trim().is_empty() {
                return Err("Destination name must not be empty".into());
            }

            if destination.name != destination.name.trim() {
                return Err("Destination name must not have leading or trailing whitespace".into());
            }

            if destination.web_hook_url.trim().is_empty() {
                return Err(
                    format!("Webhook URL is empty for destination: {}", destination.name).into(),
                );
            }

            let url = reqwest::Url::parse(&destination.web_hook_url).map_err(|_| {
                format!("Invalid Webhook URL for destination: {}", destination.name)
            })?;

            if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
                return Err(format!(
                    "Invalid Webhook URL scheme or host for destination: {}",
                    destination.name
                )
                .into());
            }
            match destinations.entry(destination.name) {
                Entry::Vacant(entry) => {
                    entry.insert(destination.web_hook_url);
                }
                Entry::Occupied(entry) => {
                    return Err(format!("Duplicate destination: {}", entry.key()).into());
                }
            }
        }

        Ok(AppConfig {
            bind_addr: SocketAddr::new(self.server.host, self.server.port),
            api_token: self.auth.api_token,
            destinations,
        })
    }
}

fn parse_config(yaml: &str) -> Result<Config, Box<dyn Error>> {
    yaml_serde::from_str(yaml).map_err(|error| {
        let message = match error.location() {
            Some(location) => format!(
                "Invalid YAML configuration at line {}, column {}",
                location.line(),
                location.column()
            ),
            None => String::from("Invalid YAML configuration"),
        };

        message.into()
    })
}

pub(crate) fn load_config(path: &Path) -> Result<AppConfig, Box<dyn Error>> {
    let yaml = std::fs::read_to_string(path)?;
    let config = parse_config(&yaml)?;
    config.into_app_config()
}

#[cfg(test)]
mod tests {
    use super::{AuthConfig, Config, Destination, ServerConfig, parse_config};

    #[test]
    fn parses_destinations_from_yaml() {
        let yaml = r#"
auth:
  api_token: "test-token"
destinations:
  - name: news
    web_hook_url: "https://example.com/webhook"
"#;
        let config = parse_config(yaml).expect("valid YAML should parse");

        assert_eq!(config.destinations.len(), 1);

        let destination = &config.destinations[0];
        assert_eq!(destination.name, "news");
        assert_eq!(destination.web_hook_url, "https://example.com/webhook");
        assert_eq!(config.server.host.to_string(), "127.0.0.1");
        assert_eq!(config.server.port, 44334);
        assert_eq!(config.auth.api_token.unwrap(), "test-token");
    }

    #[test]
    fn rejects_unknown_destination_fields() {
        let yaml = r#"
auth:
  api_token: "test-token"
destinations:
  - name: news
    web_hook_url: "https://example.com/webhook"
    unknown_option: true
"#;

        assert!(parse_config(yaml).is_err());
    }

    #[test]
    fn rejects_duplicate_destination_names() {
        let yaml = r#"
auth:
  api_token: "test-token"
destinations:
  - name: news
    web_hook_url: "https://example.com/first"
  - name: news
    web_hook_url: "https://example.com/second"
"#;

        let config = parse_config(yaml).expect("valid YAML should parse");

        let error = config
            .into_app_config()
            .err()
            .expect("invalid configuration should be rejected");

        assert_eq!(error.to_string(), "Duplicate destination: news");
    }

    #[test]
    fn rejects_destination_names_with_surrounding_whitespace() {
        for name in [" news", "news "] {
            let config = Config {
                server: ServerConfig::default(),
                auth: AuthConfig {
                    api_token: Some("api_token".to_string()),
                },
                destinations: vec![Destination {
                    name: String::from(name),
                    web_hook_url: String::from("https://example.com/webhook"),
                }],
            };

            let error = config
                .into_app_config()
                .err()
                .expect("invalid configuration should be rejected");

            assert_eq!(
                error.to_string(),
                "Destination name must not have leading or trailing whitespace"
            );
        }
    }

    #[test]
    fn yaml_errors_do_not_expose_input_values() {
        let yaml = r#"
auth:
  api_token: "test-token"
destinations: "https://example.com/webhook/TEST_SECRET"
"#;

        let error = parse_config(yaml)
            .err()
            .expect("destinations must be a list");

        for message in [error.to_string(), format!("{error:?}")] {
            assert!(message.contains("Invalid YAML configuration"));
            assert!(!message.contains("TEST_SECRET"));
            assert!(!message.contains("https://"));
        }
    }

    #[test]
    fn allows_config_without_authentication() {
        let yaml = r#"
destinations:
  - name: news
    web_hook_url: "https://example.com/webhook"
"#;

        let config = parse_config(yaml).expect("valid YAML should parse");
        let app_config = config
            .into_app_config()
            .expect("authentication should be optional");

        assert!(app_config.api_token.is_none());
    }
}
