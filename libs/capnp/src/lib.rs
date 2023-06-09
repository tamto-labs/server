use std::{net::SocketAddr, sync::Arc};

use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use chord_rs_core::NodeService;
use client::ChordCapnpClient;
use futures::AsyncReadExt;
use tokio::sync::Semaphore;

pub mod client;
pub mod parser;
mod server;

pub mod chord_capnp {

    include!(concat!(env!("OUT_DIR"), "/capnp/chord_capnp.rs"));
}

pub struct Server {
    addr: SocketAddr,
    node: Arc<NodeService<ChordCapnpClient>>,
}

impl Server {
    pub async fn new(addr: SocketAddr, ring: Option<SocketAddr>) -> Self {
        const REPLICATION_FACTOR: usize = 3; // TODO: make this configurable
        let node_service = Arc::new(NodeService::new(addr, REPLICATION_FACTOR));
        if let Some(ring) = ring {
            const MAX_RETRIES: u32 = 5;
            chord_rs_core::server::join_ring(node_service.clone(), ring, MAX_RETRIES).await;
        }
        chord_rs_core::server::background_tasks(node_service.clone());

        Self {
            addr,
            node: node_service,
        }
    }

    pub async fn run(&self, max_connections: usize) {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let server = server::NodeServerImpl::new(self.node.clone());
                let listener = tokio::net::TcpListener::bind(&self.addr).await.unwrap();
                let chord_node_client: chord_capnp::chord_node::Client =
                    capnp_rpc::new_client(server);
                let sem = Arc::new(Semaphore::new(max_connections));

                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let sem = sem.clone();
                    stream.set_nodelay(true).unwrap();
                    let (reader, writer) =
                        tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
                    let network = twoparty::VatNetwork::new(
                        reader,
                        writer,
                        rpc_twoparty_capnp::Side::Server,
                        Default::default(),
                    );

                    let rpc_system =
                        RpcSystem::new(Box::new(network), Some(chord_node_client.clone().client));

                    tokio::task::spawn_local(async move {
                        if let Ok(aq) = sem.try_acquire() {
                            log::trace!("Semaphore acquired");
                            if let Err(err) = rpc_system.await {
                                log::error!("rpc system error: {}", err);
                            }
                            log::trace!("Semaphore released");
                            drop(aq);
                        } else {
                            log::debug!("Failed to acquire semaphore")
                        }
                    });
                }
            })
            .await
    }
}
