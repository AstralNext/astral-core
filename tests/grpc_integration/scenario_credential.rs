//! 场景：CredentialService

use crate::harness::{assert_unimplemented, TestServer};
use astral_core::pb::{
    CreateTokenRequest, GenerateNetworkCredentialRequest, ListTokensRequest,
    RotateNodeDeviceKeyRequest, RotateTokenRequest, RevokeTokenRequest,
};
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s03_create_list_revoke() {
    let server = TestServer::start().await;
    let mut cred = server.credential().await;

    let created = cred
        .create_token(Request::new(CreateTokenRequest {
            name: "scenario-ci".into(),
            expires_at: None,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(created.token.starts_with("ask_"));
    assert!(!created.token_id.is_empty());

    let listed = cred
        .list_tokens(Request::new(ListTokensRequest {
            include_revoked: false,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(listed.tokens.len() >= 2);
    assert!(listed.tokens.iter().any(|t| t.token_id == created.token_id));

    // 吊销新创建的（bootstrap 仍在）
    cred.revoke_token(Request::new(RevokeTokenRequest {
        token_id: created.token_id.clone(),
    }))
    .await
    .expect("revoke");

    let after = cred
        .list_tokens(Request::new(ListTokensRequest {
            include_revoked: false,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(!after.tokens.iter().any(|t| t.token_id == created.token_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s03_forbid_revoke_last_token() {
    let server = TestServer::start().await;
    let mut cred = server.credential().await;
    let listed = cred
        .list_tokens(Request::new(ListTokensRequest {
            include_revoked: false,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.tokens.len(), 1);
    let only = &listed.tokens[0];
    let err = cred
        .revoke_token(Request::new(RevokeTokenRequest {
            token_id: only.token_id.clone(),
        }))
        .await
        .expect_err("禁止吊销最后一把");
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s03_unimplemented_extras() {
    let server = TestServer::start().await;
    let mut cred = server.credential().await;
    assert_unimplemented(
        &cred
            .rotate_token(Request::new(RotateTokenRequest {
                token_id: "x".into(),
                keep_id: false,
            }))
            .await
            .unwrap_err(),
    );
    assert_unimplemented(
        &cred
            .generate_network_credential(Request::new(GenerateNetworkCredentialRequest {
                instance: None,
                name: String::new(),
                expires_at: None,
            }))
            .await
            .unwrap_err(),
    );
    assert_unimplemented(
        &cred
            .rotate_node_device_key(Request::new(RotateNodeDeviceKeyRequest {
                node: None,
            }))
            .await
            .unwrap_err(),
    );
}
