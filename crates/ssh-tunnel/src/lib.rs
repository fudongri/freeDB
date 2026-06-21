use anyhow::{bail, Result};
use core_domain::SshTunnelConfig;

#[derive(Clone, Default)]
pub struct SshTunnelManager;

impl SshTunnelManager {
    pub fn validate(&self, config: Option<&SshTunnelConfig>) -> Result<()> {
        if let Some(config) = config {
            if config.enabled && (config.host.trim().is_empty() || config.username.trim().is_empty()) {
                bail!("ssh tunnel host and username are required");
            }
        }
        Ok(())
    }

    pub fn prepare_endpoint(&self, host: &str, port: u16, _config: Option<&SshTunnelConfig>) -> Result<(String, u16)> {
        Ok((host.to_string(), port))
    }
}
