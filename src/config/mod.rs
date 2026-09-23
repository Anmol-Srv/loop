use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    /// The interface to listen on. `0.0.0.0` inside a container, where the
    /// only way in is the container network and the proxy in front of it;
    /// `127.0.0.1` on a bare VM, so nothing reaches the plain-HTTP port except
    /// through the TLS proxy on the same machine.
    pub host: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
        })
    }
}
