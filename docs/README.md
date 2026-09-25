# cpp_ecshop 实现蓝图

本目录不是 PHP 代码的逐行翻译，而是把参考项目 `/Users/ibqo/Develop/git/github/php/ecshop/upload` 的业务能力，拆成可逐步实现、可测试的 C++ HTTP 服务计划。先完成商品、购物车、账号和订单主路径，再补齐促销、支付回调、后台和页面渲染。

## 阅读顺序

1. [01_libraries.md](01_libraries.md)：技术栈、CMake 依赖和学习顺序。
2. [07_platform_compatibility.md](07_platform_compatibility.md)：macOS、Linux、Windows 兼容规则与 CI 矩阵。
3. [08_first_url_tutorial.md](08_first_url_tutorial.md)：已实现的第一个 URL、运行与仿写步骤。
4. [09_http_server_adapters.md](09_http_server_adapters.md)：Crow/httplib 编译期选择、适配器边界与 route module 规则。
5. [10_api_error_factories.md](10_api_error_factories.md)：错误响应工厂、`[[nodiscard]]` 与 HTTP 409 错误码约定。
6. [02_url_api_list.md](02_url_api_list.md)：PHP URL 与新 REST URL 的对应表。
7. [03_api_contract.md](03_api_contract.md)：请求、响应、状态码与鉴权约定。
8. [04_architecture.md](04_architecture.md)：分层和模块边界。
9. [05_detailed_design.md](05_detailed_design.md)：从路由到 SQL 的实现细节。
10. [sql/README.md](sql/README.md)：原始 88 张表、核心迁移脚本与导入次序。
11. [06_curl_testing.md](06_curl_testing.md)：可复制的 HTTP 验证流程。
12. [deploy/README.md](deploy/README.md)：Docker、Nginx、systemd 部署模板。

## 参考范围与约束

- 参考版本：PHP ECSHOP 的 `upload/` 目录；入口是若干 `*.php` 文件，数据库结构源文件是 `upload/install/data/structure.sql`。
- 源结构文件校验：SHA-256 为 `83a0a7bfd417999132665e7db7a3d4c8627bd7e8fa8ee932952cc4d4246b6a26`，共 88 个 `CREATE TABLE`。
- 新项目目标：MySQL 8、InnoDB、utf8mb4、参数化 SQL、JSON API。不要继承 PHP 中的 MyISAM、拼接 SQL、MD5 密码或 `act`/`step` 超级入口设计。
- 当前仓库只有 CMake 与 `main.cpp` 骨架；本文档不声称接口已经实现。

## 里程碑

| 阶段 | 可交付能力 | 验收 |
| --- | --- | --- |
| M0 | CMake、配置、健康检查、日志 | `GET /healthz` 返回 200 |
| M1 | 分类、商品详情、搜索 | 商品列表与详情查询 |
| M2 | 注册、登录、地址、购物车 | Cookie/JWT 会话下可增删购物车 |
| M3 | 结算、下单、库存扣减 | 事务内创建订单与订单项 |
| M4 | 支付回调、发货、取消/退款 | 幂等回调和订单状态机 |
| M5 | 后台、促销、内容、监控 | 权限、审计、指标和备份 |
