use crate::client::{ClientError, ClientsPool};
use crate::node::store::{Db, NodeStore};
use crate::node::Finger;
use crate::{Client, Node, NodeId};
use std::net::SocketAddr;
use std::sync::Arc;
use std::vec;

#[cfg(test)]
pub(crate) mod tests;

#[derive(Debug)]
pub struct NodeService<C: Client> {
    id: NodeId,
    addr: SocketAddr,
    store: NodeStore,

    clients: ClientsPool<C>,
}

impl<C: Client + Clone> NodeService<C> {
    /// Create a new node service
    ///
    /// # Arguments
    ///
    /// * `socket_addr` - The address of the node
    /// * `replication_factor` - The number of successors to keep track of
    pub fn new(socket_addr: SocketAddr, replication_factor: usize) -> Self {
        let id: NodeId = socket_addr.into();
        Self::with_id(id, socket_addr, replication_factor)
    }

    fn with_id(id: impl Into<NodeId>, addr: SocketAddr, replication_factor: usize) -> Self {
        let id = id.into();
        let store = NodeStore::new(Node::with_id(id, addr), replication_factor);
        Self {
            id,
            addr,
            store,
            clients: ClientsPool::default(),
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub(crate) fn store(&self) -> Db {
        self.store.db()
    }

    /// Find the successor of the given id.
    ///
    /// If the given id is in the range of the current node and its successor, the successor is returned.
    /// Otherwise, the successor of the closest preceding node is returned.
    ///
    /// # Arguments
    ///
    /// * `id` - The id to find the successor for
    pub async fn find_successor(&self, id: NodeId) -> Result<Node, error::ServiceError> {
        let successor = self.store().successor();
        if Node::is_between_on_ring(id.0, self.id.0, successor.id.0) {
            Ok(successor)
        } else {
            let n = self.closest_preceding_node(id);
            let client: Arc<C> = self.client(n).await;
            let successor = client.find_successor(id).await?;
            Ok(successor)
        }
    }

    pub async fn get_predecessor(&self) -> Result<Option<Node>, error::ServiceError> {
        Ok(self.store().predecessor())
    }

    pub async fn get_successor(&self) -> Result<Node, error::ServiceError> {
        Ok(self.store().successor())
    }

    /// Join the chord ring.
    ///
    /// This method is used to join the chord ring. It will find the successor of its own id
    /// and set it as the successor.
    ///
    /// # Arguments
    ///
    /// * `node` - The node to join the ring with. It's an existing node in the ring.
    pub async fn join(&self, node: Node) -> Result<(), error::ServiceError> {
        let client: Arc<C> = self.client(node).await;
        let successor = client.find_successor(self.id).await?;
        self.store().set_successor(successor);

        Ok(())
    }

    /// Notify the node about a potential new predecessor.
    ///
    /// If the predecessor is not set or the given node is in the range of the current node and the
    /// predecessor, the predecessor is set to the given node.
    ///
    /// # Arguments
    ///
    /// * `node` - The node which might be the new predecessor
    pub fn notify(&self, node: Node) {
        let predecessor = self.store().predecessor();
        if predecessor.is_none()
            || Node::is_between_on_ring(node.id.0, predecessor.unwrap().id.0, self.id.0)
        {
            log::debug!("Setting predecessor to {:?}", node);
            {
                self.store().set_predecessor(node);
            }
        }
    }

    /// Stabilize the node
    ///
    /// This method is used to stabilize the node. It will check if a predecessor of the successor
    /// is in the range of the current node and its successor. If so, the successor will be set to
    /// the retrieved predecessor.
    ///
    /// It will also notify the successor about the current node.
    ///
    /// > **Note**
    /// >
    /// > This method should be called periodically.
    pub async fn stabilize(&self) -> Result<(), error::ServiceError> {
        let successor = self.store().successor();
        let client: Arc<C> = self.client(successor).await;
        let result = client.predecessor().await;
        drop(client);

        if let Ok(Some(x)) = result {
            if Node::is_between_on_ring(x.id.0, self.id.0, self.store().successor().id.0) {
                self.store().set_successor(x);
            }
        }

        let successor = self.store().successor();
        let client: Arc<C> = self.client(successor).await;

        client
            .notify(Node {
                id: self.id,
                addr: self.addr,
            })
            .await?;

        Ok(())
    }

    pub async fn reconcile_successors(&self) -> Result<(), error::ServiceError> {
        let successor = self.store().successor();
        let client: Arc<C> = self.client(successor.clone()).await;
        let successors_of_successor = client.successor_list().await?;

        let mut successors = vec![successor];
        successors.extend(successors_of_successor);

        // TODO: the list should have the specific size
        // if the successor returns less than we expect, we shouldn't remove any elements
        self.store().set_successor_list(successors);
        Ok(())
    }

    /// Check predecessor
    ///
    /// This method is used to check if the predecessor is still alive. If not, the predecessor is
    /// set to `None`.
    ///
    /// > **Note**
    /// >
    /// > This method should be called periodically.
    pub async fn check_predecessor(&self) -> Result<(), error::ServiceError> {
        if let Some(predecessor) = self.store().predecessor() {
            let client: Arc<C> = self.client(predecessor).await;
            match client.ping().await {
                Ok(_) => Ok(()),
                Err(ClientError::ConnectionFailed(_)) => {
                    self.store().unset_predecessor();
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        } else {
            Ok(())
        }
    }

    /// Fix fingers
    ///
    /// This method is used to fix the fingers. It iterates over all fingers and re-requests the
    /// successor of the finger's id. Then sets the successor of the finger to the retrieved node.
    ///
    /// > **Note**
    /// >
    /// > This method should be called periodically.
    pub async fn fix_fingers(&self) {
        for i in 0..Finger::FINGER_TABLE_SIZE {
            let finger_id = Finger::finger_id(self.id.0, (i + 1) as u8);
            let result = { self.find_successor(NodeId(finger_id)).await };
            if let Ok(successor) = result {
                self.store().update_finger(i.into(), successor)
            } else {
                log::error!("Failed to fix finger: {:?}", result.unwrap_err());
            }
        }
    }

    /// Get finger table
    ///
    /// This method is used to get the finger table of the node.
    pub fn finger_table(&self) -> Vec<Finger> {
        self.store().finger_table()
    }

    /// Get closest preceding node
    ///
    /// This method is used to get the closest preceding node of the given id.
    /// It will iterate over the finger table and return the closest node to the given id.
    ///
    /// # Arguments
    ///
    /// * `id` - The id to find the closest preceding node for
    ///
    /// # Returns
    ///
    /// The closest preceding node
    fn closest_preceding_node(&self, id: NodeId) -> Node {
        self.store()
            .closest_preceding_node(self.id.0, id.0)
            .unwrap_or(Node::new(self.addr))
    }

    async fn client(&self, node: Node) -> Arc<C> {
        self.clients.get_or_init(node).await
    }
}

pub mod error {
    use crate::client;
    use std::fmt::Display;

    #[derive(Debug)]
    pub enum ServiceError {
        Unexpected(String),
    }

    impl From<client::ClientError> for ServiceError {
        fn from(err: client::ClientError) -> Self {
            Self::Unexpected(format!("Client error: {}", err))
        }
    }

    impl Display for ServiceError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Unexpected(message) => write!(f, "{}", message),
            }
        }
    }
}
