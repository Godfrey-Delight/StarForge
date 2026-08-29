use crate::utils::{config, http_client, output, print as p};
use anyhow::Result;
use clap::Subcommand;
use std::time::Duration;

#[derive(Subcommand)]
pub enum NetworkCommands {
    /// Show the current active network and available networks
    Show {
        /// Emit a machine-readable JSON object instead of the human-readable output
        #[arg(long)]
        json: bool,
    },
    /// Switch the active network (testnet, mainnet, or custom)
    Switch {
        /// Target network to switch to
        network: String,
    },
    /// Add a custom network endpoint
    Add {
        /// Name for the custom network
        name: String,
        /// Horizon API URL
        #[arg(long)]
        horizon_url: String,
        /// Optional Soroban RPC URL
        #[arg(long)]
        soroban_rpc_url: Option<String>,
        /// Optional network faucet / Friendbot URL
        #[arg(long)]
        friendbot_url: Option<String>,
        /// Optional network passphrase for transaction signing (defaults to testnet passphrase)
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Test connectivity to a network
    Test {
        /// Network to test (defaults to current active network)
        #[arg(default_value = None)]
        network: Option<String>,
    },
    /// Remove a custom network from configuration
    Remove {
        /// Name of the custom network to remove
        name: String,
    },
    /// Rename a custom network
    Rename {
        /// Current network name
        old_name: String,
        /// New network name
        new_name: String,
    },
}

pub async fn handle(cmd: NetworkCommands) -> Result<()> {
    match cmd {
        NetworkCommands::Show { json } => show(json),
        NetworkCommands::Switch { network } => switch(network),
        NetworkCommands::Add {
            name,
            horizon_url,
            soroban_rpc_url,
            friendbot_url,
            passphrase,
        } => add_network(
            name,
            horizon_url,
            soroban_rpc_url,
            friendbot_url,
            passphrase,
        ),
        NetworkCommands::Test { network } => test_network(network).await,
        NetworkCommands::Remove { name } => remove_network(name),
        NetworkCommands::Rename { old_name, new_name } => rename_network(old_name, new_name),
    }
}

fn show(json: bool) -> Result<()> {
    let cfg = config::load()?;
    let emit_json = json || output::is_json_mode_enabled();

    if emit_json {
        #[derive(serde::Serialize)]
        struct NetworkEntry {
            name: String,
            horizon_url: String,
            soroban_rpc_url: Option<String>,
            friendbot_url: Option<String>,
            active: bool,
        }

        #[derive(serde::Serialize)]
        struct NetworkResponse {
            active_network: String,
            networks: Vec<NetworkEntry>,
        }

        let networks = cfg
            .networks
            .iter()
            .map(|(name, net_cfg)| NetworkEntry {
                name: name.clone(),
                horizon_url: net_cfg.horizon_url.clone(),
                soroban_rpc_url: net_cfg.soroban_rpc_url.clone(),
                friendbot_url: net_cfg.friendbot_url.clone(),
                active: cfg.network == *name,
            })
            .collect();

        return output::print_json(&NetworkResponse {
            active_network: cfg.network.clone(),
            networks,
        });
    }

    p::header("Networks");
    p::separator();

    for (name, net_cfg) in &cfg.networks {
        let active = if cfg.network == *name { " ✓" } else { "" };
        println!("  {} {}", name.to_uppercase(), active);
        p::kv("Horizon", &net_cfg.horizon_url);
        if let Some(soroban_url) = &net_cfg.soroban_rpc_url {
            p::kv("Soroban RPC", soroban_url);
        }
        if let Some(friendbot_url) = &net_cfg.friendbot_url {
            p::kv("Friendbot", friendbot_url);
        }
        println!();
    }

    p::separator();
    p::info(&format!("Active network: {}", cfg.network));
    Ok(())
}

fn switch(target: String) -> Result<()> {
    let mut cfg = config::load()?;

    // Validate network exists (accepts built-ins + custom networks)
    config::validate_network_exists(&cfg, &target)?;

    // Check if already on the target network
    if cfg.network == target {
        p::info(&format!("Already on {}. No changes made.", target));
        return Ok(());
    }

    let previous = cfg.network.clone();
    cfg.network = target.clone();
    config::save(&cfg)?;

    // Print mainnet warning
    if target == "mainnet" {
        p::warn("You are now on MAINNET. Transactions use real funds!");
        p::warn("Double-check all addresses and amounts before sending.");
    }

    p::success(&format!(
        "Network switched from {} to {}.",
        previous, target
    ));

    Ok(())
}

pub fn validate_url(label: &str, url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        anyhow::bail!("{} URL cannot be empty", label);
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|e| {
        anyhow::anyhow!("Invalid {} URL '{}': {}", label, url, e)
    })?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        anyhow::bail!("{} URL scheme must be http or https, got '{}'", label, parsed.scheme());
    }
    if parsed.host_str().is_none() {
        anyhow::bail!("{} URL missing valid host", label);
    }
    Ok(())
}

pub fn validate_passphrase(passphrase: &Option<String>) -> Result<()> {
    if let Some(ref p) = passphrase {
        if p.trim().is_empty() {
            anyhow::bail!("Network passphrase cannot be empty or only whitespace");
        }
    }
    Ok(())
}

fn add_network(
    name: String,
    horizon_url: String,
    soroban_rpc_url: Option<String>,
    friendbot_url: Option<String>,
    passphrase: Option<String>,
) -> Result<()> {
    let mut cfg = config::load()?;

    validate_url("Horizon", &horizon_url)?;

    if let Some(ref url) = soroban_rpc_url {
        validate_url("Soroban RPC", url)?;
    }

    if let Some(ref url) = friendbot_url {
        validate_url("Friendbot", url)?;
    }

    validate_passphrase(&passphrase)?;

    // Normalize trailing slashes so URL construction is consistent downstream
    let horizon_url = horizon_url.trim().trim_end_matches('/').to_string();
    let soroban_rpc_url = soroban_rpc_url.map(|u| u.trim().trim_end_matches('/').to_string());
    let friendbot_url = friendbot_url.map(|u| u.trim().trim_end_matches('/').to_string());

    config::add_custom_network(
        &mut cfg,
        name.clone(),
        horizon_url.clone(),
        soroban_rpc_url.clone(),
        friendbot_url.clone(),
        passphrase,
    )?;
    config::save(&cfg)?;

    p::success(&format!("Network '{}' added successfully", name));
    p::kv("Horizon", &horizon_url);
    if let Some(url) = soroban_rpc_url {
        p::kv("Soroban RPC", &url);
    }
    if let Some(url) = friendbot_url {
        p::kv("Friendbot", &url);
    }
    Ok(())
}

async fn test_network(network_name: Option<String>) -> Result<()> {
    let cfg = config::load()?;
    let test_network = network_name.unwrap_or_else(|| cfg.network.clone());

    let net_cfg = config::get_network_config(&cfg, &test_network)?;

    p::info(&format!("Testing connectivity to '{}'…", test_network));
    p::info(&format!("Horizon: {}", net_cfg.horizon_url));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .build()?;

    // Test Horizon endpoint
    let client = http_client::get_client();
    match client
        .get(&format!("{}/health", net_cfg.horizon_url))
        .send()
        .await
    {
        Ok(_) => {
            p::success("✓ Horizon endpoint is reachable");
        }
        Err(e) => {
            p::warn(&format!("✗ Horizon endpoint failed: {}", e));
        }
    }

    // Test Soroban RPC if available
    if let Some(soroban_url) = &net_cfg.soroban_rpc_url {
        p::info(&format!("Soroban RPC: {}", soroban_url));
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestLedger",
            "params": []
        });

        match client.post(soroban_url).json(&req).send().await {
            Ok(_) => {
                p::success("✓ Soroban RPC endpoint is reachable");
            }
            Err(e) => {
                p::warn(&format!("✗ Soroban RPC endpoint failed: {}", e));
            }
        }
    }

    p::info("Network test complete");
    Ok(())
}

fn remove_network(name: String) -> Result<()> {
    let mut cfg = config::load()?;

    let was_active = cfg.network == name;
    config::remove_custom_network(&mut cfg, &name)?;
    config::save(&cfg)?;

    p::success(&format!("Network '{}' removed", name));
    if was_active {
        p::warn("Active network was removed; switched to testnet.");
        p::kv("Active network", "testnet");
    }
    Ok(())
}

fn rename_network(old_name: String, new_name: String) -> Result<()> {
    let mut cfg = config::load()?;
    config::rename_custom_network(&mut cfg, &old_name, &new_name)?;
    config::save(&cfg)?;

    p::success(&format!(
        "Network renamed from '{}' to '{}'",
        old_name, new_name
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url("Horizon", "https://horizon.stellar.org").is_ok());
        assert!(validate_url("Horizon", "http://localhost:8000").is_ok());
    }

    #[test]
    fn test_validate_url_invalid() {
        assert!(validate_url("Horizon", "").is_err());
        assert!(validate_url("Horizon", "   ").is_err());
        assert!(validate_url("Horizon", "ftp://stellar.org").is_err());
        assert!(validate_url("Horizon", "not-a-valid-url").is_err());
    }

    #[test]
    fn test_validate_passphrase() {
        assert!(validate_passphrase(&None).is_ok());
        assert!(validate_passphrase(&Some("Test SDF Network ; September 2015".to_string())).is_ok());
        assert!(validate_passphrase(&Some("".to_string())).is_err());
        assert!(validate_passphrase(&Some("   ".to_string())).is_err());
    }
}

