use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use crate::server::chord_proto::chord_node_client::ChordNodeClient;
use crate::server::chord_proto::{
    self, FindSuccessorRequest, GetPredecessorRequest, NotifyRequest,
};
use chord_rs_core::client::ClientError;
use chord_rs_core::{Client, Node, NodeId};
use error_stack::{IntoReport, Report, Result, ResultExt};
use tonic::async_trait;
use tonic::transport::{Channel, Endpoint};

#[derive(Debug)]
pub struct ChordGrpcClient {
    // pub(crate) endpoint: Endpoint,
    pub(crate) client: ClientGuard,
}

#[derive(Debug, Clone)]
pub(crate) struct ClientGuard {
    client: Arc<Mutex<Option<ChordNodeClient<Channel>>>>,
}

impl ClientGuard {
    fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl Client for ChordGrpcClient {
    async fn init(addr: SocketAddr) -> Self {
        log::debug!("Initializing client for {}", addr);
        let endpoint = Endpoint::from_shared(format!("http://{}", addr)).unwrap();
        let client_guard = ClientGuard::new();
        let client_guard_clone = client_guard.clone();

        let client = ChordNodeClient::connect(endpoint.clone()).await;
        if let Err(err) = &client {
            log::error!("Failed to initialize client: {:?}", err);
        } else {
            log::debug!("Client initialized");
            client_guard_clone
                .client
                .lock()
                .unwrap()
                .replace(client.unwrap());
        }

        ChordGrpcClient {
            client: client_guard,
        }
    }

    async fn find_successor(&self, id: NodeId) -> Result<Node, ClientError> {
        let mut client = self.client()?;

        let request = tonic::Request::new(FindSuccessorRequest { id: id.into() });
        let response = client
            .find_successor(request)
            .await
            .into_report()
            .change_context(ClientError::Unexpected)?;
        // if let Err(err) = response {
        //     log::warn!("Failed to find successor: {:?}", err);
        //     return Err(ClientError::Unexpected(err.to_string()));
        // }
        let response = response.into_inner();

        let node = response.node.unwrap();
        let node: Node = node.try_into().unwrap();

        Ok(node)
    }

    async fn successor(&self) -> Result<Node, ClientError> {
        let mut client = self.client()?;

        let request = tonic::Request::new(chord_proto::GetSuccessorRequest {});

        let response = client.get_successor(request).await.unwrap().into_inner();

        if let Some(node) = response.node {
            let node: Node = node.try_into().unwrap();
            Ok(node)
        } else {
            Err(Report::new(ClientError::Unexpected).attach_printable("No successor found"))
        }
    }

    async fn successor_list(&self) -> Result<Vec<Node>, ClientError> {
        todo!("successor_list")
    }

    async fn predecessor(&self) -> Result<Option<Node>, ClientError> {
        let mut client = self.client()?;

        let request = tonic::Request::new(GetPredecessorRequest {});

        let response = client.get_predecessor(request).await.unwrap().into_inner();

        if let Some(node) = response.node {
            let node: Node = node.try_into().unwrap();
            return Ok(Some(node));
        }

        Ok(None)
    }

    async fn notify(&self, predecessor: Node) -> Result<(), ClientError> {
        let mut client = self.client()?;

        let request = tonic::Request::new(NotifyRequest {
            node: Some(predecessor.into()),
        });
        client.notify(request).await.unwrap();

        Ok(())
    }

    async fn ping(&self) -> Result<(), ClientError> {
        let mut client = self.client()?;

        let request = tonic::Request::new(chord_proto::PingRequest {});
        client.ping(request).await.unwrap();

        Ok(())
    }
}

impl ChordGrpcClient {
    pub async fn new(addr: SocketAddr) -> Self {
        Self::init(addr).await
    }

    pub fn client(&self) -> Result<ChordNodeClient<Channel>, ClientError> {
        if let Some(client) = self.client.client.lock().unwrap().clone() {
            Ok(client)
        } else {
            Err(Report::new(ClientError::NotInitialized))
        }
    }
}

#[derive(Debug)]
pub struct IpParseError {
    msg: String,
}

impl IpParseError {
    fn new(msg: &str) -> Self {
        IpParseError {
            msg: msg.to_string(),
        }
    }
}

impl std::fmt::Display for IpParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl TryFrom<chord_proto::IpAddress> for IpAddr {
    type Error = IpParseError;

    fn try_from(ip: chord_proto::IpAddress) -> std::result::Result<Self, Self::Error> {
        fn ipv4(addr: Vec<u8>) -> [u8; 4] {
            let mut array = [0; 4];
            array.copy_from_slice(&addr);
            return array;
        }

        fn ipv6(addr: Vec<u8>) -> [u8; 16] {
            let mut array = [0; 16];
            array.copy_from_slice(&addr);
            return array;
        }

        if ip.is_v4() && ip.address.len() != 4 {
            return Err(IpParseError::new("Invalid IPv4 address"));
        } else if ip.is_v6() && ip.address.len() != 16 {
            return Err(IpParseError::new("Invalid IPv6 address"));
        } else if ip.is_v4() {
            return Ok(IpAddr::V4(Ipv4Addr::from(ipv4(ip.address))));
        } else if ip.is_v6() {
            return Ok(IpAddr::V6(Ipv6Addr::from(ipv6(ip.address))));
        } else {
            return Err(IpParseError::new("Invalid IP address"));
        }
    }
}

impl chord_proto::IpAddress {
    fn is_v4(&self) -> bool {
        self.version == chord_proto::IpVersion::Ipv4 as i32
    }

    fn is_v6(&self) -> bool {
        self.version == chord_proto::IpVersion::Ipv6 as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ipv4() {
        fn addr(addr: Vec<u8>) -> chord_proto::IpAddress {
            chord_proto::IpAddress {
                version: chord_proto::IpVersion::Ipv4 as i32,
                address: addr,
            }
        }

        let valid_ip = addr(vec![127, 0, 0, 1]);
        let invalid_ip = IpAddr::try_from(addr(vec![127, 0, 0, 1, 2]));

        assert_eq!(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::try_from(valid_ip).unwrap()
        );
        assert!(invalid_ip.is_err());
        assert_eq!("Invalid IPv4 address", invalid_ip.err().unwrap().msg);

        let invalid_ip = IpAddr::try_from(addr(vec![127, 0]));
        assert!(invalid_ip.is_err());
        assert_eq!("Invalid IPv4 address", invalid_ip.err().unwrap().msg);
    }

    #[test]
    fn parse_ipv6() {
        fn addr(addr: Vec<u8>) -> chord_proto::IpAddress {
            chord_proto::IpAddress {
                version: chord_proto::IpVersion::Ipv6 as i32,
                address: addr,
            }
        }

        let ipv6: Ipv6Addr = "fd9f:9b7:9d0e::".parse().unwrap();

        let mut valid_bytes = vec![253, 159, 9, 183, 157, 14];
        valid_bytes.resize(16, 0);
        let valid_ip = addr(valid_bytes);
        let invalid_ip = IpAddr::try_from(addr(vec![127, 0, 0, 1, 2]));

        assert_eq!(ipv6, IpAddr::try_from(valid_ip).unwrap());
        assert!(invalid_ip.is_err());
        assert_eq!("Invalid IPv6 address", invalid_ip.err().unwrap().msg);

        let invalid_ip = IpAddr::try_from(addr(vec![127, 0]));
        assert!(invalid_ip.is_err());
        assert_eq!("Invalid IPv6 address", invalid_ip.err().unwrap().msg);
    }
}
