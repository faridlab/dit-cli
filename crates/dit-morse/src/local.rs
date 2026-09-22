//! `.dit/morse.local.yaml` — this machine's environments, and the hosts it
//! will let a scenario reach (§20.5, §20.6).
//!
//! This file is never committed. It is the one place a secret may live, and
//! the one place a host becomes trusted, precisely because it does not travel
//! with the repository: a scenario that arrives in a pull request cannot
//! bring its own permission with it.
//!
//! Two environment variables overlay it, for the one caller that has no such
//! file — a CI job. They are explicit by construction: a pipeline step names
//! the hosts it means and the values it supplies, in the workflow file a
//! person reviews. That is still someone asking, which is what §20.5 draws
//! its line around.

use std::collections::BTreeMap;

use dit_parse::{Yaml, YamlError};

/// The env var naming the hosts a run may reach, comma-separated.
pub const ALLOW_HOSTS_VAR: &str = "DIT_MORSE_ALLOW_HOSTS";
/// The env var supplying variable values, as `name=value` pairs.
pub const VARS_VAR: &str = "DIT_MORSE_VARS";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocalError {
    #[error("{0}")]
    Yaml(#[from] YamlError),
    #[error("`envs:` must be a mapping of environment name to its settings")]
    BadEnvs,
    #[error("`allow_hosts:` must be a list of host names")]
    BadAllowHosts,
}

/// One environment's values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalEnv {
    /// Overrides the server the spec's `servers:` entry names. Absent is the
    /// normal case — the API states where it lives, not DIT.
    pub server: Option<String>,
    pub vars: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalConfig {
    pub envs: BTreeMap<String, LocalEnv>,
    /// Hosts this machine will let a run reach. An entry is a host, which
    /// allows any port on it, or `host:port`, which allows exactly that one.
    pub allow_hosts: Vec<String>,
}

impl LocalConfig {
    pub fn parse(text: &str) -> Result<LocalConfig, LocalError> {
        let root = dit_parse::parse_yaml(text)?;
        let mut envs = BTreeMap::new();
        match root.get("envs") {
            None | Some(Yaml::Null) => {}
            Some(Yaml::Map(entries)) => {
                for (name, node) in entries {
                    let mut env = LocalEnv {
                        server: node
                            .get("server")
                            .and_then(Yaml::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned),
                        ..Default::default()
                    };
                    if let Some(Yaml::Map(vars)) = node.get("vars") {
                        for (key, value) in vars {
                            if let Some(text) = value.as_str() {
                                env.vars.insert(key.clone(), text.to_owned());
                            }
                        }
                    }
                    envs.insert(name.clone(), env);
                }
            }
            Some(_) => return Err(LocalError::BadEnvs),
        }
        let allow_hosts = match root.get("allow_hosts") {
            None | Some(Yaml::Null) => Vec::new(),
            Some(Yaml::Seq(items)) => items
                .iter()
                .filter_map(Yaml::as_str)
                .map(str::trim)
                .filter(|h| !h.is_empty())
                .map(str::to_ascii_lowercase)
                .collect(),
            Some(_) => return Err(LocalError::BadAllowHosts),
        };
        Ok(LocalConfig { envs, allow_hosts })
    }

    /// Emit the file back, so `dit morse allow` can add a host without
    /// asking a person to hand-edit YAML. Only ever written to the local,
    /// gitignored path — never through a transaction, because this is not a
    /// DIT file and must not reach a commit.
    pub fn write(&self) -> String {
        let mut out = String::from(
            "# Morse environments and the hosts this machine allows (§20.6).\n\
             # Never commit this file — it holds values, and git does not forget.\n",
        );
        if !self.envs.is_empty() {
            out.push_str("envs:\n");
            for (name, env) in &self.envs {
                out.push_str(&format!("  {name}:\n"));
                if let Some(server) = &env.server {
                    out.push_str(&format!("    server: {}\n", quoted(server)));
                }
                if !env.vars.is_empty() {
                    out.push_str("    vars:\n");
                    for (key, value) in &env.vars {
                        out.push_str(&format!("      {key}: {}\n", quoted(value)));
                    }
                }
            }
        }
        out.push_str("allow_hosts:\n");
        for host in &self.allow_hosts {
            out.push_str(&format!("  - {}\n", quoted(host)));
        }
        out
    }

    /// Add a host, keeping the list sorted and free of duplicates. Returns
    /// whether it was new, so a command can say "already allowed" rather
    /// than pretending to have done something.
    pub fn allow(&mut self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() || self.allow_hosts.contains(&host) {
            return false;
        }
        self.allow_hosts.push(host);
        self.allow_hosts.sort();
        true
    }

    /// Fold in `DIT_MORSE_ALLOW_HOSTS` and `DIT_MORSE_VARS`, for a caller
    /// with no local file. The values are added to the named environment,
    /// which is created if it does not exist.
    pub fn overlay(&mut self, env_name: &str, allow: Option<&str>, vars: Option<&str>) {
        if let Some(list) = allow {
            for host in list.split(',') {
                self.allow(host);
            }
        }
        if let Some(pairs) = vars {
            let env = self.envs.entry(env_name.to_owned()).or_default();
            for pair in pairs.split(',') {
                if let Some((name, value)) = pair.split_once('=') {
                    let name = name.trim();
                    if !name.is_empty() {
                        env.vars.insert(name.to_owned(), value.to_owned());
                    }
                }
            }
        }
    }

    /// Whether a run may reach this authority. `host` allows any port on it;
    /// `host:port` allows exactly one. Nothing is implicit — not localhost,
    /// not a suffix, not a wildcard: a rule that guesses is a rule that
    /// eventually guesses wrong in someone's favour.
    pub fn allows(&self, host: &str, port: Option<u16>) -> bool {
        let host = host.trim().to_ascii_lowercase();
        self.allow_hosts
            .iter()
            .any(|entry| match entry.split_once(':') {
                Some((h, p)) => h == host && port.is_some_and(|port| p == port.to_string()),
                None => entry == &host,
            })
    }
}

fn quoted(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@'));
    if safe {
        s.to_owned()
    } else {
        format!("{s:?}")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const FILE: &str = r#"envs:
  local:
    server: "http://localhost:4000"
    vars:
      email: "dev@acme.test"
      password: "hunter2"
  staging:
    vars:
      email: "ci@acme.test"
allow_hosts:
  - localhost
  - "api.staging.acme.com:443"
"#;

    #[test]
    fn the_local_file_carries_environments_and_the_hosts_this_machine_trusts() {
        let cfg = LocalConfig::parse(FILE).unwrap();
        assert_eq!(cfg.envs.len(), 2);
        let local = &cfg.envs["local"];
        assert_eq!(local.server.as_deref(), Some("http://localhost:4000"));
        assert_eq!(local.vars["password"], "hunter2");
        assert_eq!(
            cfg.envs["staging"].server, None,
            "the spec says where it lives"
        );
        assert_eq!(cfg.allow_hosts.len(), 2);
    }

    #[test]
    fn nothing_is_allowed_implicitly() {
        let cfg = LocalConfig::parse(FILE).unwrap();
        assert!(
            cfg.allows("localhost", Some(3000)),
            "a bare host allows any port"
        );
        assert!(
            cfg.allows("LOCALHOST", Some(9999)),
            "host matching ignores case"
        );
        assert!(cfg.allows("api.staging.acme.com", Some(443)));
        assert!(
            !cfg.allows("api.staging.acme.com", Some(8443)),
            "`host:port` allows exactly one port"
        );
        assert!(
            !cfg.allows("127.0.0.1", Some(3000)),
            "127.0.0.1 is not localhost — a rule that guesses eventually guesses wrong"
        );
        assert!(
            !cfg.allows("evil.acme.com", Some(443)),
            "and a suffix is not a match"
        );
        assert!(!LocalConfig::default().allows("anything", None));
    }

    #[test]
    fn a_host_added_survives_a_round_trip_through_the_file() {
        let mut cfg = LocalConfig::parse(FILE).unwrap();
        assert!(cfg.allow("staging.acme.com"));
        assert!(!cfg.allow("staging.acme.com"), "twice is not new");
        assert!(!cfg.allow("  "), "and neither is nothing");
        let back = LocalConfig::parse(&cfg.write()).unwrap();
        assert_eq!(back, cfg, "what the command wrote is what it reads back");
        assert!(back.allows("staging.acme.com", None));
    }

    #[test]
    fn a_pipeline_supplies_hosts_and_values_through_the_environment() {
        let mut cfg = LocalConfig::default();
        cfg.overlay(
            "ci",
            Some("staging.acme.com,api.staging.acme.com:443"),
            Some("email=ci@acme.test,password=s3cret"),
        );
        assert!(cfg.allows("staging.acme.com", Some(443)));
        assert!(cfg.allows("api.staging.acme.com", Some(443)));
        assert_eq!(cfg.envs["ci"].vars["email"], "ci@acme.test");
        assert_eq!(cfg.envs["ci"].vars["password"], "s3cret");
    }

    #[test]
    fn a_file_that_is_not_a_mapping_says_so_rather_than_being_ignored() {
        assert!(matches!(
            LocalConfig::parse("envs: [a, b]\n"),
            Err(LocalError::BadEnvs)
        ));
        assert!(matches!(
            LocalConfig::parse("allow_hosts: nope\n"),
            Err(LocalError::BadAllowHosts)
        ));
        assert_eq!(LocalConfig::parse("").unwrap(), LocalConfig::default());
    }
}
