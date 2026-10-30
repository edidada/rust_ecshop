# MySQL、促销规则与旧支付适配设计

日期：2026-10-30。实现基线：294979be5181379a3afc06d9c722f3fd4cdd8bee。原 PHP 固定为 submodule 725956bfee1ebcdb8b0eae87027b76be9571be1a。本文件先于本轮业务编码提交；验收结果另记，设计不代表已实现或已通过。

## 目标与验收

1. MySQL 8 使用独立 Rust ecs_* schema、InnoDB、utf8mb4，字段与 SQLite 开发模型一致；从空库迁移，重复启动不破坏数据。CI 增加真实 MySQL 服务，运行与 SQLite 相同的 HTTP 场景及资金/库存断言。
2. 促销覆盖会员价、数量阶梯、限时价、属性附加费；优惠活动支持会员等级及全场/分类（含子分类）/品牌/指定商品范围，减额、折扣及赠品。报价和下单调用同一规则，优惠总额不超过适用商品金额；记录快照，取消恢复资产与赠品库存。
3. 旧入口恢复正确含义：respond.php 是渠道分发；chinabank_receive.php 是网银异步对账；receive.php 实际是确认收货，不能继续当支付回调。实现源代码中 Chinabank MD5、Alipay MD5 两类可本地验证的协议，并提供签名、商户、金额、支付意图、重放验证。其他插件必须显式报告未启用，不能伪装验签成功。
4. 本地只编辑、格式化和静态检查；不执行 Rust 编译、程序、数据库或测试。全部执行证据来自 push 后的 GitHub Actions；失败时读取 job 日志修正并再次推送。

## 数据库设计

新增 docs/sql/mysql/001_runtime.sql，使用 Rust 当前运行模型，不导入 PHP MyISAM DDL，也不复用历史无前缀的 core_schema。迁移只包含 DDL，不含演示数据；开发/CI seed 独立。日期统一 Unix 秒；金额为精确 DECIMAL 或整数分；用户/订单 ID 为 BIGINT。长 JSON/正文为 LONGTEXT，所有参与唯一索引的文本有界。迁移版本表记录已应用版本；启动迁移须 ECSHOP_AUTO_MIGRATE=1，部署账户可改用事先执行迁移后的最小权限账户。

共享 SQL 边界转换仅处理仓库受控静态语句：INSERT OR IGNORE、UPDATE OR IGNORE、ON CONFLICT ... DO UPDATE、excluded.column、标量 MIN。绑定参数不参与 SQL 转换。SQLite 保持原语义；MySQL 使用 INSERT IGNORE / UPDATE IGNORE、ON DUPLICATE KEY UPDATE、VALUES(column)、LEAST。DDL 与 seed 的转换在生成脚本中完成并输出可审阅 SQL，运行时不把 SQLite DDL 喂给 MySQL。

增加 ecs_user_write_lock，事务在读取用户余额、购物车、订单前锁定当前用户行，防止余额支付/取消/回调交错产生重复收款或退款；营销竞价继续使用活动锁。SQL 约束和条件 UPDATE 仍保留。MySQL 禁用“读后未经条件更新”的资金操作；失败必须回滚。锁顺序为用户 -> 活动 -> 商品（按 ID），回调也必须使用相同用户锁。

## 促销模型

新增 ecs_user_rank、ecs_member_price、ecs_volume_price，用户扩展 rank_id；等级折扣用百分比整数，会员指定价优先于等级折扣；随后与有效数量阶梯、有效限时价取较低基础价，再加属性价。所有金额运算 checked，边界失败拒绝请求。

活动 user_rank 为明确等级列表，0 表普通会员；不满足条件返回业务错误。act_range 按 PHP FAR_ALL=0/FAR_CATEGORY=1/FAR_BRAND=2/FAR_GOODS=3，分类递归包含后代并限制深度。门槛按适用商品当前服务端价格与数量计算。FAT_PRICE=1 减固定额，FAT_DISCOUNT=2 按保留价格百分比计，FAT_GOODS=0 从 gift JSON 允许集合选择赠品，act_type_ext 为可选赠品件数上限。赠品不得由客户端指定价格，重新校验可售性/库存并写零价订单项和审计快照；取消按订单项恢复库存。优惠、红包、积分按顺序叠加并封顶，礼包与优惠活动互斥，避免重复抵扣。

JSON 配置是新模型合同；旧 PHP serialize 数据须由离线迁移转换为 JSON，运行时不执行 PHP、不反序列化对象。会员、阶梯与活动均在下单事务中重读，报价不是库存预留。

## 旧支付协议与安全边界

新增 POST /api/v1/me/orders/:id/payment-intents，锁订单后绑定 pay_id、provider、商户外部编号和剩余应付整数分。订单改支付方式或继续余额付款后，原意图必须失效（金额、pay_id 二次核对），支付成功消费意图并写渠道流水。

- Chinabank：POST form v_oid/v_pstatus/v_amount/v_moneytype/v_md5str；按 PHP 拼接原始字段加 key 做 MD5，大小写兼容常量时间比较，币种必须 CNY，成功状态仅 20。v_oid 使用服务端支付意图编号；异步成功返回纯文本 ok，非法返回 error。签名本身不绑定 v_mid，因此商户账户从服务端 provider 配置与 intent 绑定，不能信任请求中的账户。
- Alipay：GET/POST form；按 PHP ksort 对字段名排序，排除 sign/sign_type/code，拼接 key=value&... 后追加 key 做 MD5。只支持 MD5，匹配 seller_email/partner（如提供），限制成功状态 TRADE_SUCCESS/TRADE_FINISHED/WAIT_SELLER_SEND_GOODS，out_trade_no 绑定意图，total_fee 精确转分，trade_no 唯一。返回 success/fail。
- respond.php 用 code 白名单分发；不允许请求拼出文件路径或执行插件。拒绝重复 form key，防止签名解析歧义，限制 body 大小。密钥由环境提供，不落数据库或日志；未配置即禁用。
- receive.php GET/POST 要求 Bearer 认证，参数 id 归属当前用户，调用 shipped -> received 状态机；不保留仅凭收货人姓名即可修改订单的旧权限缺陷。
- PayPal IPN、银联、财付通等需要远端验证或不同签名的插件登记为后续适配，默认拒绝；不会调用真实支付机构。本轮只验收上述明确的协议，不能宣称所有历史插件上线可用。

MD5 仅用于源协议兼容，密码和现代回调继续使用 PBKDF2/HMAC。CI 用 Python hashlib 生成独立签名向量，覆盖成功、篡改、错误商户、错误币种、失败状态、过期/失效意图、同流水重放与跨订单重用；禁止以“HTTP 200”代替业务落账验证。

## CI 与交付

保留三平台 SQLite 构建、Clippy、单测、HTTP 回归；增加 Ubuntu/MySQL 8 job，使用同一 Python 验收脚本和 MySQL connector seed，另跑促销及旧支付场景。产物保存服务日志、逐请求结果；报告区分路由探测、业务成功、并发及真实第三方联调。本轮不进行真实渠道联调。每次修复后推送两个远程，最后以 HEAD 对应 Actions completed/success 为准。
