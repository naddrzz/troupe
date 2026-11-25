//! Terminal QR code rendering, LAN IPv4 detection, and the `troupe-server qr`
//! subcommand.
//!
//! Unconditionally compiled into `termul_manager_lib` (mirroring `onboard` and
//! `server_update`) so its test suite runs under default `cargo test` and
//! `web::router::server_config` can share [`detect_lan_ipv4`]. Only the
//! `troupe-server qr` CLI dispatch in `server_main.rs` is gated by
//! `standalone-server`.
//!
//! ## Security invariants
//! - When a bearer token is embedded (`#token=...`), printing to non-TTY stdout
//!   is refused so secrets never land in systemd journals, CI logs, or
//!   redirected files (`--no-token` opts out of embedding the token).
//! - Persisted tokens are loaded through [`crate::web::auth::load_persisted_token`]
//!   (symlink-safe `O_NOFOLLOW` + regular-file + owner-only permission checks).
//! - Neither `tracing` logs nor the text caption below the QR ever print the
//!   raw bearer token.

use std::io::{IsTerminal, Write};
use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use qrcode::{Color, EcLevel, QrCode};
use tracing::{info, warn};

use crate::brand_env;
use crate::web::auth::{load_persisted_token, WebAuth, WebAuthToken};
use crate::web::config::BindMode;
use crate::web::server_config_file;

/// Default port when neither CLI nor `server-config.json` specifies one.
const DEFAULT_PORT: u16 = 8080;

/// Quiet-zone margin in QR modules on each side (2 modules is compact yet
/// reliably scannable on phone cameras).
const QUIET_ZONE_MODULES: usize = 2;

/// Detect the host's primary outbound non-loopback IPv4 address using a
/// stdlib UDP routing-table probe (zero packets are transmitted).
///
/// Returns `None` on isolated/unrouted hosts or if only loopback/unspecified
/// addresses are available.
#[must_use]
pub fn detect_lan_ipv4() -> Option<IpAddr> {
    for probe in ["1.1.1.1:80", "10.255.255.255:80"] {
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        if socket.connect(probe).is_err() {
            continue;
        }
        if let Ok(local) = socket.local_addr() {
            let ip = local.ip();
            if ip.is_ipv4() && !ip.is_loopback() && !ip.is_unspecified() {
                return Some(ip);
            }
        }
    }
    None
}

/// Render `payload` as a compact, square-aspect Unicode half-block QR code for
/// terminal display (`EcLevel::M`, matching `RemoteAccessPopover`).
///
/// - When `color` is `true`, each row is wrapped in ANSI black-on-white SGR
///   (`\x1b[30;47m...\x1b[0m`) so phone cameras scan reliably on both dark and
///   light terminal themes.
/// - When `color` is `false` (`--plain` or `NO_COLOR`), emits plain Unicode
///   half-blocks inverted for dark-terminal contrast (white quiet zone via
///   full-block foreground characters) with zero ANSI escape codes.
pub fn render_terminal_qr(payload: &str, color: bool) -> Result<String, String> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M)
        .map_err(|e| format!("failed to encode QR code: {e}"))?;
    let width = code.width();
    let total = width + 2 * QUIET_ZONE_MODULES;

    let is_dark = |x: usize, y: usize| -> bool {
        if x < QUIET_ZONE_MODULES
            || y < QUIET_ZONE_MODULES
            || x >= QUIET_ZONE_MODULES + width
            || y >= QUIET_ZONE_MODULES + width
        {
            false
        } else {
            code[(x - QUIET_ZONE_MODULES, y - QUIET_ZONE_MODULES)] == Color::Dark
        }
    };

    let rows = total.div_ceil(2);
    let mut out = String::with_capacity(rows * (total * 4 + 16));
    for row_idx in 0..rows {
        let y_top = row_idx * 2;
        let y_bot = y_top + 1;
        if color {
            out.push_str("\x1b[30;47m");
        }
        for x in 0..total {
            let top = is_dark(x, y_top);
            let bot = y_bot < total && is_dark(x, y_bot);
            let ch = if color {
                // Foreground = black (dark module), background = white (light module).
                match (top, bot) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                }
            } else {
                // Plain dark-terminal mode: foreground = light, background = dark.
                match (top, bot) {
                    (true, true) => ' ',
                    (true, false) => '▄',
                    (false, true) => '▀',
                    (false, false) => '█',
                }
            };
            out.push(ch);
        }
        if color {
            out.push_str("\x1b[0m");
        }
        if row_idx + 1 < rows {
            out.push('\n');
        }
    }
    Ok(out)
}

/// Whether ANSI color should be used for terminal QR output (honors `NO_COLOR`).
#[must_use]
pub fn terminal_color_enabled(plain_flag: bool) -> bool {
    if plain_flag {
        return false;
    }
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Parsed CLI flags for `troupe-server qr`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct QrCliArgs {
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub state_dir: Option<PathBuf>,
    pub web_auth_token: Option<WebAuthToken>,
    pub no_token: bool,
    pub plain: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QrCliParseError {
    Help,
    Message(String),
}

pub(crate) fn parse_qr_args(args: &[String]) -> Result<QrCliArgs, QrCliParseError> {
    let mut parsed = QrCliArgs::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(QrCliParseError::Help),
            "--url" => {
                let value = iter
                    .next()
                    .ok_or_else(|| QrCliParseError::Message("missing value for --url".into()))?;
                parsed.url = Some(value.clone());
            }
            "--host" => {
                let value = iter
                    .next()
                    .ok_or_else(|| QrCliParseError::Message("missing value for --host".into()))?;
                parsed.host = Some(value.clone());
            }
            "--port" => {
                let value = iter
                    .next()
                    .ok_or_else(|| QrCliParseError::Message("missing value for --port".into()))?;
                let port: u16 = value.trim().parse().map_err(|_| {
                    QrCliParseError::Message(format!(
                        "invalid --port '{value}': expected 1-65535"
                    ))
                })?;
                if port == 0 {
                    return Err(QrCliParseError::Message(
                        "invalid --port '0': expected 1-65535".into(),
                    ));
                }
                parsed.port = Some(port);
            }
            "--state-dir" => {
                let value = iter.next().ok_or_else(|| {
                    QrCliParseError::Message("missing value for --state-dir".into())
                })?;
                if value.trim().is_empty() {
                    return Err(QrCliParseError::Message(
                        "invalid --state-dir: path cannot be empty".into(),
                    ));
                }
                parsed.state_dir = Some(PathBuf::from(value));
            }
            "--web-auth-token" => {
                let value = iter.next().ok_or_else(|| {
                    QrCliParseError::Message("missing value for --web-auth-token".into())
                })?;
                let token = WebAuthToken::new(value).ok_or_else(|| {
                    QrCliParseError::Message(
                        "invalid --web-auth-token: token cannot be empty".into(),
                    )
                })?;
                parsed.web_auth_token = Some(token);
            }
            "--no-token" => parsed.no_token = true,
            "--plain" => parsed.plain = true,
            other => {
                return Err(QrCliParseError::Message(format!(
                    "unknown option '{other}' for 'troupe-server qr'"
                )));
            }
        }
    }
    Ok(parsed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetSource {
    Url,
    Host,
    LanIp,
}

impl TargetSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Host => "host",
            Self::LanIp => "lan_ip",
        }
    }
}

pub(crate) struct ResolvedQrTarget {
    /// Bare URL without `#token=` (safe to print as visible text).
    pub bare_url: String,
    /// Full URL encoded inside the QR code (includes `#token=...` when active).
    pub qr_payload: String,
    pub has_token: bool,
    pub is_plaintext_http: bool,
    pub source: TargetSource,
}

/// Format `http://<host>:<port>/`, bracketing raw IPv6 addresses.
fn format_http_origin(host: &str, port: u16) -> String {
    let trimmed = host.trim();
    if trimmed.contains(':') && !trimmed.starts_with('[') {
        format!("http://[{trimmed}]:{port}/")
    } else {
        format!("http://{trimmed}:{port}/")
    }
}

/// Validate and normalize an explicit `--url` base URL, stripping any fragment
/// so `WebAuth::share_url` appends `#token=` cleanly.
fn normalize_explicit_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let parsed = url::Url::parse(trimmed)
        .map_err(|e| format!("invalid --url '{trimmed}': {e} (expected http:// or https://)"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "invalid --url '{trimmed}': scheme must be http or https"
        ));
    }
    if parsed.host_str().is_none() {
        return Err(format!("invalid --url '{trimmed}': missing host"));
    }
    let mut clean = parsed;
    clean.set_fragment(None);
    Ok(clean.to_string())
}

fn resolve_state_dir(override_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir.to_path_buf();
    }
    crate::onboard::OnboardAnswers::defaults()
        .to_server_config()
        .service_account_state_dir()
}

pub(crate) fn resolve_qr_target(
    args: &QrCliArgs,
    state_dir: &Path,
    env_token: Option<WebAuthToken>,
    lan_ip: Option<IpAddr>,
) -> Result<ResolvedQrTarget, String> {
    // Load persisted server config when present (a missing file falls back to
    // defaults; a corrupt file only blocks auto-resolution, never explicit --url).
    let saved_config = match server_config_file::load(state_dir) {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!(
                target: "termul::server_qr",
                error = %e,
                "could not load server-config.json; falling back to CLI flags/defaults"
            );
            None
        }
    };

    let resolved_port = args
        .port
        .or_else(|| saved_config.as_ref().map(|c| c.port))
        .unwrap_or(DEFAULT_PORT);

    let (bare_url, source) = if let Some(explicit_url) = &args.url {
        (normalize_explicit_url(explicit_url)?, TargetSource::Url)
    } else if let Some(explicit_host) = &args.host {
        let trimmed = explicit_host.trim();
        if trimmed.is_empty() {
            return Err("invalid --host: hostname or IP cannot be empty".into());
        }
        if BindMode::parse(trimmed) == Some(BindMode::All) {
            return Err(format!(
                "'{trimmed}' is a bind wildcard, not a reachable phone address. \
                 Pass your machine's LAN IP/hostname via --host <ip> or a full --url <https://...>."
            ));
        }
        (format_http_origin(trimmed, resolved_port), TargetSource::Host)
    } else {
        let configured_host = saved_config
            .as_ref()
            .map(|c| c.host.as_str())
            .unwrap_or("127.0.0.1");
        let bind_mode = BindMode::parse(configured_host);
        if bind_mode == Some(BindMode::All) {
            let ip = lan_ip.ok_or_else(|| {
                "could not auto-detect a LAN IPv4 address for this machine. \
                 Pass --host <lan-ip-or-hostname> or --url <https://...>."
                    .to_string()
            })?;
            (
                format_http_origin(&ip.to_string(), resolved_port),
                TargetSource::LanIp,
            )
        } else {
            return Err(
                "this troupe-server is configured for localhost only (127.0.0.1), so other \
                 devices cannot reach it. Switch to all interfaces (0.0.0.0) in Settings -> \
                 Server or `troupe-server onboard`, or pass --url <https://...> if behind a \
                 reverse proxy."
                    .into(),
            );
        }
    };

    let token = if args.no_token {
        None
    } else if let Some(cli_token) = args.web_auth_token.clone() {
        Some(cli_token)
    } else if let Some(env_tok) = env_token {
        Some(env_tok)
    } else {
        load_persisted_token(state_dir)?
    };

    let is_plaintext_http = bare_url.starts_with("http://");
    let (qr_payload, has_token) = match token {
        Some(tok) => (WebAuth::new(tok).share_url(&bare_url), true),
        None => (bare_url.clone(), false),
    };

    Ok(ResolvedQrTarget {
        bare_url,
        qr_payload,
        has_token,
        is_plaintext_http,
        source,
    })
}

/// Entry point wired by `server_main.rs` for `troupe-server qr [OPTIONS]`.
pub fn run(args: &[String]) -> ExitCode {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let is_tty = stdout.is_terminal();
    let env_token = brand_env::value("WEB_AUTH_TOKEN").and_then(|v| WebAuthToken::new(&v));
    let mut out_lock = stdout.lock();
    let mut err_lock = stderr.lock();
    run_with_io(
        args,
        is_tty,
        env_token,
        detect_lan_ipv4(),
        &mut out_lock,
        &mut err_lock,
    )
}

/// Testable core of [`run`]: accepts injected TTY state, env token, LAN IP,
/// and output streams so every path is unit-tested deterministically.
pub(crate) fn run_with_io<W1: Write, W2: Write>(
    args: &[String],
    is_tty: bool,
    env_token: Option<WebAuthToken>,
    lan_ip: Option<IpAddr>,
    stdout: &mut W1,
    stderr: &mut W2,
) -> ExitCode {
    let parsed = match parse_qr_args(args) {
        Ok(p) => p,
        Err(QrCliParseError::Help) => {
            let _ = writeln!(stdout, "{}", qr_usage());
            return ExitCode::SUCCESS;
        }
        Err(QrCliParseError::Message(msg)) => {
            let _ = writeln!(stderr, "troupe-server qr: {msg}");
            let _ = writeln!(stderr);
            let _ = writeln!(stderr, "{}", qr_usage());
            return ExitCode::from(2);
        }
    };

    let state_dir = resolve_state_dir(parsed.state_dir.as_deref());
    let target = match resolve_qr_target(&parsed, &state_dir, env_token, lan_ip) {
        Ok(t) => t,
        Err(msg) => {
            warn!(
                target: "termul::server_qr",
                error = %msg,
                "failed to resolve QR target"
            );
            let _ = writeln!(stderr, "troupe-server qr: {msg}");
            return ExitCode::from(1);
        }
    };

    if target.has_token && !is_tty {
        warn!(
            target: "termul::server_qr",
            "refused tokenized QR on non-TTY stdout"
        );
        let _ = writeln!(
            stderr,
            "troupe-server qr: refusing to print a QR code containing the web auth token to \
             non-TTY stdout (secrets must not land in logs or files). Run in an interactive \
             terminal, or pass --no-token."
        );
        return ExitCode::from(1);
    }

    let color = is_tty && terminal_color_enabled(parsed.plain);
    let qr = match render_terminal_qr(&target.qr_payload, color) {
        Ok(q) => q,
        Err(msg) => {
            warn!(target: "termul::server_qr", error = %msg, "QR encoding failed");
            let _ = writeln!(stderr, "troupe-server qr: {msg}");
            return ExitCode::from(1);
        }
    };

    info!(
        target: "termul::server_qr",
        has_token = target.has_token,
        source = target.source.as_str(),
        "rendered server connect QR"
    );

    let _ = writeln!(stdout, "{qr}");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "Scan to open: {}", target.bare_url);
    if target.has_token {
        let _ = writeln!(
            stdout,
            "Note: QR includes your web auth token (#token=...). Share only with trusted devices."
        );
    }
    if target.is_plaintext_http {
        let _ = writeln!(
            stdout,
            "Transport: plaintext HTTP — use on a trusted LAN or place behind an HTTPS reverse \
             proxy (--url https://...)."
        );
    }
    ExitCode::SUCCESS
}

/// Concise CLI help for `troupe-server qr`.
#[must_use]
pub fn qr_usage() -> &'static str {
    r#"troupe-server qr — display a scannable QR code to connect a phone or tablet

USAGE:
    troupe-server qr [OPTIONS]

OPTIONS:
    --url <URL>                Explicit http(s) base URL (e.g. https://troupe.example.com)
    --host <HOST>              Explicit LAN IP or hostname (overrides auto-detected LAN IP)
    --port <PORT>              Server port override [default: server-config.json or 8080]
    --state-dir <PATH>         State dir containing server-config.json and web-auth-token
    --web-auth-token <TOKEN>   Explicit bearer token [env: TROUPE_WEB_AUTH_TOKEN]
    --no-token                 Omit #token=... from the QR code (allows non-TTY output)
    --plain                    Render plain Unicode blocks without ANSI color escapes
    -h, --help                 Show this help

SECURITY:
    When a web auth token is embedded, `troupe-server qr` requires an interactive
    terminal (TTY) so secrets never land in log files or pipes. Pass --no-token
    to emit a tokenless QR non-interactively."#
}

#[cfg(test)]
#[path = "server_qr_tests.rs"]
mod tests;
