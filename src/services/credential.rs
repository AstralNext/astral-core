//! CredentialService：管理 API Token。

use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::credential_service_server::CredentialService;
use crate::pb::{
    CreateTokenRequest, CreateTokenResponse, GenerateNetworkCredentialRequest,
    GenerateNetworkCredentialResponse, ListNetworkCredentialsRequest,
    ListNetworkCredentialsResponse, ListTokensRequest, ListTokensResponse,
    RevokeNetworkCredentialRequest, RevokeNetworkCredentialResponse, RevokeTokenRequest,
    RevokeTokenResponse, RotateNodeDeviceKeyRequest, RotateNodeDeviceKeyResponse,
    RotateTokenRequest, RotateTokenResponse, TokenMeta,
};
use crate::services::util::ts_from_unix;

/// CredentialService 服务端。
pub struct CredentialSvc {
    state: AppState,
}

impl CredentialSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl CredentialService for CredentialSvc {
    async fn create_token(
        &self,
        req: Request<CreateTokenRequest>,
    ) -> Result<Response<CreateTokenResponse>, Status> {
        let r = req.into_inner();
        let expires = r.expires_at.map(|t| t.seconds);
        let (rec, plain) = self
            .state
            .tokens
            .create(r.name.clone(), expires)
            .map_err(Status::from)?;
        Ok(Response::new(CreateTokenResponse {
            token_id: rec.token_id,
            name: rec.name,
            token: plain,
            prefix: rec.prefix,
            created_at: Some(ts_from_unix(rec.created_at_unix)),
            expires_at: rec.expires_at_unix.map(ts_from_unix),
        }))
    }

    async fn list_tokens(
        &self,
        req: Request<ListTokensRequest>,
    ) -> Result<Response<ListTokensResponse>, Status> {
        let include = req.into_inner().include_revoked;
        let tokens = self
            .state
            .tokens
            .list(include)
            .map_err(Status::from)?
            .into_iter()
            .map(|t| TokenMeta {
                token_id: t.token_id,
                name: t.name,
                prefix: t.prefix,
                created_at: Some(ts_from_unix(t.created_at_unix)),
                expires_at: t.expires_at_unix.map(ts_from_unix),
                revoked: t.revoked,
            })
            .collect();
        Ok(Response::new(ListTokensResponse { tokens }))
    }

    async fn revoke_token(
        &self,
        req: Request<RevokeTokenRequest>,
    ) -> Result<Response<RevokeTokenResponse>, Status> {
        self.state
            .tokens
            .revoke(&req.into_inner().token_id)
            .map_err(Status::from)?;
        Ok(Response::new(RevokeTokenResponse {}))
    }

    async fn rotate_token(
        &self,
        _req: Request<RotateTokenRequest>,
    ) -> Result<Response<RotateTokenResponse>, Status> {
        Err(Status::unimplemented("RotateToken 尚未实现"))
    }

    async fn generate_network_credential(
        &self,
        _req: Request<GenerateNetworkCredentialRequest>,
    ) -> Result<Response<GenerateNetworkCredentialResponse>, Status> {
        Err(Status::unimplemented("GenerateNetworkCredential 尚未实现"))
    }

    async fn list_network_credentials(
        &self,
        _req: Request<ListNetworkCredentialsRequest>,
    ) -> Result<Response<ListNetworkCredentialsResponse>, Status> {
        Err(Status::unimplemented("ListNetworkCredentials 尚未实现"))
    }

    async fn revoke_network_credential(
        &self,
        _req: Request<RevokeNetworkCredentialRequest>,
    ) -> Result<Response<RevokeNetworkCredentialResponse>, Status> {
        Err(Status::unimplemented("RevokeNetworkCredential 尚未实现"))
    }

    async fn rotate_node_device_key(
        &self,
        _req: Request<RotateNodeDeviceKeyRequest>,
    ) -> Result<Response<RotateNodeDeviceKeyResponse>, Status> {
        Err(Status::unimplemented("RotateNodeDeviceKey 尚未实现"))
    }
}
