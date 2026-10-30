# curl 示例：请求体 JSON + 响应快照

本目录记录 HTTP 请求的 JSON body（`*.body.json`）、curl 命令读本体的写法，以及实测响应快照（`responses/`），全部随 git 追踪。

约定：

- 请求体文件以 `*.body.json` 命名；curl 用 `--data-binary @文件` 读取本体，避免转义.shell 引号。
- 响应用 `-o responses/<名字>.response.json` 落盘；快照里的 `access_token` 等一次性凭据已替换为 `***redacted***`。
- `curl-demo@example.test` / 用户名为示例数据，每个演示环境重新注册即可。

## 命令

启动服务（本地演示）：

```pwsh
$env:ECSHOP_PORT = "28995"
$env:ECSHOP_DATABASE_URL = "sqlite://target/curl-demo.sqlite3"
$env:ECSHOP_INTEGRATION_MODE = "mock"
$env:ECSHOP_ENV = "test"
target/debug/rust_ecshop.exe
```

健康检查（无 body，响应保存到文件）：

```bash
curl -sS http://127.0.0.1:28995/healthz \
  -o docs/curl/responses/healthz.response.json
```

注册（curl 从 JSON 文件读取 body 请求）：

```bash
curl -sS -X POST http://127.0.0.1:28995/api/v1/auth/register \
  -H "Content-Type: application/json" \
  --data-binary @docs/curl/register.body.json \
  -o docs/curl/responses/register.response.json
```

加购物车、下单等需要登录（把注册返回的 access_token 放进环境变量）：

```bash
TOKEN=$(python -c "import json;print(json.load(open('docs/curl/responses/register.response.json'))['access_token'])")
curl -sS -X POST http://127.0.0.1:28995/api/v1/me/cart \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  --data-binary @docs/curl/cart-add.body.json
```

checkout 报价（address_id 按实际创建的地址替换）：

```bash
curl -sS -X POST http://127.0.0.1:28995/api/v1/checkout/quote \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  --data-binary @docs/curl/checkout-quote.body.json
```
