# MySQL 运行迁移与促销/旧支付验收记录

日期：2026-10-30。设计合同：`14_mysql_promotions_legacy_payment_design.md`（commit `c191ec5`）。
本轮实现头三次迭代（`f9109c2`、`70f69cc`、`9ba0b4f`）后经 CI 修复迭代，验收基线为 commit `a633831`。

## CI 结论

- HEAD run：**41062919999** — completed/**success**
  https://github.com/edidada/rust_ecshop/actions/runs/41062919999
- 四个 job 全部通过：
  - build (macos-latest) / (ubuntu-latest) / (windows-latest)：check、clippy `-D warnings`、build、test（含 RustCrypto 重复依赖 grep 门）、HTTP+mock 验收（SQLite）
  - MySQL 8 runtime acceptance：mysql:8.0 service、`ECSHOP_AUTO_MIGRATE=1` 运行迁移（`GET_LOCK` + 版本门 `ecs_schema_migrations=1`）、mysql-connector-python fixture seed、同一 HTTP 验收脚本
- 产物：`http-evidence-mysql`（`http-server.log`、`http-results.json` 260 条逐请求记录）。

## 中间失败与修复（均记录于提交历史）

1. `70f69cc`：`form_value` 缺失生命周期 elision；`i64::checked_min` 不存在 → `Ord::min` 封顶。
2. `9ba0b4f`：SQLite seed 方言（`INSERT IGNORE` → `INSERT OR IGNORE`）；MySQL CHECK 引用 AUTO_INCREMENT 列（#3818）→ 移除 `ecs_user_referral` 跨列 CHECK（调用点仍强制）。
3. `b4325fe`：TEXT/LONGTEXT 列进入键规范需长度 → 生成器把参与 PRIMARY KEY/UNIQUE 的 TEXT 列升为 VARCHAR(190)。
4. `81003ed`：mysql-connector 占位符 `?` → `%s`。
5. `a633831`：fixture seed 拆分前剥离 `--` 注释行（首行注释含 `;`）。

## MySQL 验收证据（run 41062919999 产物）

- 运行迁移：服务启动即建 87 张表并写入版本 1，无人工干预。
- 促销与定价：会员价 39.90 压过 rank 折扣、阶梯 60.00（≥3 件）、PATCH 后 71.91（90% rank 折扣）、quote==order 且取消恢复资产；全场减 5.00（`favourables/3`）→ 优惠订单建单；rank 限定 `favourables/4` → 403；赠品 allow-list/超上限 → 400，合法赠品 → 零价 order_goods 行 + 库存扣减，取消后恢复。
- 旧支付（MySQL 与三个 SQLite job 共用向量）：
  - Chinabank `v_oid=v_md5str…`：合法 20 → `ok` 且订单 paid；重放 → `ok`；签章篡改/pstatus=30/USD 币种/金额不符 → `error` 且订单保持 pending_payment。
  - Alipay ksort MD5：合法 TRADE_SUCCESS → `success` 且订单 paid；重放 → `success`；篡改 → `fail`；WAIT_BUYER_PAY → `fail` 不落账；新 intent 使旧 intent 失效后旧单 fail；金额不符 fail；跨订单复用 trade_no fail；商户 partner 伪造 fail。
  - `/respond.php?code=paypal` → 400「payment channel not enabled」（其他历史插件显式未启用）。
- receive.php：GET/POST Bearer 认证收货，未发货 409、发货后 200。

## 边界声明

- 其他历史插件（PayPal IPN、银联、财付通等）显式未启用；未做真实渠道联调。
- 不改 submodule（third_party/shopex-ecshop 固定在 725956b）。
