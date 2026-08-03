//! 端到端：控制端 listen + 节点出站 AgentSession + 隧道 Ping。

use std::net::SocketAddr;
use std::time::Duration;

use astral_core::agent::{run_agent_loop, AgentConnectConfig};
use astral_core::app::AppState;
use astral_core::config::RuntimeConfigBuilder;
use astral_core::controller::{run_controller, ControllerListenParams, META_NODE_ID};
use astral_core::pb::node_service_client::NodeServiceClient;
use astral_core::pb::system_service_client::SystemServiceClient;
use astral_core::pb::{ListNodesRequest, PingRequest};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::Request;

async fn free_addr() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = l.local_addr().expect("addr");
    drop(l);
    addr
}

async fn wait_connect(url: &str) {
    for _ in 0..100 {
        if Channel::from_shared(url.to_string())
            .unwrap()
            .connect()
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("无法连接 {url}");
}

fn with_bearer<T>(mut req: Request<T>, token: &str) -> Request<T> {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("bearer"),
    );
    req
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_dials_controller_and_tunnel_ping() {
    let token = "test-shared-secret";
    let ctrl_dir = TempDir::new().expect("ctrl dir");
    let node_dir = TempDir::new().expect("node dir");

    let ctrl_addr = free_addr().await;
    let ctrl_url = format!("http://{ctrl_addr}");

    let (ctrl_stop_tx, ctrl_stop_rx) = oneshot::channel::<()>();
    let ctrl_params = ControllerListenParams {
        bind: ctrl_addr,
        token: token.into(),
        data_dir: ctrl_dir.path().to_path_buf(),
        tls: None,
    };
    let ctrl_join = tokio::spawn(async move {
        let _ = run_controller(ctrl_params, async move {
            let _ = ctrl_stop_rx.await;
        })
        .await;
    });
    wait_connect(&ctrl_url).await;

    // 节点：仅出站 Agent（隧道分发不依赖本地 listen）
    let node_runtime = RuntimeConfigBuilder::new()
        .grpc_listen("127.0.0.1:0".parse().unwrap())
        .data_dir(node_dir.path().to_path_buf())
        .build();
    let (node_state, _) = AppState::bootstrap(node_runtime).expect("node bootstrap");
    let node_id = node_state.node_id.clone();

    let agent_cfg = AgentConnectConfig {
        controller: ctrl_url.clone(),
        token: token.into(),
        retry_base: Duration::from_millis(200),
        retry_max: Duration::from_secs(2),
        heartbeat_interval: Duration::from_secs(30),
        tls: Default::default(),
    };
    let agent_state = node_state.clone();
    let agent_join = tokio::spawn(async move {
        run_agent_loop(agent_state, agent_cfg).await;
    });

    // 等到 ListNodes 看到该节点（管理 API 需 Bearer）
    let channel = Channel::from_shared(ctrl_url.clone())
        .unwrap()
        .connect()
        .await
        .expect("ctrl channel");
    let mut nodes = NodeServiceClient::new(channel.clone());
    let mut seen = false;
    for _ in 0..80 {
        let resp = nodes
            .list_nodes(with_bearer(
                Request::new(ListNodesRequest {
                    online_only: true,
                }),
                token,
            ))
            .await
            .expect("ListNodes")
            .into_inner();
        if resp.nodes.iter().any(|n| n.node_id == node_id && n.online) {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(seen, "控制端未看到出站节点 {node_id}");

    // 无 Bearer 应被拒绝
    let denied = nodes
        .list_nodes(Request::new(ListNodesRequest {
            online_only: true,
        }))
        .await;
    assert!(denied.is_err(), "无 Bearer 的管理 API 应失败");

    // 经隧道 Ping 节点
    let mut sys = SystemServiceClient::new(channel);
    let mut req = with_bearer(Request::new(PingRequest {}), token);
    req.metadata_mut().insert(
        META_NODE_ID,
        MetadataValue::try_from(node_id.as_str()).expect("meta"),
    );
    let ping = sys.ping(req).await.expect("tunnel Ping");
    assert!(ping.into_inner().ok, "隧道 Ping 应返回 ok=true");

    agent_join.abort();
    let _ = ctrl_stop_tx.send(());
    let _ = ctrl_join.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_rejects_wrong_controller_token() {
    let ctrl_dir = TempDir::new().expect("ctrl dir");
    let node_dir = TempDir::new().expect("node dir");
    let ctrl_addr = free_addr().await;
    let ctrl_url = format!("http://{ctrl_addr}");
    let token = "controller-secret";

    let (ctrl_stop_tx, ctrl_stop_rx) = oneshot::channel::<()>();
    let ctrl_join = tokio::spawn(async move {
        let _ = run_controller(
            ControllerListenParams {
                bind: ctrl_addr,
                token: token.into(),
                data_dir: ctrl_dir.path().to_path_buf(),
                tls: None,
            },
            async move {
                let _ = ctrl_stop_rx.await;
            },
        )
        .await;
    });
    wait_connect(&ctrl_url).await;

    let node_runtime = RuntimeConfigBuilder::new()
        .grpc_listen("127.0.0.1:0".parse().unwrap())
        .data_dir(node_dir.path().to_path_buf())
        .build();
    let (node_state, _) = AppState::bootstrap(node_runtime).expect("bootstrap");

    // 节点用错误 token：会话应失败并重试；控制端 ListNodes 应一直为空
    let agent_state = node_state.clone();
    let agent_join = tokio::spawn(async move {
        run_agent_loop(
            agent_state,
            AgentConnectConfig {
                controller: ctrl_url.clone(),
                token: "wrong-secret".into(),
                retry_base: Duration::from_millis(150),
                retry_max: Duration::from_millis(300),
                heartbeat_interval: Duration::from_secs(60),
                tls: Default::default(),
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(800)).await;
    let channel = Channel::from_shared(format!("http://{ctrl_addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut nodes = NodeServiceClient::new(channel);
    let resp = nodes
        .list_nodes(with_bearer(
            Request::new(ListNodesRequest {
                online_only: false,
            }),
            token,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(
        resp.nodes.is_empty(),
        "错误 token 不应出现在线节点: {:?}",
        resp.nodes
    );

    agent_join.abort();
    let _ = ctrl_stop_tx.send(());
    let _ = ctrl_join.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_dials_controller_over_tls() {
    use astral_core::tls_util::{build_client_tls, ClientTlsOpts, ServerTlsPaths};
    use std::fs;

    let token = "tls-shared-secret";
    let ctrl_dir = TempDir::new().expect("ctrl dir");
    let node_dir = TempDir::new().expect("node dir");
    let cert_dir = TempDir::new().expect("cert dir");

    let mut params = rcgen::CertificateParams::new(vec!["localhost".into()]).expect("sans");
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "localhost");
    let key_pair = rcgen::KeyPair::generate().expect("key");
    let cert = params.self_signed(&key_pair).expect("cert");
    let cert_path = cert_dir.path().join("server.pem");
    let key_path = cert_dir.path().join("server.key");
    fs::write(&cert_path, cert.pem()).expect("write cert");
    fs::write(&key_path, key_pair.serialize_pem()).expect("write key");

    let ctrl_addr = free_addr().await;
    let port = ctrl_addr.port();
    let ctrl_url = format!("https://localhost:{port}");
    let bind: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    let (ctrl_stop_tx, ctrl_stop_rx) = oneshot::channel::<()>();
    let ctrl_params = ControllerListenParams {
        bind,
        token: token.into(),
        data_dir: ctrl_dir.path().to_path_buf(),
        tls: Some(ServerTlsPaths {
            cert: cert_path.clone(),
            key: key_path,
        }),
    };
    let ctrl_join = tokio::spawn(async move {
        let _ = run_controller(ctrl_params, async move {
            let _ = ctrl_stop_rx.await;
        })
        .await;
    });
    tokio::time::sleep(Duration::from_millis(250)).await;

    let node_runtime = RuntimeConfigBuilder::new()
        .grpc_listen("127.0.0.1:0".parse().unwrap())
        .data_dir(node_dir.path().to_path_buf())
        .build();
    let (node_state, _) = AppState::bootstrap(node_runtime).expect("bootstrap");
    let node_id = node_state.node_id.clone();

    let agent_cfg = AgentConnectConfig {
        controller: ctrl_url.clone(),
        token: token.into(),
        retry_base: Duration::from_millis(200),
        retry_max: Duration::from_secs(2),
        heartbeat_interval: Duration::from_secs(30),
        tls: ClientTlsOpts {
            ca_cert: Some(cert_path.clone()),
            domain: Some("localhost".into()),
        },
    };
    let agent_join = tokio::spawn(async move {
        run_agent_loop(node_state, agent_cfg).await;
    });

    let channel = Channel::from_shared(ctrl_url.clone())
        .unwrap()
        .tls_config(
            build_client_tls(
                &ctrl_url,
                &ClientTlsOpts {
                    ca_cert: Some(cert_path),
                    domain: Some("localhost".into()),
                },
            )
            .expect("tls"),
        )
        .expect("tls cfg")
        .connect()
        .await
        .expect("https channel");

    let mut nodes = NodeServiceClient::new(channel.clone());
    let mut seen = false;
    for _ in 0..80 {
        let resp = nodes
            .list_nodes(with_bearer(
                Request::new(ListNodesRequest {
                    online_only: true,
                }),
                token,
            ))
            .await
            .expect("ListNodes")
            .into_inner();
        if resp.nodes.iter().any(|n| n.node_id == node_id && n.online) {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(seen, "TLS 控制端未看到节点 {node_id}");

    let mut sys = SystemServiceClient::new(channel);
    let mut req = with_bearer(Request::new(PingRequest {}), token);
    req.metadata_mut().insert(
        META_NODE_ID,
        MetadataValue::try_from(node_id.as_str()).expect("meta"),
    );
    let ping = sys.ping(req).await.expect("tunnel Ping over TLS");
    assert!(ping.into_inner().ok);

    agent_join.abort();
    let _ = ctrl_stop_tx.send(());
    let _ = ctrl_join.await;
}
