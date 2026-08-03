//! 测试脚手架：临时 Core 进程内 gRPC + Bearer 客户端。

use std::time::Duration;

use astral_core::app::AppState;
use astral_core::config::RuntimeConfigBuilder;
use astral_core::grpc;
use astral_core::pb::acl_service_client::AclServiceClient;
use astral_core::pb::app_message_service_client::AppMessageServiceClient;
use astral_core::pb::backup_service_client::BackupServiceClient;
use astral_core::pb::config_service_client::ConfigServiceClient;
use astral_core::pb::credential_service_client::CredentialServiceClient;
use astral_core::pb::event_service_client::EventServiceClient;
use astral_core::pb::instance_config_source::Source as ConfigSourceOneof;
use astral_core::pb::instance_service_client::InstanceServiceClient;
use astral_core::pb::logger_service_client::LoggerServiceClient;
use astral_core::pb::network_service_client::NetworkServiceClient;
use astral_core::pb::node_service_client::NodeServiceClient;
use astral_core::pb::port_forward_service_client::PortForwardServiceClient;
use astral_core::pb::stats_service_client::StatsServiceClient;
use astral_core::pb::system_service_client::SystemServiceClient;
use astral_core::pb::vpn_portal_service_client::VpnPortalServiceClient;
use astral_core::pb::{
    InstanceConfigSource, InstanceRef, StartInstanceRequest, StartInstanceResponse,
};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tonic::metadata::MetadataValue;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;
use tonic::{Request, Status};

type AuthChannel = InterceptedService<Channel, BearerAuth>;

/// 客户端 Bearer 拦截器。
#[derive(Clone)]
pub struct BearerAuth {
    header: MetadataValue<tonic::metadata::Ascii>,
}

impl BearerAuth {
    pub fn new(token: &str) -> Self {
        let header = MetadataValue::try_from(format!("Bearer {token}"))
            .expect("token 应可写入 metadata");
        Self { header }
    }
}

impl tonic::service::Interceptor for BearerAuth {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        request
            .metadata_mut()
            .insert("authorization", self.header.clone());
        Ok(request)
    }
}

/// 单测用服务器：独立 data-dir + 随机端口。
pub struct TestServer {
    _data: TempDir,
    pub addr: String,
    pub token: String,
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl TestServer {
    pub async fn start() -> Self {
        let data = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let sock = listener.local_addr().expect("local_addr");
        let runtime = RuntimeConfigBuilder::new()
            .grpc_listen(sock)
            .data_dir(data.path().to_path_buf())
            .build();
        let (state, bootstrap) = AppState::bootstrap(runtime).expect("bootstrap");
        let token = bootstrap.expect("首次应生成引导 token");

        let (tx, rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            let _ = grpc::serve_with_incoming_shutdown(state, listener, async move {
                let _ = rx.await;
            })
            .await;
        });

        let endpoint = format!("http://{sock}");
        for _ in 0..80 {
            if Channel::from_shared(endpoint.clone())
                .unwrap()
                .connect()
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        Self {
            _data: data,
            addr: endpoint,
            token,
            shutdown: Some(tx),
            join: Some(join),
        }
    }

    async fn channel(&self) -> Channel {
        Channel::from_shared(self.addr.clone())
            .unwrap()
            .connect()
            .await
            .expect("connect")
    }

    async fn auth_channel(&self) -> AuthChannel {
        InterceptedService::new(self.channel().await, BearerAuth::new(&self.token))
    }

    pub async fn system(&self) -> SystemServiceClient<AuthChannel> {
        SystemServiceClient::new(self.auth_channel().await)
    }

    pub async fn instance(&self) -> InstanceServiceClient<AuthChannel> {
        InstanceServiceClient::new(self.auth_channel().await)
    }

    pub async fn credential(&self) -> CredentialServiceClient<AuthChannel> {
        CredentialServiceClient::new(self.auth_channel().await)
    }

    pub async fn node(&self) -> NodeServiceClient<AuthChannel> {
        NodeServiceClient::new(self.auth_channel().await)
    }

    pub async fn network(&self) -> NetworkServiceClient<AuthChannel> {
        NetworkServiceClient::new(self.auth_channel().await)
    }

    pub async fn event(&self) -> EventServiceClient<AuthChannel> {
        EventServiceClient::new(self.auth_channel().await)
    }

    pub async fn config(&self) -> ConfigServiceClient<AuthChannel> {
        ConfigServiceClient::new(self.auth_channel().await)
    }

    pub async fn logger(&self) -> LoggerServiceClient<AuthChannel> {
        LoggerServiceClient::new(self.auth_channel().await)
    }

    pub async fn backup(&self) -> BackupServiceClient<AuthChannel> {
        BackupServiceClient::new(self.auth_channel().await)
    }

    pub async fn vpn(&self) -> VpnPortalServiceClient<AuthChannel> {
        VpnPortalServiceClient::new(self.auth_channel().await)
    }

    pub async fn portforward(&self) -> PortForwardServiceClient<AuthChannel> {
        PortForwardServiceClient::new(self.auth_channel().await)
    }

    pub async fn acl(&self) -> AclServiceClient<AuthChannel> {
        AclServiceClient::new(self.auth_channel().await)
    }

    pub async fn stats(&self) -> StatsServiceClient<AuthChannel> {
        StatsServiceClient::new(self.auth_channel().await)
    }

    pub async fn app_message(&self) -> AppMessageServiceClient<AuthChannel> {
        AppMessageServiceClient::new(self.auth_channel().await)
    }

    /// 裸连接（不带 Token）。
    pub async fn bare_system(&self) -> SystemServiceClient<Channel> {
        SystemServiceClient::new(self.channel().await)
    }

    /// 错误 Token。
    pub async fn system_with_token(&self, token: &str) -> SystemServiceClient<AuthChannel> {
        SystemServiceClient::new(InterceptedService::new(
            self.channel().await,
            BearerAuth::new(token),
        ))
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

/// CI 友好的无 TUN 配置（固定 instance_id）。
pub fn smoke_toml() -> String {
    smoke_toml_with_id("22222222-2222-4222-8222-222222222222")
}

pub fn smoke_toml_with_id(id: &str) -> String {
    format!(
        r#"
instance_id = "{id}"
hostname = "grpc-smoke"
dhcp = true
listeners = ["udp://0.0.0.0:0"]
no_tun = true

[network_identity]
network_name = "grpc-smoke-net"
network_secret = "grpc-smoke-secret"
"#
    )
}

pub fn config_source_toml(toml: String) -> InstanceConfigSource {
    InstanceConfigSource {
        source: Some(ConfigSourceOneof::Toml(toml)),
    }
}

pub fn instance_ref(id: &str) -> InstanceRef {
    InstanceRef {
        node: None,
        instance_id: id.to_string(),
    }
}

/// 启动冒烟实例；失败时返回 None（CI 无特权可接受）。
pub async fn try_start_smoke(
    inst: &mut InstanceServiceClient<AuthChannel>,
) -> Option<StartInstanceResponse> {
    let resp = inst
        .start_instance(Request::new(StartInstanceRequest {
            node: None,
            config: Some(config_source_toml(smoke_toml())),
            display_name: "grpc-smoke".into(),
            source_path: String::new(),
        }))
        .await;
    match resp {
        Ok(r) => Some(r.into_inner()),
        Err(e) => {
            eprintln!("场景跳过 StartInstance（环境限制）: {e}");
            None
        }
    }
}

pub fn assert_unimplemented(err: &Status) {
    assert_eq!(
        err.code(),
        tonic::Code::Unimplemented,
        "期望 UNIMPLEMENTED，实际 {:?}: {}",
        err.code(),
        err.message()
    );
}
