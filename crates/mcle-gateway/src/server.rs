use bytes::Bytes;
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::GracefulShutdown;
use mcle_core::coverage::{CoverageRecorder, Strict};
use mcle_core::faults::FaultSet;
use mcle_core::sandbox::SandboxRegistry;
use mcle_iam::IamEngine;
use mcle_sdk::{AuthzEvaluator, CloudAdapter, Host, IamMode, PipelineLayer, ServiceFacade};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub iam: IamMode,
    pub strict: Option<Strict>,
    pub seed: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        GatewayConfig {
            iam: IamMode::Audit,
            strict: None,
            seed: 0,
        }
    }
}

#[derive(Default, Clone)]
pub struct Plugins {
    pub adapters: Vec<Arc<dyn CloudAdapter>>,
    pub services: Vec<Arc<dyn ServiceFacade>>,
    pub evaluators: Vec<Arc<dyn AuthzEvaluator>>,
    pub layers: Vec<Arc<dyn PipelineLayer>>,
}

pub struct Gateway {
    pub(crate) host: Host,
    pub(crate) plugins: Plugins,
    pub(crate) iam: IamEngine,
    pub(crate) faults: FaultSet,
    pub(crate) coverage: CoverageRecorder,
    pub(crate) sandboxes: SandboxRegistry,
    pub(crate) config: GatewayConfig,
    pub(crate) ready: AtomicBool,
    pub(crate) ports: std::sync::RwLock<BTreeMap<String, u16>>,
}

pub struct RunningGateway {
    pub addr: SocketAddr,
    pub gateway: Arc<Gateway>,
    shutdown: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl RunningGateway {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.task).await;
    }
}

impl Gateway {
    pub fn new(host: Host, plugins: Plugins, config: GatewayConfig) -> Arc<Self> {
        let iam = IamEngine::new(config.iam);
        for e in &plugins.evaluators {
            iam.register(e.clone());
        }
        Arc::new(Gateway {
            host,
            plugins,
            iam,
            faults: FaultSet::default(),
            coverage: CoverageRecorder::default(),
            sandboxes: SandboxRegistry::default(),
            config,
            ready: AtomicBool::new(false),
            ports: Default::default(),
        })
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn coverage(&self) -> &CoverageRecorder {
        &self.coverage
    }

    pub fn faults(&self) -> &FaultSet {
        &self.faults
    }

    pub fn iam(&self) -> &IamEngine {
        &self.iam
    }

    pub fn adapters(&self) -> &[Arc<dyn CloudAdapter>] {
        &self.plugins.adapters
    }

    pub fn services(&self) -> &[Arc<dyn ServiceFacade>] {
        &self.plugins.services
    }

    pub async fn serve(self: &Arc<Self>, addr: SocketAddr) -> std::io::Result<RunningGateway> {
        let listener = TcpListener::bind(addr).await?;
        let addr = listener.local_addr()?;
        if let Ok(mut p) = self.ports.write() {
            p.insert("http".into(), addr.port());
        }
        {
            let mut cfg = match self.host.config.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if cfg.public_base_url.ends_with(":0") || cfg.public_base_url.is_empty() {
                let host = cfg
                    .public_base_url
                    .trim_end_matches(":0")
                    .trim_end_matches('/')
                    .to_string();
                let host = if host.is_empty() {
                    "http://localhost".into()
                } else {
                    host
                };
                cfg.public_base_url = format!("{host}:{}", addr.port());
            }
        }
        let (tx, mut rx) = watch::channel(false);
        let gw = self.clone();
        let task = tokio::spawn(async move {
            let graceful = GracefulShutdown::new();
            let reaper = gw.clone();
            let reap = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    for sb in reaper.sandboxes.expire(reaper.host.clock.now()) {
                        reaper.host.state.reset(Some(&sb));
                        reaper.coverage.reset(Some(&sb.0));
                    }
                }
            });
            loop {
                tokio::select! {
                    acc = listener.accept() => {
                        let Ok((stream, _)) = acc else { continue };
                        let gw = gw.clone();
                        let svc = service_fn(move |req| {
                            let gw = gw.clone();
                            async move { Ok::<_, std::convert::Infallible>(gw.handle(req).await) }
                        });
                        let conn = Builder::new(TokioExecutor::new())
                            .serve_connection_with_upgrades(TokioIo::new(stream), svc)
                            .into_owned();
                        let fut = graceful.watch(conn);
                        tokio::spawn(async move {
                            if let Err(e) = fut.await {
                                tracing::debug!(error = %e, "connection closed");
                            }
                        });
                    }
                    _ = rx.changed() => break,
                }
            }
            reap.abort();
            let _ = tokio::time::timeout(Duration::from_millis(1500), graceful.shutdown()).await;
        });
        self.ready.store(true, Ordering::SeqCst);
        Ok(RunningGateway {
            addr,
            gateway: self.clone(),
            shutdown: tx,
            task,
        })
    }

    async fn handle(self: Arc<Self>, req: Request<Incoming>) -> Response<Full<Bytes>> {
        let (parts, body) = req.into_parts();
        let raw = match body.collect().await {
            Ok(b) => b.to_bytes(),
            Err(e) => {
                return crate::internal::plain(400, &format!("failed to read body: {e}"));
            }
        };
        let head = crate::pipeline::head_from_parts(&parts);
        if head.path.starts_with("/_mcle/") || head.path == "/_mcle" {
            return crate::internal::handle(&self, head, raw).await;
        }
        crate::pipeline::handle(&self, head, raw).await
    }
}
