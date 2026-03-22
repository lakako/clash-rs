use ipnet::IpNet;
use tracing::warn;

use crate::{
    app::net::OutboundInterface, common::errors::new_io_error,
    config::internal::config::TunConfig,
};

/// FreeBSD uses the `route` command similar to macOS
pub fn add_route(via: &OutboundInterface, dest: &IpNet) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new("route");
    cmd.arg("add");

    match dest {
        IpNet::V4(_) => {
            cmd.arg("-net")
                .arg(dest.to_string())
                .arg("-interface")
                .arg(&via.name);
            warn!("executing: route add -net {} -interface {}", dest, via.name);
        }
        IpNet::V6(_) => {
            cmd.arg("-inet6")
                .arg(dest.to_string())
                .arg("-interface")
                .arg(&via.name);
            warn!(
                "executing: route add -inet6 {} -interface {}",
                dest, via.name
            );
        }
    }

    let output = cmd.output()?;

    if !output.status.success() {
        Err(new_io_error(format!(
            "add route failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    } else {
        Ok(())
    }
}

fn get_default_gateway()
-> std::io::Result<(Option<std::net::Ipv4Addr>, Option<std::net::Ipv6Addr>)> {
    // IPv4
    let cmd_v4 = std::process::Command::new("route")
        .arg("-n")
        .arg("get")
        .arg("default")
        .output()?;

    let mut gateway_v4 = None;
    if cmd_v4.status.success() {
        let output = String::from_utf8_lossy(&cmd_v4.stdout);
        for line in output.lines() {
            if line.trim().starts_with("gateway:") {
                gateway_v4 = line
                    .split_whitespace()
                    .last()
                    .and_then(|x| x.parse::<std::net::Ipv4Addr>().ok());
                break;
            }
        }
    }

    // IPv6
    let cmd_v6 = std::process::Command::new("route")
        .arg("-n")
        .arg("get")
        .arg("-inet6")
        .arg("default")
        .output()?;

    let mut gateway_v6 = None;
    if cmd_v6.status.success() {
        let output = String::from_utf8_lossy(&cmd_v6.stdout);
        for line in output.lines() {
            if line.trim().starts_with("gateway:") {
                gateway_v6 = line
                    .split_whitespace()
                    .last()
                    .and_then(|x| x.parse::<std::net::Ipv6Addr>().ok());
                break;
            }
        }
    }

    Ok((gateway_v4, gateway_v6))
}

/// Get the default interface name from routing table
fn get_default_interface() -> std::io::Result<String> {
    let cmd = std::process::Command::new("route")
        .arg("-n")
        .arg("get")
        .arg("default")
        .output()?;

    if !cmd.status.success() {
        return Err(new_io_error("failed to get default route"));
    }

    let output = String::from_utf8_lossy(&cmd.stdout);
    for line in output.lines() {
        if line.trim().starts_with("interface:") {
            if let Some(iface) = line.split_whitespace().last() {
                return Ok(iface.to_string());
            }
        }
    }

    Err(new_io_error("default interface not found"))
}

/// Add default route via the original default interface
/// This is used for route_all mode to ensure we can still reach the proxy server
pub fn maybe_add_default_route() -> std::io::Result<()> {
    let (gateway_v4, gateway_v6) = get_default_gateway()?;
    let _default_interface = get_default_interface()?;

    // Add IPv4 default route if gateway found
    if let Some(gateway) = gateway_v4 {
        let cmd = std::process::Command::new("route")
            .arg("add")
            .arg("-net")
            .arg("0.0.0.0/1")
            .arg(gateway.to_string())
            .output()?;

        warn!("executing: route add -net 0.0.0.0/1 {}", gateway);

        if !cmd.status.success() {
            return Err(new_io_error("add default route 0.0.0.0/1 failed"));
        }

        let cmd = std::process::Command::new("route")
            .arg("add")
            .arg("-net")
            .arg("128.0.0.0/1")
            .arg(gateway.to_string())
            .output()?;

        warn!("executing: route add -net 128.0.0.0/1 {}", gateway);

        if !cmd.status.success() {
            return Err(new_io_error("add default route 128.0.0.0/1 failed"));
        }
    }

    if let Some(gateway) = gateway_v6 {
        let cmd = std::process::Command::new("route")
            .arg("add")
            .arg("-inet6")
            .arg("-net")
            .arg("::/1")
            .arg(gateway.to_string())
            .output()?;

        warn!("executing: route add -inet6 -net ::/1 {}", gateway);

        if !cmd.status.success() {
            return Err(new_io_error("add default IPv6 route ::/1 failed"));
        }

        let cmd = std::process::Command::new("route")
            .arg("add")
            .arg("-inet6")
            .arg("-net")
            .arg("8000::/1")
            .arg(gateway.to_string())
            .output()?;

        warn!("executing: route add -inet6 -net 8000::/1 {}", gateway);

        if !cmd.status.success() {
            return Err(new_io_error("add default IPv6 route 8000::/1 failed"));
        }
    }

    if gateway_v4.is_none() && gateway_v6.is_none() {
        Err(new_io_error(
            "cant set default route, default gateway not found",
        ))
    } else {
        Ok(())
    }
}

/// Clean up routes added by route_all mode
pub fn maybe_routes_clean_up(cfg: &TunConfig) -> std::io::Result<()> {
    if !cfg.route_all {
        return Ok(());
    }

    let (gateway_v4, gateway_v6) = get_default_gateway().unwrap_or((None, None));
    let mut result = Ok(());

    // Clean up IPv4 split routes
    if gateway_v4.is_some() {
        let cmd = std::process::Command::new("route")
            .arg("delete")
            .arg("-net")
            .arg("0.0.0.0/1")
            .output();

        if let Ok(output) = cmd {
            warn!("executing: route delete -net 0.0.0.0/1");
            if !output.status.success() {
                result = Err(new_io_error("delete route 0.0.0.0/1 failed"));
            }
        }

        let cmd = std::process::Command::new("route")
            .arg("delete")
            .arg("-net")
            .arg("128.0.0.0/1")
            .output();

        if let Ok(output) = cmd {
            warn!("executing: route delete -net 128.0.0.0/1");
            if !output.status.success() {
                result = Err(new_io_error("delete route 128.0.0.0/1 failed"));
            }
        }
    }

    // Clean up IPv6 split routes
    if gateway_v6.is_some() {
        let cmd = std::process::Command::new("route")
            .arg("delete")
            .arg("-inet6")
            .arg("-net")
            .arg("::/1")
            .output();

        if let Ok(output) = cmd {
            warn!("executing: route delete -inet6 -net ::/1");
            if !output.status.success() {
                result = Err(new_io_error("delete route ::/1 failed"));
            }
        }

        let cmd = std::process::Command::new("route")
            .arg("delete")
            .arg("-inet6")
            .arg("-net")
            .arg("8000::/1")
            .output();

        if let Ok(output) = cmd {
            warn!("executing: route delete -inet6 -net 8000::/1");
            if !output.status.success() {
                result = Err(new_io_error("delete route 8000::/1 failed"));
            }
        }
    }

    result
}
