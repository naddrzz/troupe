use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::process::ExitCode;

use super::*;
use crate::web::auth::WEB_AUTH_TOKEN_FILE;
use crate::web::server_config_file::{self, ServerConfigFile};

fn temp_state_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "troupe-server-qr-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp state dir");
    dir
}

fn write_public_config(state_dir: &Path, port: u16) {
    let cfg = ServerConfigFile {
        host: "0.0.0.0".into(),
        port,
        sessions_dir: state_dir.join("sessions"),
        projects_file: state_dir.join("projects.json"),
        allow_remote_writes: true,
        update_channel: None,
        update_interval_secs: 21600,
    };
    server_config_file::save(state_dir, &cfg).expect("save server-config.json");
}

fn write_token_file(state_dir: &Path, token: &str) {
    let path = state_dir.join(WEB_AUTH_TOKEN_FILE);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .expect("create token file");
        f.write_all(token.as_bytes()).expect("write token");
        f.write_all(b"\n").expect("write newline");
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&path, format!("{token}\n")).expect("write token");
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn parse_qr_args_accepts_all_supported_flags() {
    let args = strings(&[
        "--url",
        "https://troupe.example.com",
        "--host",
        "192.168.1.50",
        "--port",
        "9090",
        "--state-dir",
        "/var/lib/troupe",
        "--web-auth-token",
        "secret123",
        "--no-token",
        "--plain",
    ]);
    let parsed = parse_qr_args(&args).expect("parse ok");
    assert_eq!(parsed.url.as_deref(), Some("https://troupe.example.com"));
    assert_eq!(parsed.host.as_deref(), Some("192.168.1.50"));
    assert_eq!(parsed.port, Some(9090));
    assert_eq!(parsed.state_dir.as_deref(), Some(Path::new("/var/lib/troupe")));
    assert_eq!(
        parsed.web_auth_token.as_ref().map(WebAuthToken::as_str),
        Some("secret123")
    );
    assert!(parsed.no_token);
    assert!(parsed.plain);
}

#[test]
fn parse_qr_args_rejects_port_zero_and_unknown_flag() {
    assert!(matches!(
        parse_qr_args(&strings(&["--port", "0"])),
        Err(QrCliParseError::Message(_))
    ));
    assert!(matches!(
        parse_qr_args(&strings(&["--bogus"])),
        Err(QrCliParseError::Message(_))
    ));
    assert_eq!(
        parse_qr_args(&strings(&["--help"])),
        Err(QrCliParseError::Help)
    );
}

#[test]
fn render_terminal_qr_produces_half_block_grid_with_and_without_ansi() {
    let colored = render_terminal_qr("https://troupe.example.com/#token=abc123", true)
        .expect("render colored");
    assert!(colored.contains("\x1b[30;47m"));
    assert!(colored.contains("\x1b[0m"));
    assert!(colored.lines().count() >= 10);

    let plain = render_terminal_qr("https://troupe.example.com/#token=abc123", false)
        .expect("render plain");
    assert!(!plain.contains("\x1b["));
    assert!(plain.contains('█'));
    assert_eq!(colored.lines().count(), plain.lines().count());
}

#[test]
fn resolve_qr_target_uses_lan_ip_and_persisted_token_on_public_config() {
    let dir = temp_state_dir("public");
    write_public_config(&dir, 9123);
    write_token_file(&dir, "deadbeef1234");

    let target = resolve_qr_target(
        &QrCliArgs::default(),
        &dir,
        None,
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42))),
    )
    .expect("resolve ok");

    assert_eq!(target.bare_url, "http://192.168.1.42:9123/");
    assert_eq!(
        target.qr_payload,
        "http://192.168.1.42:9123/#token=deadbeef1234"
    );
    assert!(target.has_token);
    assert!(target.is_plaintext_http);
    assert_eq!(target.source, TargetSource::LanIp);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_qr_target_rejects_loopback_only_config_without_override() {
    let dir = temp_state_dir("loopback");
    let err = resolve_qr_target(
        &QrCliArgs::default(),
        &dir,
        None,
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42))),
    )
    .err()
    .expect("must reject loopback-only config");
    assert!(err.contains("localhost only (127.0.0.1)"), "got: {err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_qr_target_rejects_wildcard_host_and_non_http_url() {
    let dir = temp_state_dir("invalid");
    let wildcard_err = resolve_qr_target(
        &QrCliArgs {
            host: Some("0.0.0.0".into()),
            ..QrCliArgs::default()
        },
        &dir,
        None,
        None,
    )
    .err()
    .expect("must reject 0.0.0.0");
    assert!(wildcard_err.contains("wildcard"), "got: {wildcard_err}");

    let scheme_err = resolve_qr_target(
        &QrCliArgs {
            url: Some("ftp://example.com".into()),
            ..QrCliArgs::default()
        },
        &dir,
        None,
        None,
    )
    .err()
    .expect("must reject ftp");
    assert!(scheme_err.contains("http or https"), "got: {scheme_err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_qr_target_strips_existing_fragment_and_brackets_ipv6() {
    let dir = temp_state_dir("url-ipv6");
    write_token_file(&dir, "tok_xyz");

    let url_target = resolve_qr_target(
        &QrCliArgs {
            url: Some("https://troupe.example.com/app#old".into()),
            ..QrCliArgs::default()
        },
        &dir,
        None,
        None,
    )
    .expect("url resolve");
    assert_eq!(url_target.bare_url, "https://troupe.example.com/app");
    assert_eq!(
        url_target.qr_payload,
        "https://troupe.example.com/app#token=tok_xyz"
    );
    assert!(!url_target.is_plaintext_http);

    let v6_target = resolve_qr_target(
        &QrCliArgs {
            host: Some("fd00::1".into()),
            port: Some(8080),
            no_token: true,
            ..QrCliArgs::default()
        },
        &dir,
        None,
        None,
    )
    .expect("ipv6 resolve");
    assert_eq!(v6_target.bare_url, "http://[fd00::1]:8080/");
    assert!(!v6_target.has_token);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_with_io_refuses_tokenized_qr_on_non_tty_stdout() {
    let dir = temp_state_dir("non-tty-refuse");
    write_public_config(&dir, 8080);
    write_token_file(&dir, "supersecret_token_value");

    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        &strings(&["--state-dir", &dir.display().to_string()]),
        false, // non-TTY!
        None,
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))),
        &mut out,
        &mut err,
    );

    assert_eq!(code, ExitCode::from(1));
    assert!(out.is_empty(), "must write nothing to stdout when refused");
    let err_str = String::from_utf8(err).unwrap();
    assert!(err_str.contains("non-TTY"), "got: {err_str}");
    assert!(
        !err_str.contains("supersecret_token_value"),
        "error must never leak the token"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_with_io_allows_no_token_on_non_tty_and_never_echoes_token_text_on_tty() {
    let dir = temp_state_dir("tty-ok");
    write_public_config(&dir, 8080);
    write_token_file(&dir, "supersecret_token_value");

    // Non-TTY with --no-token succeeds (plain output, no token).
    let mut out_no_tok = Vec::new();
    let mut err_no_tok = Vec::new();
    let code = run_with_io(
        &strings(&["--state-dir", &dir.display().to_string(), "--no-token"]),
        false,
        None,
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))),
        &mut out_no_tok,
        &mut err_no_tok,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let out_str = String::from_utf8(out_no_tok).unwrap();
    assert!(out_str.contains("http://192.168.1.20:8080/"));
    assert!(!out_str.contains("\x1b["), "non-TTY must not emit ANSI");
    assert!(!out_str.contains("supersecret_token_value"));

    // Interactive TTY with token succeeds, prints bare URL + notice, NEVER raw token text.
    let mut out_tty = Vec::new();
    let mut err_tty = Vec::new();
    let code = run_with_io(
        &strings(&["--state-dir", &dir.display().to_string(), "--plain"]),
        true,
        None,
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))),
        &mut out_tty,
        &mut err_tty,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(err_tty.is_empty());
    let tty_str = String::from_utf8(out_tty).unwrap();
    assert!(tty_str.contains("Scan to open: http://192.168.1.20:8080/"));
    assert!(tty_str.contains("includes your web auth token"));
    assert!(
        !tty_str.contains("supersecret_token_value"),
        "visible text must never echo the plaintext token"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn resolve_qr_target_fails_closed_on_world_readable_token_file() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = temp_state_dir("unsafe-perms");
    write_public_config(&dir, 8080);
    let token_path = dir.join(WEB_AUTH_TOKEN_FILE);
    std::fs::write(&token_path, "tok123\n").expect("write token");
    std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o644))
        .expect("chmod 0644");

    let err = resolve_qr_target(
        &QrCliArgs::default(),
        &dir,
        None,
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))),
    )
    .err()
    .expect("must fail closed on mode 0644 token file");
    assert!(err.contains("owner-protected"), "got: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}
