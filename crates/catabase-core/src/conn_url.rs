use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::CoreError;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionFields {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub ssl: bool,
    #[serde(default)]
    pub options: crate::ConnectionOptions,
}

pub fn parse_connection_url(input: &str) -> Result<ConnectionFields, CoreError> {
    let url = Url::parse(input.trim())?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(CoreError::InvalidConnectionUrl(
            "scheme must be postgres:// or postgresql://".to_string(),
        ));
    }

    let host = url
        .host_str()
        .ok_or_else(|| CoreError::InvalidConnectionUrl("missing host".to_string()))?
        .to_string();
    let port = url.port().unwrap_or(5432);
    let database = url.path().trim_start_matches('/');
    if database.is_empty() {
        return Err(CoreError::InvalidConnectionUrl(
            "missing database".to_string(),
        ));
    }

    let username = decode(url.username());
    if username.is_empty() {
        return Err(CoreError::InvalidConnectionUrl(
            "missing username".to_string(),
        ));
    }

    let password = url.password().map(decode).filter(|value| !value.is_empty());
    let ssl = url
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("sslmode"))
        .map(|(_, value)| matches!(value.as_ref(), "require" | "verify-ca" | "verify-full"))
        .unwrap_or(false);

    Ok(ConnectionFields {
        name: String::new(),
        host,
        port,
        database: decode(database),
        username,
        password,
        ssl,
        options: Default::default(),
    })
}

pub fn build_connection_url(fields: &ConnectionFields) -> Result<String, CoreError> {
    let mut url = Url::parse("postgresql://localhost")?;
    url.set_host(Some(&fields.host))
        .map_err(|_| CoreError::InvalidConnectionUrl("invalid host".to_string()))?;
    url.set_port(Some(fields.port))
        .map_err(|_| CoreError::InvalidConnectionUrl("invalid port".to_string()))?;
    url.set_username(&fields.username)
        .map_err(|_| CoreError::InvalidConnectionUrl("invalid username".to_string()))?;
    url.set_password(fields.password.as_deref())
        .map_err(|_| CoreError::InvalidConnectionUrl("invalid password".to_string()))?;
    url.set_path(&format!("/{}", fields.database));
    url.query_pairs_mut()
        .clear()
        .append_pair("sslmode", if fields.ssl { "require" } else { "disable" });
    Ok(url.to_string())
}

fn decode(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_postgres_url() {
        let fields = parse_connection_url(
            "postgresql://cat%20user:p%40ss@db.example.com:5433/cat%20db?sslmode=require",
        )
        .unwrap();

        assert_eq!(fields.username, "cat user");
        assert_eq!(fields.password.as_deref(), Some("p@ss"));
        assert_eq!(fields.host, "db.example.com");
        assert_eq!(fields.port, 5433);
        assert_eq!(fields.database, "cat db");
        assert!(fields.ssl);
    }

    #[test]
    fn round_trips_url() {
        let fields = ConnectionFields {
            name: "Local".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "cat db".to_string(),
            username: "cat user".to_string(),
            password: Some("secret".to_string()),
            ssl: false,
            options: Default::default(),
        };

        let parsed = parse_connection_url(&build_connection_url(&fields).unwrap()).unwrap();
        assert_eq!(parsed.host, fields.host);
        assert_eq!(parsed.port, fields.port);
        assert_eq!(parsed.database, fields.database);
        assert_eq!(parsed.username, fields.username);
        assert_eq!(parsed.password, fields.password);
        assert_eq!(parsed.ssl, fields.ssl);
    }
}
