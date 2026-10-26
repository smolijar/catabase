use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectionOptions {
    pub connect_timeout_seconds: u32,
    pub query_timeout_seconds: u32,
    pub lock_timeout_seconds: u32,
    pub read_only: bool,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: 8,
            query_timeout_seconds: 0,
            lock_timeout_seconds: 0,
            read_only: false,
        }
    }
}

impl ConnectionOptions {
    pub fn validate(&self) -> Result<(), CoreError> {
        if !(1..=300).contains(&self.connect_timeout_seconds) {
            return Err(CoreError::message(
                "Connection timeout must be between 1 and 300 seconds.",
            ));
        }
        if self.query_timeout_seconds > 86400 || self.lock_timeout_seconds > 86400 {
            return Err(CoreError::message(
                "Query and lock timeouts must be between 0 and 86400 seconds.",
            ));
        }
        Ok(())
    }

    /// Startup parameters avoid extra setup round trips. Unset options preserve
    /// server/role defaults, including stricter server read-only/timeout settings.
    pub fn startup_parameters(&self) -> String {
        let mut parameters = Vec::new();
        if self.query_timeout_seconds > 0 {
            parameters.push(format!(
                "-c statement_timeout={}",
                self.query_timeout_seconds * 1000
            ));
        }
        if self.lock_timeout_seconds > 0 {
            parameters.push(format!(
                "-c lock_timeout={}",
                self.lock_timeout_seconds * 1000
            ));
        }
        if self.read_only {
            parameters.push("-c default_transaction_read_only=on".into());
        }
        parameters.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_connections_preserve_their_timeouts_and_server_defaults() {
        let defaults: ConnectionOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults, ConnectionOptions::default());
        assert_eq!(defaults.connect_timeout_seconds, 8);
        assert!(defaults.startup_parameters().is_empty());
    }

    #[test]
    fn validates_options_and_builds_numeric_postgres_startup_parameters() {
        let options = ConnectionOptions {
            connect_timeout_seconds: 12,
            query_timeout_seconds: 30,
            lock_timeout_seconds: 2,
            read_only: true,
        };
        assert!(options.validate().is_ok());
        assert_eq!(
            options.startup_parameters(),
            "-c statement_timeout=30000 -c lock_timeout=2000 -c default_transaction_read_only=on"
        );
        for invalid in [
            ConnectionOptions {
                connect_timeout_seconds: 0,
                ..options.clone()
            },
            ConnectionOptions {
                connect_timeout_seconds: 301,
                ..options.clone()
            },
            ConnectionOptions {
                query_timeout_seconds: 86401,
                ..options.clone()
            },
            ConnectionOptions {
                lock_timeout_seconds: 86401,
                ..options.clone()
            },
        ] {
            assert!(invalid.validate().is_err());
        }
    }
}
