use std::net::SocketAddr;
use mockall::predicate;
use crate::client::{ClientError, MockClient};
use crate::NodeService;
use crate::service::tests;

#[tokio::test]
async fn join_test() {
    let ctx = MockClient::init_context();

    ctx.expect().returning(|addr: SocketAddr| {
        let mut client = MockClient::new();
        if addr.port() == 42115 {
            client.expect_find_successor()
                .with(predicate::eq(1))
                .times(1)
                .returning(|_| {
                    Box::pin(async { Ok(tests::node_ref(115))})
                });
        }

        client
    });
    let node = tests::node(1);
    let mut service: NodeService<MockClient> = NodeService::new(node);

    service.join(tests::node_ref(115)).await.unwrap();

    assert_eq!(service.node.successor.id, 115);
}

#[tokio::test]
async fn join_error_test() {
    let ctx = MockClient::init_context();

    ctx.expect().returning(|addr: SocketAddr| {
        let mut client = MockClient::new();
        if addr.port() == 42116 {
            client.expect_find_successor()
                .with(predicate::eq(2))
                .times(1)
                .returning(|_| {
                    Box::pin(async { Err(ClientError::Unexpected("Test".to_string())) })
                });
        }
        client
    });
    let node = tests::node(2);
    let mut service: NodeService<MockClient> = NodeService::new(node);

    let result = service.join(tests::node_ref(116)).await;

    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert_eq!(message, "Client error: Test");
}