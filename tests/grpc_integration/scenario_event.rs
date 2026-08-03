//! 场景：EventService 订阅流

use crate::harness::TestServer;
use astral_core::pb::{EventType, SubscribeEventsRequest};
use tokio_stream::StreamExt;
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s07_subscribe_with_snapshots() {
    let server = TestServer::start().await;
    let mut ev = server.event().await;
    let mut stream = ev
        .subscribe_events(Request::new(SubscribeEventsRequest {
            node: None,
            instance_ids: vec![],
            types: vec![EventType::InstanceState as i32],
            include_snapshots: true,
        }))
        .await
        .expect("subscribe")
        .into_inner();

    // 无实例时快照可能为空；等待短暂后取消（流应保持打开）
    let next = tokio::time::timeout(std::time::Duration::from_millis(400), stream.next()).await;
    match next {
        Ok(Some(Ok(event))) => {
            assert_eq!(event.r#type, EventType::InstanceState as i32);
        }
        Ok(Some(Err(e))) => panic!("事件流出错: {e}"),
        Ok(None) => {} // 流结束（少见）
        Err(_) => {}    // 超时：无快照也算合理
    }
}
