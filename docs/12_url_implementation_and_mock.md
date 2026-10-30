# 前台 URL 实现与 mock 验收（2026-10-30）

本轮以 `09_url_completion_matrix.md` 中的前台入口、user action 和 flow step 为范围，保留既有 REST 路由，补齐缺失的写操作。PHP 模板由客户端替代；管理后台不在该矩阵中。本地仅运行格式化、Python AST 解析、路由清单检查和 git diff --check；Rust 编译、Clippy、单测及 HTTP 回归全部交给 GitHub Actions。

## 新增接口合同

| PHP 动作 | REST URL | 请求与行为 |
| --- | --- | --- |
| flow clear | DELETE `/api/v1/me/cart` | 清空当前用户购物车和礼包绑定 |
| flow link_buy | POST `/api/v1/me/cart/linked-buy` | goods_id、quantity；事务替换购物车 |
| flow drop_to_collect | POST `/api/v1/me/cart/:id/favorite` | 归属检查、加入收藏、移除购物车项 |
| flow add_package_to_cart | POST `/api/v1/me/cart/packages/:id` | 事务替换为礼包成员，结算验证成员与礼包价格 |
| flow add_favourable | POST `/api/v1/me/cart/favourables/:id` | 绑定全场减额/折扣活动；重新检查有效期和金额门槛 |
| flow select/change | PATCH `/api/v1/checkout/selection/:field` | `{"value":...}`；字段见下文 |
| flow checkout selections | GET `/api/v1/checkout/selection` | 读取持久化选项、包装与贺卡 |
| flow check_surplus/check_integral | GET `/api/v1/checkout/funds` | 当前可用余额与积分 |
| flow validate_bonus | POST `/api/v1/checkout/bonuses/:id/validation` | 检查归属、未使用、时间窗和购物车门槛 |
| exchange buy | POST `/api/v1/exchange-goods/:id/redemptions` | quantity + Idempotency-Key；事务扣积分和库存，持久化兑换单 |
| auction/snatch bid | POST `/api/v1/promotions/:id/bids` | price 十进制字符串；检查时间、价阶、重复出价、保证金/积分 |
| group_buy/auction/snatch buy | POST `/api/v1/promotions/:id/orders` | quantity、address_id、shipping_id、payment_id + Idempotency-Key；团购按 cur_price；竞拍/夺宝须结束且为中标用户，限制购买一次 |
| respond/receive/chinabank_receive | POST `/respond.php`、`/receive.php`、`/chinabank_receive.php` | 共用签名 JSON 回调合同；旧渠道字段须由适配器转成此合同 |

selection 字段：shipping、payment、pack、card、integral、bonus 为整数 ID/数量；surplus 为金额字符串；invoice、insure 为 0/1；invoice-title、invoice-content、oos 为字符串。oos 记录 cancel/wait/partial 偏好，但当前下单库存不足统一拒绝，不执行预售或拆单。保险仅 mock 模式可选择，mock 保费为零。选择的 shipping/payment 在报价或下单未提供对应 ID 时生效，显式请求 ID 优先。

报价与下单重新计算包装/贺卡费、积分与红包优惠及余额抵扣；取消待支付订单退回抵扣余额、积分与红包。订单保留选择快照。余额支付包含支付手续费，渠道回调校验剩余应付；同一流水重放不重复记账，已支付订单收到不同实付流水返回冲突，需要对账。

商品报价、购物车、结算与下单共享基础价/限时促销价、属性附加费与 SKU 库存校验。购物车 POST 新增可选 product_id、attribute_ids，当前每用户每商品仅保留一种规格，再加另一规格返回冲突，需先删除原项。订单保存规格快照，取消恢复 SKU 库存。会员/阶梯价格、复杂赠品及原 PHP 序列化配置的迁移仍不在此价格模型内，不能宣称与旧站全规则等价。

## 第三方 mock

仅 `ECSHOP_INTEGRATION_MODE=mock` 且 `ECSHOP_ENV` 不等于 production 时启用；默认返回 404。所有 mock 接口需 Bearer 认证并限制当前用户资源。

- POST `/api/v1/mock/orders/:id/pay`，body `{"provider_trade_no":"unique-trade"}`：根据数据库剩余应付构造 HMAC 通知，调用真实回调处理，不连接支付机构。
- POST `/api/v1/mock/deposits/:id/pay`：结算已准备支付意图的充值申请，幂等增加余额及流水。
- GET `/api/v1/mock/mailbox`：读取当前用户 outbox，包含验证/找回凭据，仅用于本地或 CI mock。不会发送真实邮件。
- POST `/api/v1/mock/orders/:id/ship`：paid -> shipped，生成 MOCK 物流号和操作日志。确认收货必须 shipped -> received。
- 原积分转换 API 使用站内固定开发汇率，不调用 UCenter；真实 UCenter 同步与失败补偿不属于已接入服务。

回调头为 X-Payment-Signature（共享密钥对原始 body 的 HMAC-SHA256 十六进制）；JSON 包含 order_sn、provider_trade_no、amount。旧网银签名/字段协议没有冒充真实实现，测试使用此 mock 适配合同。

## 验收证据边界

`scripts/http_acceptance.py` 在 Actions 已构建二进制上启动独立 SQLite 测试服务，运行下单/部分余额付款/支付重放/发货收货/取消退款/下架商品/SKU/礼包/兑换/团购/竞价/邮箱状态/会话到期场景，并探测全部注册 URL 的方法和错误响应。逐 URL 探测只证明路由与基础响应；不能把 401/404 计为业务成功。`http-results.json` 将 route-smoke 单独标记；HTTP 服务日志和请求结果以各平台 artifact 保存。

本轮仍保留审计中的 MySQL 方言、表结构迁移、真实渠道适配、复杂促销与并发压力验证边界。当前新增 schema 是 SQLite 开发实现；不能将驱动可连接或三平台构建成功表述为 MySQL 生产验收。

远程运行链接和最终结论由交付消息给出；本文件不预写尚未发生的通过结果。
