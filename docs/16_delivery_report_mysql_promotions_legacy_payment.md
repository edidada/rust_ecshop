# 交付报告：MySQL 运行迁移、促销与旧支付适配

日期：2026-10-30。最终提交：`f626127`（已推送 origin main + codeup main，工作区干净）。

**Actions 结论（HEAD）**：run **41063563633** — completed/**success**
https://github.com/edidada/rust_ecshop/actions/runs/41063563633

- build (macos / ubuntu / windows)：check、clippy `-D warnings`、build、test、HTTP+mock 验收（SQLite）
- MySQL 8 runtime acceptance：mysql:8.0 service + `ECSHOP_AUTO_MIGRATE=1` 运行迁移 + connector fixture seed + 同一验收脚本

## 交付内容

1. **MySQL 运行迁移**：`generate_mysql_schema.py` 静态转换（user_rank 显式 rank 0、DROP 引用自增列的跨列 CHECK、TEXT 键列升 VARCHAR(190)），87 DDL/39 seed 可审阅产物；`bootstrap_mysql` `GET_LOCK('rust_ecshop_schema')` + `ecs_schema_migrations=1` 版本门。
2. **促销/赠品**：`pricing.rs`（rank 折扣 vs member 指定价、阶梯价、限时价 checked 算术）；`flow.rs`（act_range 全场/分类递归深度10/品牌/商品按适用商品金额计、封顶 min(applicable)、FAT_PRICE/FAT_DISCOUNT/FAT_GOODS、user_rank 白名单、赠品 allow-list + 库存校验写 `ecs_cart_gift`）；下单事务内二次校验 + 零价 order_goods + 扣库存，取消自动恢复；`ecs_order_promotion('favourable')` 快照；赠品/优惠订单禁止 merge。
3. **旧支付**：`POST /api/v1/me/orders/:id/payment-intents`（剩余应付分、pay_id/provider 绑定、旧 pending intent 失效）；Chinabank MD5（CNY、pstatus=20、ok/error、常量时间比较）；Alipay ksort MD5（success/fail、seller_email/partner 与服务端配置匹配强制）；`respond.php` code 白名单分发，重复 form key/超限拒绝；`receive.php` Bearer 认证收货（共享 `confirm_receipt_tx`）；`ecs_pay_log(provider, trade_no)` 唯一 → 幂等重放 + 跨订单重用拒绝。
4. **密钥 env**：`ECSHOP_CHINABANK_KEY` / `ECSHOP_ALIPAY_KEY` / `ECSHOP_ALIPAY_PARTNER` / `ECSHOP_ALIPAY_ACCOUNT`，未配置即渠道禁用。
5. **验收脚本**：两 job 共用，覆盖会员/阶梯/promote 定价（39.90/60.00/71.91、quote==order）、促销范围与 rank 门（403）、赠品库存扣减/恢复、Chinabank/Alipay 签名向量/错误签名/失败状态/过期意图/重放/跨订单重用/伪造商户/未知插件 400「payment channel not enabled」。
6. **验收记录**：`docs/15_mysql_promotions_legacy_payment_acceptance.md`（基线 run 41062919999 四 job success + 260 条逐请求证据）。

## 修复迭代

共 6 次推送迭代至全绿（生命周期 elision、i64 封顶、SQLite 方言、MySQL #3818/TEXT 键、connector `%s`、seed 注释剥离），全部记录于提交历史。

## 边界

其他历史插件（PayPal IPN、银联、财付通等）显式未启用；未做真实渠道联调；submodule 固定未动（725956b）。
