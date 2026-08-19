
本机单例内核：嵌入 EasyTier，在 `127.0.0.1:50051` 对本机 Astral GUI 提供 JSON-RPC 2.0。无 Token、无远程中控。

系统服务名固定为 `dev.astral.core`。

```bash
astral-core run
astral-core service install
astral-core service update --program ./astral-core
```

对本机 `POST http://127.0.0.1:50051/`，body 为 JSON-RPC 2.0：

- `ping`
- `info`
- `instance.start` / `instance.stop` / `instance.get` / `instance.list_meta`
- `network.status`
- `logs.recent`（`after` + `limit`，GUI 轮询）
