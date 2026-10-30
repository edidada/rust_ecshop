# Rust 重写 ECShop 静态审计（2026-10-30）

## 结论与审计边界

当前实现已有较广的 JSON API 路由和 SQLite 开发数据模型，但不能据此认定完成 PHP ECShop 等价重写或满足生产交易要求。优先阻断项是支付金额一致性、MySQL 可部署性，以及商品状态与价格规则的一致性。

- Rust 基线：`46d79f55d666fedd8f48e8bccbc3237236c4b1cd`（main）。
- PHP 原代码：git submodule `third_party/shopex-ecshop`，固定提交 `725956bfee1ebcdb8b0eae87027b76be9571be1a`（本地标记 2.7.3）。未更新子模块版本。
- 开始时 `Cargo.lock` 显示修改，但 `git diff` 无内容差异，`git add Cargo.lock` 后工作区干净，属于换行符规范化，无可提交内容。main 无 upstream，普通 `git pull` 失败后显式执行 `git pull origin main`，结果 Already up to date。
- 方法：读取 Rust 路由、认证、配置、数据库与金额辅助层、SQL、CI，并重点对照 PHP `flow.php`、`includes/lib_order.php`、`includes/lib_payment.php`、安装表结构。覆盖矩阵中的其余业务只核对接口范围，不构成逐函数完整审计。
- 本次没有本地编译、运行服务、连接数据库、执行支付请求或修改业务实现。下文触发步骤是待执行的回归场景，不能当作已复现运行结果。
- P1：影响资金、业务正确性或阻断目标部署；P2：安全生命周期、兼容性与验收缺口。

## 主要发现

### F01 / P1：余额支付忽略支付手续费，可提前置为已支付

证据：`src/http/routes/order.rs:637` 的 `order_surplus` 只查询 `goods_amount` 和 `shipping_fee`，`total_due` 不含 `payment_fee`；而 `order_create`（同文件 167 行）将三者相加为 `order_amount`。余额累计达到前两者后便将订单置为 `paid`。

触发场景：商品 100.00、运费 10.00、手续费 5.00，余额付款 110.00 即可使标价 115.00 的订单变为 paid。若业务规定余额支付免手续费，也应同步调整费用与应付额；当前没有该处理，账目仍不一致。

建议：统一生成总额、已支付、剩余应付三类金额，明确支付方式费率规则；余额付款和渠道付款共同引用该模型。回归覆盖零手续费、非零手续费、部分付款、全额付款与改支付方式。

### F02 / P1：部分余额与外部付款不能正确组合，重复实付也缺少处理

证据：`src/http/routes/payment.rs` 的 `payment_callback` 将回调金额与原 `order_amount` 直接比较，不读取 `ecs_order_balance_payment.paid_cents`；`order_surplus` 记录余额付款但不扣减 `order_amount`。订单已经 paid 时，对新的渠道流水仍插入 paid 日志并确认成功，没有退款或异常对账分支。

触发场景：100.00 订单先付余额 30.00，渠道正常回调 70.00 会被拒绝；回调 100.00 则可能被接受，造成累计实付 130.00。余额全付后再收到外部实付，也只会记录并确认。

PHP 对照：`third_party/shopex-ecshop/upload/includes/lib_payment.php:140` 的 `order_paid` 区分支付日志与订单，支付成功会更新 `money_paid`、清零 `order_amount`。Rust 可以重新设计模型，但必须保持资金守恒。

建议：按订单锁定并核对剩余应付，支付渠道绑定订单支付意图；重复通知与真实重复收款分别处理。回归覆盖同一流水重放、不同流水重复付款、余额和回调交错、取消和回调竞争。

### F03 / P1：MySQL 驱动接入不等于 MySQL 业务可运行

证据：`src/infrastructure/db.rs` 对 MySQL 只初始化驱动并提示 DBA 预置 schema；`src/shared/rbutil.rs` 将 SQL 原样交给 Executor。`src/http/routes/order.rs:442` 附近购物车回填、686 行余额付款、843 行地址更新采用 SQLite `ON CONFLICT ... DO UPDATE`，切换 MySQL 驱动不会自动改写这些方言。

此外，`docs/sql/001_core_schema.mysql8.sql` 创建无 `ecs_` 前缀的 `users`、`shipping`、`order_info` 等表，Rust 查询采用 `ecs_*`；脚本订单时间字段为 `add_time DATETIME`，Rust 使用 `created_at` 和 Unix 秒。PHP 原库虽然使用 `ecs_`，用户密码字段为 `password`/`ec_salt`/`salt`，也不能满足 Rust 的 `password_hash` 查询。两套现有基线都不能直接作为完整 Rust 生产库。

建议：建立 Rust 专用、版本化 MySQL migrations，逐表核对名称、列、类型、索引与新增业务表；将 UPSERT 等语句按后端实现。验收必须从空 MySQL 库初始化，再实际完成注册、购物车、下单、余额及支付回调，不能只验证连接成功。

### F04 / P1：下单没有复查商品在售和删除状态

证据：`src/http/routes/order.rs:134` 的购物车读取只按用户筛选；151 行库存扣减只检查库存数量，不检查 `is_on_sale`、`is_delete`。

触发场景：合法加入购物车后商品被下架或软删除，库存仍足够，当前下单路径仍可成交。

PHP 对照：`third_party/shopex-ecshop/upload/includes/lib_order.php:1012` 的 `addto_cart` 过滤删除状态并拒绝下架商品。Rust 在加购物车阶段校验仍不足以覆盖下单前状态变化。

建议：在交易内重新校验可售性，并将可售条件纳入库存条件更新；验证商品下架、删除、库存变化发生于加购之后的情况。

### F05 / P1：商品报价与订单落价链路不统一，规格与 SKU 语义缺失

证据：`src/http/routes/goods.rs:463` 的 `price_quote` 可接收属性并累加属性价格；`src/http/routes/order.rs:134` 下单只读取商品 `shop_price`，订单创建请求只有地址、配送、支付和备注，不带规格；购物车查询和订单项插入只使用 goods_id 与数量，没有 product_id 或属性快照。

PHP 对照：`includes/lib_order.php:1108` 调用 `get_final_price($goods_id, $num, true, $spec)`，保存 product_id，库存调整也区分 product_id（2261 行起）。Rust 当前路径无法保持报价属性、SKU 库存和订单价格一致；会员价、阶梯价、活动价也没有在此落价路径应用。

建议：统一价格计算服务，在购物车、结算、下单中使用相同规则并生成订单快照；按 SKU 扣减库存。验收覆盖附加属性价格、不同 SKU、阶梯数量和活动有效期。若暂不支持，接口应明确拒绝，文档标为部分实现。

### F06 / P2：修改邮箱后保留旧邮箱验证状态

证据：`src/http/routes/auth.rs:242` 的 `me_patch` 只执行 `UPDATE ecs_users SET email = ?`，没有同步清除 `is_validated`；`me` 仍将该字段返回为 `email_verified`。

触发场景：已验证邮箱 A 的用户修改为邮箱 B，无需验证 B 即继续显示已验证。需进一步动态验证旧邮箱 token 在修改后是否仍可用。

建议：邮箱变化时清除验证标记，撤销旧验证 token，并把新验证凭据绑定目标邮箱。回归验证状态与 token 对邮箱地址的绑定。

### F07 / P2：Bearer 会话没有过期检查

证据：`src/http/auth.rs:9` 仅按 token_hash 查询会话和用户；`insert_session_tx` 只插入 token_hash、user_id。没有在认证路径检查绝对到期时间或空闲时间。

影响：泄露的 token 在主动注销或密码变更撤销前持续有效。已有随机 token、哈希存储、注销及密码修改撤销是有效基础，但不等于会话有效期管理。

建议：增加 expires_at，并在查询中强制检查；覆盖到期边界、注销、密码重置与多会话撤销。

### F08 / P2：旧 URL 的 DONE 标记高估业务完成程度

证据：`docs/09_url_completion_matrix.md` 将部分入口标为 DONE，同时文字明确积分兑换写操作、竞拍/团购写操作与 flow 细分结算动作尚未完成；`src/http/routes/mod.rs` 的交易路径仅提供较简化 REST 接口。该矩阵还出现 C++ 列名、SQLite/DEV 等历史文本。

PHP `flow.php` 实际具有地区配送配置、余额、积分、红包、包装、贺卡、发票等多步骤逻辑；Rust 下单使用固定配送费和固定支付费。`order_received`（`order.rs:378` 附近）直接允许 paid -> received，缺少独立发货状态门槛；PHP `lib_payment.php` 则独立维护支付和配送状态。仅有类似名称的路由不代表这些规则被实现。

建议：将覆盖拆为“有路由、持久化、规则等价、异常/并发验收”四个维度；明确当前仅重写前台 JSON API，管理后台、模板、原支付插件和数据迁移不能默认列入已完成范围。前端替代 HTML 是可接受设计，但需独立定义兼容合同。

## 其他风险与已有基础

- 金额辅助 `src/shared/rbutil.rs` 的 `col_cents` 先经 f64 再转分，落库也常使用 `as f64 / 100.0`，不属于全链路整数分。建议采用整数分或精确十进制并检查所有加法溢出；目前没有金额边界运行证据。
- 支付 provider 来自路径，回调仅验证共用密钥对 body 的 HMAC，没有在该函数绑定支付意图/订单支付渠道；生产默认密钥保护只在 `ECSHOP_ENV=production` 时启用（`src/http/state.rs`）。部署验收应检查环境和渠道绑定。
- 事务存在不代表跨后端并发安全：例如改支付方式先读 pending，再按 order_id 无状态条件 UPDATE。应在目标 MySQL 隔离级别下验证与回调并发，不能将 SQLite 行为直接外推。
- 正面基础：多数数据库输入使用绑定参数；订单和地址操作多有归属过滤；订单创建采用条件库存扣减、幂等键和事务；密码使用带随机盐 PBKDF2；会话存 token 哈希；已有余额流水及密码变更撤销会话。上述是代码结构确认，不代表全部错误分支已验收。
- PHP 用户表、密码和订单状态不能直接搬到 Rust。`docs/sql/README.md` 的迁移建议不是可执行迁移工具；正式迁移仍需字段映射、密码重置方案、数据校验与回滚演练。

## CI 证据与验收计划

通过 `gh run list` 和 `gh run view 36161410771 --json jobs` 实时查询：基线 SHA 对应 [GitHub Actions 36161410771](https://github.com/edidada/rust_ecshop/actions/runs/36161410771) 已 completed/success，Ubuntu、macOS、Windows 三个 build job 均 success，完成时间为 2026-09-25 UTC。

`.github/workflows/ci.yml` 已包含 check、RustCrypto 重复依赖检查、Clippy、build、test。但没有 MySQL service 或 HTTP 交易流程回归步骤；这些绿色结果只能证明当次 CI 定义的检查成功，不能排除本报告的业务缺陷。本次未触发新 CI。

建议按以下次序整改和验收：

1. 先解决 F01/F02：建立统一应付模型和支付意图，对账验证金额守恒。
2. 解决 F03：空 MySQL migrations 与真实接口集成测试，保留三平台构建检查。
3. 解决 F04/F05：商品可售条件、SKU 和统一价格快照，加入加购后状态变化测试。
4. 解决 F06/F07，并补邮箱 token 绑定、会话到期和撤销测试。
5. 重整覆盖矩阵，逐 action 列出支持范围；补发货/收货状态机、促销与迁移验收。
6. 支付、取消、余额、改支付方式、合并订单并发测试应检验最终库存、资金、订单状态和流水，而非只检查 HTTP 状态码。

本报告为静态审计交付，不是修复完成证明或上线批准；未执行的回归场景与未逐项深入的业务仍待后续验收。
