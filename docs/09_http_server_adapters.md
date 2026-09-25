# 多 HTTP Server 适配与路由模块

项目可同时支持 Crow 与 cpp-httplib，但一次构建只选择其中一个 HTTP server：

```text
-DECSHOP_HTTP_SERVER=crow
-DECSHOP_HTTP_SERVER=httplib
```

这样最终只生成一个 `ecshop_api`，部署简单；CI 则以矩阵分别构建两个选择，避免另一套适配器长期失效。

## 分层边界

```text
Crow request ─────┐
                  ├─> api::GoodsApi ─> Use Case ─> Repository ─> SQLite
httplib request ──┘
```

| 层 | 职责 | 是否两套 |
| --- | --- | --- |
| `domain`、`application`、`infrastructure` | 业务规则、事务、数据库访问 | 否 |
| `api` | Request/Response DTO、参数校验、错误码、JSON 输出 | 否 |
| `http/crow`、`http/httplib` | 从框架请求提取参数，注册路由，写回框架响应 | 是，但必须很薄 |
| `HttpServer.cpp` | 创建 server、装配 route module、监听端口 | 是，但必须很小 |

不要在 Crow 与 httplib 的 route 文件中重复数字、枚举、日期或分页校验，JSON 字段、HTTP 状态码、错误文案、业务判断或 SQL。两端都将原始 path/query/body 值交给共享 `api` 层。

## 按 bounded context 拆路由

`HttpServer.cpp` 只负责装配。路由按业务边界而不是按 URL 数量拆分：

```text
src/http/
  crow/
    HttpServer.cpp
    health/Routes.cpp
    goods/Routes.cpp
    order/Routes.cpp          # 订单 API 实现后加入
    admin/Routes.cpp          # 后台 API 实现后加入
  httplib/
    HttpServer.cpp
    health/Routes.cpp
    goods/Routes.cpp
    order/Routes.cpp
    admin/Routes.cpp
```

当前已经实现的模块会生成以下独立 CMake targets：

| Context | Target | 当前 URL |
| --- | --- | --- |
| health | `ecshop_http_health` | `GET /healthz` |
| goods | `ecshop_http_goods` | 商品查询、库存预留 |

`ecshop_http` 仅链接这两个 context target，再由 `ecshop_api` 链接 `ecshop_http`。当订单或后台业务 API 实现后，为 Crow 和 httplib 各增加同名 `Routes.cpp`，并将 `order` 或 `admin` 加入 `ECSHOP_HTTP_CONTEXTS`；CMake 会生成 `ecshop_http_order` 或 `ecshop_http_admin`。每次构建只会编译和链接 `src/http/${ECSHOP_HTTP_SERVER}/` 下被选中的框架实现。

```text
main
 └─ ecshop_http
     ├─ ecshop_http_health
     ├─ ecshop_http_goods
     ├─ ecshop_http_order      # 未来
     └─ ecshop_http_admin      # 未来
          └─ 共享 api / application / infrastructure
```

## 扩展规则

1. 先在共享 `api` 层定义 Request、Response 和校验，再实现 Use Case 与 Repository。
2. 为每个 HTTP 框架仅增加参数提取和响应映射；两端调用同一个 API 方法。
3. 以 bounded context 创建 route module；不要为每个 URL 都创建一个 target。
4. 一个 context 内 URL 数量很大时，可继续按资源拆多个 `.cpp`，但保留同一个 context target。
5. 对共享 API 编写契约测试；针对每个框架运行同一份 HTTP 集成测试用例，确保状态码、错误码和 JSON 一致。

不建议先抽象出一个完全通用的 `Router` 接口来遮蔽 Crow 与 httplib。它通常会丢失框架的路由能力，最终形成难维护的第三套 HTTP 框架。保持两套薄适配器、一个共享 API 契约更直接。
