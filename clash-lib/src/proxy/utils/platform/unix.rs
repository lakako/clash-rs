use std::io;

use crate::app::net::OutboundInterface;

pub(crate) fn must_bind_socket_on_interface(
    #[allow(unused_variables)] socket: &socket2::Socket,
    iface: &OutboundInterface,
    #[allow(unused_variables)] family: socket2::Domain,
) -> io::Result<()> {
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux",))]
    {
        use tracing::error;
        socket
            .bind_device(Some(iface.name.as_bytes()))
            .inspect_err(|e| {
                error!("failed to bind socket to interface {}: {e}", iface.name);
            })
    }
    #[cfg(target_os = "freebsd")]
    {
        // FreeBSD: bind to the interface's IP address
        // This ensures both outbound and inbound traffic go through the interface
        use std::net::{IpAddr, SocketAddr};
        use tracing::warn;

        // Prefer IPv4 address, fallback to IPv6
        let addr = iface.addr_v4.map(IpAddr::V4).or(iface.addr_v6.map(IpAddr::V6));
        
        if let Some(addr) = addr {
            let bind_addr = SocketAddr::new(addr, 0);
            socket.bind(&bind_addr.into()).inspect_err(|e| {
                warn!(
                    "failed to bind socket to interface {} ({}): {}",
                    iface.name, addr, e
                );
            })
        } else {
            warn!("interface {} has no IP address, skipping bind", iface.name);
            Ok(())
        }
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "fuchsia",
        target_os = "linux",
        target_os = "freebsd",
    )))]
    {
        use crate::common::errors::new_io_error;
        Err(new_io_error(format!(
            "unsupported platform: {}",
            iface.name
        )))
    }
}
