use crate::client::{ClientError, MockClient};
use crate::service::tests::{self, ExpectationExt};
use crate::service::tests::{get_lock, MTX};
use crate::{NodeId, NodeService};
use std::net::SocketAddr;

#[tokio::test]
async fn when_predecessor_is_up_it_should_not_be_removed() {
    let _m = get_lock(&MTX);
    let ctx = MockClient::init_context();

    ctx.expect().returning(|addr: SocketAddr| {
        let mut client = MockClient::new();
        if addr.port() == 42012 {
            client.expect_ping().times(1).returning(|| Ok(()));
        }
        client
    });

    let service: NodeService<MockClient> =
        NodeService::with_id(8, SocketAddr::from(([127, 0, 0, 1], 42001)), 3);
    service.store.db().set_successor(tests::node(16));
    service.store.db().set_predecessor(tests::node(12));

    service.check_predecessor().await.unwrap();

    assert!(service.store.db().predecessor().is_some());
    assert_eq!(service.store.db().predecessor().unwrap().id, NodeId(12));
}

#[tokio::test]
async fn when_predecessor_is_down_it_should_be_removed() {
    let _m = get_lock(&MTX);
    let ctx = MockClient::init_context();

    ctx.expect().returning(|addr: SocketAddr| {
        let client = MockClient::mock(addr, 10, |mut client| {
            client
                .expect_ping()
                .times(1)
                .returning_error(ClientError::ConnectionFailed("Error".to_string()));

            client
        });

        client
    });

    let service: NodeService<MockClient> =
        NodeService::with_id(8, SocketAddr::from(([127, 0, 0, 1], 42001)), 3);
    service.store.db().set_successor(tests::node(10));
    service.store.db().set_predecessor(tests::node(10));

    service.check_predecessor().await.unwrap();

    assert!(service.store.db().predecessor().is_none());
}
