//! UI port selection and the single-instance check (docs/M12-desktop-shell.md, Phase B).
//!
//! Binds first and hands the bound listener on, never "check, then bind": order is the
//! configured port, then the next [`FALLBACK_PORTS`] ports, then one the OS assigns. Before
//! falling back from a taken configured port, it asks that port's `/api/version` whether another
//! sequencer runs there: two engines must never drive the same Resolume.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::time::Duration;

/// Ports tried after the configured one (8090 → 8091 … 8099).
pub const FALLBACK_PORTS: u16 = 9;
/// How long the single-instance check waits for an answer.
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug)]
pub enum UiBind {
    Bound {
        listener: TcpListener,
        /// The configured port; differs from the listener's port after a fallback.
        requested: u16,
    },
    /// Another sequencer answers on the configured port.
    AlreadyRunning { url: String },
}

#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error("Port {0} is in use")]
    InUse(u16),
    #[error("UI server on {addr}: {source}")]
    Io {
        addr: SocketAddr,
        source: std::io::Error,
    },
}

/// Taken by someone else (Windows also reports reserved/excluded ports as access denied).
fn taken(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::AddrInUse | ErrorKind::PermissionDenied)
}

fn bind(ip: IpAddr, port: u16) -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind((ip, port))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

pub async fn bind_ui(ip: IpAddr, port: u16, strict: bool) -> Result<UiBind, BindError> {
    let io = |port, source| BindError::Io {
        addr: SocketAddr::new(ip, port),
        source,
    };
    match bind(ip, port) {
        Ok(listener) => {
            return Ok(UiBind::Bound {
                listener,
                requested: port,
            })
        }
        Err(e) if taken(&e) => {}
        Err(e) => return Err(io(port, e)),
    }
    let host = if ip.is_unspecified() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        ip
    };
    let url = format!("http://{}", SocketAddr::new(host, port));
    if is_sequencer(&url).await {
        return Ok(UiBind::AlreadyRunning { url });
    }
    if strict {
        return Err(BindError::InUse(port));
    }
    let candidates = (1..=FALLBACK_PORTS)
        .filter_map(|i| port.checked_add(i))
        .chain([0]);
    for candidate in candidates {
        match bind(ip, candidate) {
            Ok(listener) => {
                let actual = listener.local_addr().map(|a| a.port()).unwrap_or(candidate);
                log::warn!("UI port {port} is in use by another program; using {actual} instead");
                return Ok(UiBind::Bound {
                    listener,
                    requested: port,
                });
            }
            Err(e) if taken(&e) && candidate != 0 => {}
            Err(e) => return Err(io(candidate, e)),
        }
    }
    unreachable!("port 0 either binds or returns an error")
}

/// Whether a sequencer answers `GET <url>/api/version`.
async fn is_sequencer(url: &str) -> bool {
    let Ok(client) = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() else {
        return false;
    };
    let Ok(resp) = client.get(format!("{url}/api/version")).send().await else {
        return false;
    };
    resp.json::<serde_json::Value>()
        .await
        .is_ok_and(|v| v["app"] == crate::APP_ID)
}
