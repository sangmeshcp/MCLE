#![forbid(unsafe_code)]

mod client;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use mcle_core::coverage::{render, violations, Strict};
use mcle_core::CoreOptions;
use mcle_gateway::{Gateway, GatewayConfig, RunningGateway};
use mcle_sdk::host::HostConfig;
use mcle_sdk::IamMode;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const EXIT_CONFIG: u8 = 2;
const EXIT_BIND: u8 = 3;
const EXIT_TIMEOUT: u8 = 4;
const EXIT_STRICT: u8 = 10;

#[derive(Parser)]
#[command(
    name = "mcle",
    version,
    about = "Multi-cloud local emulator (AWS, Azure, GCP, OCI, IBM Cloud)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the emulator in the foreground.
    Start(ServerArgs),
    /// Start the emulator, run a command against it, then stop.
    Run {
        #[command(flatten)]
        server: ServerArgs,
        /// Fail with exit code 10 on fidelity gaps: L1 (only L0 calls) or L2 (also L1 data-plane calls).
        #[arg(long, num_args = 0..=1, default_missing_value = "L2", require_equals = true)]
        strict: Option<String>,
        /// Write report.{md,json,xml} and state.json here.
        #[arg(long, env = "MCLE_ARTIFACTS_DIR")]
        artifacts_dir: Option<PathBuf>,
        /// When to write artifacts: always | on-failure.
        #[arg(long, default_value = "always")]
        artifacts: String,
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },
    /// Print environment exports for SDKs/CLIs.
    Env {
        #[arg(long, default_value = "all")]
        cloud: String,
        #[arg(long, env = "MCLE_ENDPOINT", default_value = "http://localhost:4700")]
        url: String,
    },
    /// Wait until a running instance is ready.
    Wait {
        #[arg(long, env = "MCLE_ENDPOINT", default_value = "http://localhost:4700")]
        url: String,
        #[arg(long, default_value = "30")]
        timeout: u64,
    },
    /// Exit 0 if a running instance is healthy (for container HEALTHCHECK).
    Healthcheck {
        #[arg(long, env = "MCLE_ENDPOINT", default_value = "http://localhost:4700")]
        url: String,
    },
    /// Print the coverage report of a running instance.
    Report {
        #[arg(long, env = "MCLE_ENDPOINT", default_value = "http://localhost:4700")]
        url: String,
        /// md | json | junit
        #[arg(long, default_value = "md")]
        format: String,
        #[arg(long)]
        strict: Option<String>,
    },
    /// Export or import emulator state.
    State {
        #[command(subcommand)]
        cmd: StateCmd,
    },
}

#[derive(Subcommand)]
enum StateCmd {
    Export {
        #[arg(long, env = "MCLE_ENDPOINT", default_value = "http://localhost:4700")]
        url: String,
        #[arg(long)]
        file: PathBuf,
    },
    Import {
        #[arg(long, env = "MCLE_ENDPOINT", default_value = "http://localhost:4700")]
        url: String,
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Args, Clone)]
struct ServerArgs {
    #[arg(long, env = "MCLE_BIND", default_value = "127.0.0.1")]
    bind: IpAddr,
    /// 0 picks a free port; see --port-file.
    #[arg(long, env = "MCLE_PORT")]
    port: Option<u16>,
    /// Write `{"http": <port>, "url": ...}` here once listening.
    #[arg(long, env = "MCLE_PORT_FILE")]
    port_file: Option<PathBuf>,
    /// Host used in generated URLs (e.g. a CI service alias).
    #[arg(long, env = "MCLE_PUBLIC_HOST")]
    public_host: Option<String>,
    /// Comma-separated clouds to enable (default: all compiled in).
    #[arg(long, env = "MCLE_CLOUDS", value_delimiter = ',')]
    clouds: Vec<String>,
    /// off | audit | enforce
    #[arg(long, env = "MCLE_IAM", default_value = "audit")]
    iam: String,
    #[arg(long, env = "MCLE_SEED", default_value = "0")]
    seed: u64,
    /// real | fixed:<rfc3339>
    #[arg(long, env = "MCLE_CLOCK", default_value = "real")]
    clock: String,
    #[arg(long, env = "MCLE_LRO_DELAY_MS", default_value = "0")]
    lro_delay_ms: u64,
    /// Snapshot file to load at startup.
    #[arg(long, env = "MCLE_STATE_IMPORT")]
    state_import: Option<PathBuf>,
    /// JSON/array of fault rules to load at startup.
    #[arg(long, env = "MCLE_FAULTS")]
    faults: Option<PathBuf>,
    /// text | json
    #[arg(long, env = "MCLE_LOG_FORMAT", default_value = "text")]
    log_format: String,
}

enum Failure {
    Config(String),
    Bind(String),
}

fn init_logging(format: &str) {
    let filter =
        tracing_subscriber::EnvFilter::try_from_env("MCLE_LOG").unwrap_or_else(|_| "warn".into());
    let b = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    let _ = if format == "json" {
        b.json().try_init()
    } else {
        b.try_init()
    };
}

async fn start_server(a: &ServerArgs, default_port: u16) -> Result<RunningGateway, Failure> {
    let iam =
        IamMode::parse(&a.iam).ok_or_else(|| Failure::Config(format!("bad --iam `{}`", a.iam)))?;
    let port = a.port.unwrap_or(default_port);
    let public = match &a.public_host {
        Some(h) if h.contains("://") => format!("{h}:{port}"),
        Some(h) => format!("http://{h}:{port}"),
        None => format!("http://localhost:{port}"),
    };
    let opts = CoreOptions {
        seed: a.seed,
        clock: a.clock.clone(),
        lro_delay_ms: a.lro_delay_ms,
        config: HostConfig {
            public_base_url: public,
            ..HostConfig::default()
        },
    };
    let host = mcle_core::build_host(&opts).map_err(Failure::Config)?;
    let plugins = mcle_bundle::plugins(&host, &a.clouds);
    if plugins.adapters.is_empty() {
        return Err(Failure::Config(format!(
            "no compiled-in cloud matches --clouds {:?}",
            a.clouds
        )));
    }
    let gw = Gateway::new(
        host,
        plugins,
        GatewayConfig {
            iam,
            strict: None,
            seed: a.seed,
        },
    );
    if let Some(p) = &a.state_import {
        let doc: Value = read_json(p).map_err(|e| Failure::Config(e.to_string()))?;
        let snap = mcle_persist::parse(doc).map_err(|e| Failure::Config(e.to_string()))?;
        mcle_persist::import(&*gw.host().state, snap);
    }
    if let Some(p) = &a.faults {
        let doc: Value = read_json(p).map_err(|e| Failure::Config(e.to_string()))?;
        let doc = doc.get("faults").cloned().unwrap_or(doc);
        let rules = serde_json::from_value(doc)
            .map_err(|e| Failure::Config(format!("bad faults file: {e}")))?;
        gw.faults().set(rules);
    }
    let running = gw
        .serve(SocketAddr::new(a.bind, port))
        .await
        .map_err(|e| Failure::Bind(format!("cannot bind {}:{port}: {e}", a.bind)))?;
    if let Some(pf) = &a.port_file {
        let doc = json!({"http": running.addr.port(), "url": gw.host().config().public_base_url});
        std::fs::write(pf, doc.to_string())
            .map_err(|e| Failure::Config(format!("port file: {e}")))?;
    }
    Ok(running)
}

fn read_json(p: &PathBuf) -> Result<Value> {
    let text = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", p.display()))
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(_) => return std::future::pending().await,
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn failure_code(f: Failure) -> ExitCode {
    match f {
        Failure::Config(m) => {
            eprintln!("mcle: {m}");
            ExitCode::from(EXIT_CONFIG)
        }
        Failure::Bind(m) => {
            eprintln!("mcle: {m}");
            ExitCode::from(EXIT_BIND)
        }
    }
}

fn env_pairs(gw: &Gateway) -> Vec<(String, String)> {
    let base = gw.host().config().public_base_url;
    let mut out = vec![("MCLE_ENDPOINT".to_string(), base.clone())];
    for a in gw.adapters() {
        out.extend(a.env(&base));
    }
    out
}

fn write_artifacts(dir: &PathBuf, gw: &Gateway, strict: Option<Strict>) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let entries = gw.coverage().entries(None);
    std::fs::write(dir.join("report.md"), render(&entries, "md", strict))?;
    std::fs::write(dir.join("report.json"), render(&entries, "json", strict))?;
    std::fs::write(dir.join("report.xml"), render(&entries, "junit", strict))?;
    let snap = mcle_persist::export(&*gw.host().state);
    std::fs::write(dir.join("state.json"), serde_json::to_vec_pretty(&snap)?)?;
    Ok(())
}

async fn run_child(
    server: ServerArgs,
    strict: Option<String>,
    artifacts_dir: Option<PathBuf>,
    artifacts: String,
    command: Vec<String>,
) -> ExitCode {
    let strict = match strict.as_deref().map(Strict::parse) {
        None => None,
        Some(Some(s)) => Some(s),
        Some(None) => {
            return failure_code(Failure::Config("bad --strict (expected L1 or L2)".into()))
        }
    };
    let running = match start_server(&server, 0).await {
        Ok(r) => r,
        Err(f) => return failure_code(f),
    };
    let gw = running.gateway.clone();
    let Some((prog, args)) = command.split_first() else {
        return failure_code(Failure::Config("missing command after --".into()));
    };
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(args).envs(env_pairs(&gw));
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            running.shutdown().await;
            return failure_code(Failure::Config(format!("cannot run `{prog}`: {e}")));
        }
    };
    let status = tokio::select! {
        s = child.wait() => s.ok(),
        _ = shutdown_signal() => {
            let _ = child.start_kill();
            child.wait().await.ok()
        }
    };
    let child_code = status.and_then(|s| s.code()).unwrap_or(1);
    let viol = strict
        .map(|s| violations(&gw.coverage().entries(None), s))
        .unwrap_or_default();
    let failed = child_code != 0 || !viol.is_empty();
    if let Some(dir) = &artifacts_dir {
        if artifacts == "always" || failed {
            if let Err(e) = write_artifacts(dir, &gw, strict) {
                eprintln!("mcle: writing artifacts: {e}");
            }
        }
    }
    running.shutdown().await;
    if !viol.is_empty() {
        eprintln!("mcle: strict mode violations:");
        for v in &viol {
            eprintln!("  - {v}");
        }
        if child_code == 0 {
            return ExitCode::from(EXIT_STRICT);
        }
    }
    ExitCode::from(u8::try_from(child_code).unwrap_or(1))
}

async fn wait_ready(url: &str, timeout: u64) -> ExitCode {
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        if let Ok((200, _)) = client::get(&format!("{url}/_mcle/ready")).await {
            return ExitCode::SUCCESS;
        }
        if Instant::now() >= deadline {
            eprintln!("mcle: {url} not ready after {timeout}s");
            return ExitCode::from(EXIT_TIMEOUT);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn remote(result: Result<String>) -> ExitCode {
    match result {
        Ok(s) => {
            println!("{}", s.trim_end());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mcle: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn ok_body(r: Result<(u16, String)>) -> Result<String> {
    match r? {
        (200..=299, b) => Ok(b),
        (s, b) => Err(anyhow!("HTTP {s}: {b}")),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Start(a) => {
            init_logging(&a.log_format);
            let running = match start_server(&a, 4700).await {
                Ok(r) => r,
                Err(f) => return failure_code(f),
            };
            eprintln!(
                "mcle: listening on http://{} ({} clouds)",
                running.addr,
                running.gateway.adapters().len()
            );
            shutdown_signal().await;
            running.shutdown().await;
            ExitCode::SUCCESS
        }
        Cmd::Run {
            server,
            strict,
            artifacts_dir,
            artifacts,
            command,
        } => {
            init_logging(&server.log_format);
            run_child(server, strict, artifacts_dir, artifacts, command).await
        }
        Cmd::Env { cloud, url } => {
            let host = match mcle_core::build_host(&CoreOptions {
                config: HostConfig {
                    public_base_url: url.clone(),
                    ..HostConfig::default()
                },
                ..Default::default()
            }) {
                Ok(h) => h,
                Err(e) => return failure_code(Failure::Config(e)),
            };
            let clouds = if cloud == "all" { vec![] } else { vec![cloud] };
            let plugins = mcle_bundle::plugins(&host, &clouds);
            println!("export MCLE_ENDPOINT={url}");
            for a in plugins.adapters {
                for (k, v) in a.env(&url) {
                    println!("export {k}={v}");
                }
            }
            ExitCode::SUCCESS
        }
        Cmd::Wait { url, timeout } => wait_ready(&url, timeout).await,
        Cmd::Healthcheck { url } => match client::get(&format!("{url}/_mcle/health")).await {
            Ok((200, _)) => ExitCode::SUCCESS,
            _ => ExitCode::FAILURE,
        },
        Cmd::Report {
            url,
            format,
            strict,
        } => {
            let mut u = format!("{url}/_mcle/report?format={format}");
            if let Some(s) = strict {
                u.push_str(&format!("&strict={s}"));
            }
            remote(ok_body(client::get(&u).await).await).await
        }
        Cmd::State {
            cmd: StateCmd::Export { url, file },
        } => {
            let r = ok_body(client::get(&format!("{url}/_mcle/state/export")).await).await;
            match r.and_then(|b| std::fs::write(&file, b).map_err(Into::into)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("mcle: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::State {
            cmd: StateCmd::Import { url, file },
        } => {
            let body = match std::fs::read(&file) {
                Ok(b) => b,
                Err(e) => return failure_code(Failure::Config(format!("{}: {e}", file.display()))),
            };
            remote(ok_body(client::post(&format!("{url}/_mcle/state/import"), body).await).await)
                .await
        }
    }
}
