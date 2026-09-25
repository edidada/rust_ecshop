# HTTP API 契约

Base URL：`http://localhost:8080/api/v1`；业务 API 使用 JSON，`Content-Type: application/json; charset=utf-8`，`/sitemap.xml` 等文档型端点会明确返回其他媒体类型。成功响应直接返回资源；错误固定为：

```json
{"code":"validation_error","message":"quantity must be between 1 and 999","request_id":"...","details":{"field":"quantity"}}
```

| HTTP | `code` | 使用场景 |
| --- | --- | --- |
| 400 | `validation_error` | JSON、类型、范围或业务前置条件错误 |
| 401 | `unauthenticated` | 无效、过期或缺失会话 |
| 403 | `forbidden` | 已登录但无资源权限 |
| 404 | `not_found` | 商品、订单等不存在或不可见 |
| 409 | `conflict` / `out_of_stock` | 用户重复、版本冲突、库存不足 |
| 422 | `invalid_state` | 订单状态不允许该动作 |
| 500 | `internal_error` | 不泄露 SQL/堆栈；记录 request_id |

鉴权：当前实现使用 `Authorization: Bearer <access_token>`。令牌只在登录/注册响应中明文返回一次，服务端只存 SHA-256 哈希。订单等后续接口会再增加幂等键。

## 商品

`GET /catalog` 映射 PHP `catalog.php`，返回全部可见商品分类和品牌。分类按父级、排序值及 ID 排序，并包含仅统计上架、未删除、允许单独销售商品的 `goods_count`；`parent_id` 可供客户端自行构建分类树。

`GET /goods-widget.js?cat_id=1&brand_id=1&goods_num=10&intro_type=is_new` 映射 PHP `goods_script.php` 的公开商品挂件，返回 `application/javascript` 并设置 `window.cppEcshopGoodsWidget`。`intro_type` 支持 `is_best`、`is_new`、`is_hot`、`is_promote`、`is_random`，数量上限 50。旧 PHP 的 `type=collection&u=任意用户ID` 会泄露他人收藏，因此不兼容；登录用户应使用 `/api/v1/me/favorites`。

`GET /quotation?category_id=1&brand_id=1&q=C%2B%2B&page=1&page_size=20` 映射 PHP `quotation.php?act=print_quotation`，返回商品及 SKU 报价行。每行保留商品/SKU 编号、分类、基础价格、实际库存和属性 ID；前端若需属性加价，应把 `product_id` 与 `attribute_ids` 提交给 `/goods/{id}/price-quote`，不能自行拼接价格。

`GET /sitemap.xml` 映射 PHP `sitemaps.php`，返回标准 XML sitemap，收录首页、可见分类、文章分类、最多 300 个公开商品及公开文章。绝对地址的根 URL 来自 `site.base_url` 配置，不信任请求的 `Host` 头。

`GET /feed.xml` 映射 PHP `feed.php`，返回 RSS 2.0。默认返回最新公开商品，可用 `cat`（含子分类）和 `brand` 筛选；`type` 支持 `group_buy`、`snatch`、`auction`、`exchange`、`activity`、`package` 及 `article_cat{id}`。Feed 最多返回 100 项，链接根地址同样使用 `site.base_url`。

`GET /goods/{id}` 返回商品、图、规格、库存摘要；不存在为 404。

`POST /goods/{id}/price-quote`

```json
{"quantity":2,"product_id":81,"attribute_ids":[1001,1005]}
```

```json
{"goods_id":12,"quantity":2,"unit_price":"49.90","total":"99.80","currency":"CNY","stock_available":18}
```

该接口对应 PHP `goods.php?act=price`。价格只用于展示；下单时必须重新计算，客户端总价不可被信任。

`GET /compare?goods_ids=12,14` 映射 PHP `compare.php?goods[]=...` 的公开对比读取。`goods_ids` 必须是两个到四个、无重复的正整数，按输入顺序返回可售商品的基本字段；任一商品不存在、删除或下架时返回 404，避免在对比结果中泄露不可见商品。

## 账号与地址

`POST /auth/register`

```json
{"username":"alice","email":"alice@example.test","password":"not-a-real-password","agreement_accepted":true}
```

返回 `201` 和用户 ID、用户名及 Bearer access token。密码长度、唯一性和邮箱格式由服务端验证，数据库仅存带随机盐的 PBKDF2-SHA256 哈希。

`POST /auth/availability/username` 映射 PHP `user.php?act=is_registered`，请求 `{"username":"alice"}`，返回 `{"available":true|false}`。该结果只用于界面提示，注册仍依赖数据库唯一约束作最终竞态保护。

`POST /auth/availability/email` 映射 PHP `user.php?act=check_email`，请求 `{"email":"alice@example.test"}`，返回邮箱是否可用。这同样只是提示性查询，注册写入仍由数据库唯一约束防止 TOCTOU。

`POST /auth/login` 请求 `{ "username":"alice", "password":"..." }`，成功返回用户 ID、用户名和 Bearer access token。`POST /auth/logout` 返回 204 并作废当前会话。

`PATCH /me/password` 映射 PHP `user.php?act=act_edit_password`，请求：

```json
{"current_password":"old-password","new_password":"new-password"}
```

新密码为 8--1024 字节。服务端先验证当前 Bearer 用户和原密码，再通过“用户 ID + 读取到的旧哈希”条件原子更新密码，避免校验与写入之间的 TOCTOU。SQLite 与 MySQL 都在同一事务内撤销该用户的全部 session；成功返回 204，客户端必须用新密码重新登录。原密码错误或并发修改返回 409，无效 session 返回 401。

`POST /me/bonuses/claim` 映射 PHP `user.php?act=act_add_bonus`，请求 `{"bonus_sn":"202609230001"}`。序列号必须作为 1--20 位数字字符串传入，避免 JSON 大整数精度丢失。仓储在事务内锁定 `user_bonus`，检查关联 `bonus_type.use_end_date`，再以 `user_id=0` 为条件领取；成功返回 204，不存在返回 404，过期或已被领取返回 409。SQLite 开发数据提供一次性示例序列号 `202609230001`。

`GET /me/bonuses` 映射 PHP `user.php?act=bonus`，只返回当前 Bearer 用户领取的红包。金额保留为十进制字符串，状态为 `not_started`、`available`、`expired` 或 `used`；`used` 项同时返回关联订单 ID。状态由数据库 UTC 时间和 `order_id` 计算，不接收客户端时间。

`POST /me/email-verifications` 映射 PHP `user.php?act=send_hash_mail`，要求 Bearer token，成功返回 `202 {"status":"queued"}`。服务端使用密码学随机数生成一次性 token，只把 SHA-256 哈希写入 `email_verification_tokens`；明文 token 仅作为 `verify_email` 模板 payload 写入 `email_outbox`，供独立邮件发送器消费，不会进入 HTTP 响应或日志。重复申请会原子替换旧 token，token 24 小时过期。

`POST /email-verifications/confirm` 映射 PHP `user.php?act=validate_email`，请求 `{"token":"64位小写十六进制token"}`。仓储在事务中锁定未消费且未过期的 token，写入永久的 `email_verified_users` 状态后以同一 token 哈希条件消费 token；成功返回 204。无效、过期、已消费或并发重复请求统一返回 409，不泄露用户信息。

`POST /password-resets` 映射 PHP `user.php?act=get_password`、`send_pwd_email`，请求 `{"email":"alice@example.test"}`，对存在与不存在的邮箱一律返回 `202 {"status":"accepted"}`，防止账号枚举。存在用户时，服务端在事务中生成一小时有效的随机 token，数据库只保存 SHA-256 哈希，明文仅进入 `password_reset` 邮件 outbox。旧版安全问题找回容易被猜测，不再单独实现，统一走此流程。

`POST /password-resets/confirm` 映射 PHP `user.php?act=reset_password`，请求 `{"token":"64位小写十六进制token","new_password":"..."}`。新密码限制为 8--1024 字节。仓储锁定未消费且未过期的 token，在同一事务内更新 PBKDF2-SHA256 密码哈希、撤销该用户全部 session 并消费 token；成功返回 204，客户端需重新登录。无效、过期、已消费或并发重复 token 返回 409。

`POST /me/account/requests` 映射 PHP `user.php?act=act_account`，充值请求为 `{"kind":"deposit","amount":"100.00","payment_id":1,"note":"..."}`，提现请求为 `{"kind":"withdrawal","amount":"50.00","note":"..."}`。金额只接受十进制字符串并在 C++ 转换为整数分，避免浮点误差。充值必须选择启用的支付方式，初始状态为 `pending_payment`；提现在事务中用条件更新把可用余额转为冻结余额，余额不足或并发超额申请返回 409，初始状态为 `pending_review`。

`GET /me/account/requests` 映射 PHP `user.php?act=account_log`，只按 Bearer 用户 ID 返回充值/提现申请，并同时返回 `available_balance` 与 `frozen_balance`。金额由数据库整数分格式化为两位十进制字符串；支付方式、状态、创建时间和可选支付时间均来自服务端数据。

`DELETE /me/account/requests/{id}` 映射 PHP `user.php?act=cancel`，只允许申请所有者取消尚未处理的 `pending_payment` 充值或 `pending_review` 提现。取消提现时在同一事务中把冻结金额原子退回可用余额再删除申请；其他用户、已处理或不存在的申请均返回 404。成功返回 204。

`GET /me/account/transactions` 映射 PHP `user.php?act=account_detail`，返回当前用户整数分账本及可用/冻结余额。每条流水包含可用余额变化、冻结余额变化、原因和业务引用；正负金额统一格式化为带符号的两位十进制字符串。该账本作为后续充值到账、提现结算和积分转换的审计来源。

`POST /me/account/requests/{id}/payment` 映射 PHP `user.php?act=pay`，请求为 `{"payment_id":1}`。接口只允许申请所有者为 `pending_payment` 状态的充值申请选择启用的支付方式；提现、已处理申请以及越权 ID 均不会生成支付意图。仓储在同一事务中锁定申请、更新支付方式并按申请 ID 幂等写入 `account_payment_intent`。响应中的 `amount`、`fee`、`total` 均由整数分格式化，重复请求更新同一个支付意图，不会重复创建支付记录。

`POST /me/addresses` 请求：

```json
{"consignee":"张三","country_id":1,"province_id":2,"city_id":52,"district_id":500,"address":"中山路 1 号","mobile":"13800000000","zip":"200000","is_default":true}
```

返回 201。`country_id` 到 `district_id` 必须存在于 `region`，并属于合法层级。

`DELETE /me/addresses/{id}` 映射 `user.php?act=drop_consignee`，删除条件同时包含地址 ID 与 Bearer 用户 ID；不存在或属于其他用户均返回 404，成功返回 204。

## 商品收藏

`GET /me/favorites` 返回当前 Bearer 用户的收藏列表。`POST /me/favorites` 请求 `{ "goods_id": 12 }`，只可收藏上架、未删除商品；同一用户重复收藏返回 409。`PATCH /me/favorites/{id}` 请求 `{ "attention": true }` 或 `false`，更新关注标记；`DELETE /me/favorites/{id}` 返回 204。更新和删除都在 SQL 条件中同时匹配收藏 ID 与当前用户 ID，因此其他用户的收藏一律返回 404，不能跨用户修改。

## 留言板

`GET /messages` 返回与 PHP `message.php` 一致的前台、已审核留言（`feedback.msg_area=1` 且 `msg_status=1`）。`POST /messages` 可匿名提交：

```json
{"username":"访客","email":"visitor@example.test","type":0,"title":"咨询","content":"请问何时发货？"}
```

`content` 必填且限制为 1--2000 字节，用户名、邮箱和标题分别最长 60、60、200 字节。可选 Bearer token 会把 `user_id` 绑定到当前 session，并默认采用登录名；传入 `anonymous:true` 则展示名为 `anonymous`。开发 SQL 默认立即发布；生产环境可把新行的 `msg_status` 改为 0 并经后台审核后公开。

`DELETE /me/messages/{id}` 映射 PHP `user.php?act=del_msg`，要求 Bearer token。只有留言所有者可删除，并在同一事务中删除该留言的回复；不存在或属于其他用户时返回 404。

`GET /me/messages` 映射 PHP `user.php?act=message_list`，只返回当前 Bearer 用户的顶层留言，包含关联订单 ID 和第一条回复。

## 广告

`GET /cycle-image.xml` 映射 PHP `cycle_image.php`，从广告表读取当前有效的图片广告（`media_type=0`），按广告位与 ID 输出 `<bcaster><item .../></bcaster>` XML。这样 SQLite/MySQL 使用相同数据模型，不再依赖单机 `data/cycle_image.xml` 缓存文件。

`GET /ads/{id}` 查询活动时间窗内、已启用的 ECSHOP `ad` 记录。`POST /ads/{id}/click` 请求 `{ "referer": "partner.example" }` 会在单个数据库事务中增加 `ad.click_count`，并将来源计数 upsert 到 `adsense`，返回 `{ "redirect_url": "..." }` 供前端自行执行导航。它刻意不提供 PHP `affiche.php?act=js` 的字符串拼接脚本输出，以免将管理端广告代码直接作为 API 可执行响应；前端应按 `media_type` 安全地呈现返回资料。

## 商品图库

`GET /goods/{id}/gallery` 映射 PHP `gallery.php?id=`，返回商品名称与按 `img_id` 排序的 `goods_gallery` 图片。每行包含缩略图、展示图、原图和描述；商品不存在/已删除或尚未上传图库时均返回 404，客户端可退回商品详情页。

## 购物车

`GET /shipping-options?country_id=1&province_id=2&city_id=52&district_id=500` 映射公开的 PHP `myship.php` 配送演示页，不要求登录。它返回当前选择、四级地区候选和已启用配送方式，并逐级校验省、市、区的父子归属。当前简化 SQL 的 `shipping` 表是全局费率，尚未引入原项目的 `shipping_area` 区域费率表，因此不同合法地区暂时返回同一组配送方式。

所有购物车接口均要求 Bearer token，且用户 ID 仅从该 token 对应的 session 取得。

`POST /me/cart`：

```json
{"goods_id":12,"quantity":2}
```

返回 201 的购物车行：

```json
{"id":301,"goods_id":12,"name":"示例商品","price":"49.90","quantity":2,"version":1}
```

`GET /me/cart` 返回 `{ "items": [...], "total_quantity": 2 }`。更新 `PATCH /me/cart/{id}` 请求 `{ "quantity": 3, "version": 1 }`。`version` 是乐观锁版本：SQL 的 `WHERE` 同时匹配 `id`、当前用户与 `version`，失配返回 409，客户端重新 GET `/me/cart`。删除为 `DELETE /me/cart/{id}`，成功 204。

当前购物车只实现无 SKU/属性组合的基础商品行；同一用户同一商品的 POST 会原子累加数量并递增版本。SQLite 使用 `UNIQUE(user_id, goods_id)` + upsert，MySQL 使用对应唯一键 + `ON DUPLICATE KEY UPDATE`，两者都在事务中取得行 ID。

## 结算与订单

`POST /checkout/quote`：

```json
{"address_id":9,"shipping_id":1,"payment_id":1}
```

先用 `GET /checkout/options` 读取可用 `shipping` 与 `payment`。报价验证地址属于当前 Bearer 用户、配送/支付方式启用且购物车有可售商品；响应以当前商品价格计算：

```json
{"address_id":9,"shipping_id":1,"payment_id":1,"items":[...],"goods_amount":"99.80","shipping_fee":"8.00","payment_fee":"0.00","order_amount":"107.80"}
```

金额以分的整数在服务端计算，JSON 中以两位小数的字符串输出。订单创建尚会再次以数据库当前数据重算，不信任客户端报价。

`POST /orders` 必带 `Idempotency-Key`：

```json
{"address_id":9,"shipping_id":1,"payment_id":1,"remark":"工作日送达"}
```

返回 201：

```json
{"id":9001,"order_sn":"EC...","status":"pending_payment","goods_amount":"99.80","shipping_fee":"8.00","payment_fee":"0.00","order_amount":"107.80","replayed":false}
```

服务端在同一个数据库事务内重新读取购物车、条件式扣减库存、写入订单与订单商品快照并清空购物车。库存不足时全部回滚并返回 409。相同用户、相同幂等键和相同请求重放同一结果（200 且 `replayed:true`）；同键不同内容返回 409。支付回调只接受支付渠道签名；不得用浏览器重定向作为支付成功依据。

`GET /me/orders` 返回当前用户的订单摘要列表。`GET /me/orders/{id}` 返回订单收货信息和 `order_goods` 快照；两个查询都以已认证用户 ID 作为 SQL 条件，其他用户的订单与不存在订单一律返回 404。

`POST /me/orders/{id}/cancel` 仅能取消当前用户仍为 `pending_payment` 的订单。成功后返回状态为 `cancelled` 的订单摘要；同一事务会回补 `order_goods` 数量到库存、退回此前用于该订单的账户余额并插入订单及账户审计。已取消、已付款或其他非待支付状态返回 409，因状态条件更新失败而不会重复回补库存或余额。

`POST /me/orders/{id}/received` 映射 `user.php?act=affirm_received`，仅允许订单所有者把 `paid` 订单原子更新为 `received`，并在同一事务写入客户确认收货审计。重复确认或非已支付状态返回 409，其他用户的订单返回 404。

`POST /me/orders/{id}/cart` 映射 `user.php?act=return_to_cart`，在单一事务内校验订单归属，将订单商品按当前可售库存上限合并回当前用户购物车。订单不存在或不属于当前用户返回 404；没有任何可售且有库存的商品返回 409。

`POST /me/orders/merge` 映射 `user.php?act=merge_order`，请求体为 `{"from_order_id":101,"to_order_id":102}`。两个订单必须不同、均属于当前用户、均为 `pending_payment` 且尚未使用账户余额。服务端在单一事务内锁定原订单，新建合并订单、转移商品快照、清理原支付/审计记录并写入合并审计。成功返回 201 和新订单摘要。

`GET /me/orders/by-number/{order_sn}/status` 映射 `user.php?act=order_query`，使用 Bearer token 代替旧版可枚举订单号的公开查询。SQL 同时匹配订单号和当前用户 ID，返回订单状态与金额摘要。

`PATCH /me/orders/{id}/surplus` 映射 PHP `user.php?act=act_edit_surplus`，请求为 `{"amount":"10.00"}`。金额只接受十进制字符串并转换为整数分；超过不含支付手续费的剩余应付款时自动截断。仓储在一个事务中锁定当前用户订单、条件扣减 `account_balance`、重新计算定额支付手续费、累计 `order_balance_payment.paid_cents`，并写入 `account_log` 与 `order_action`。余额不足、订单已支付或并发状态变化返回 409；若账户余额付清订单，状态原子变为 `paid`。取消部分余额支付的待支付订单会在同一事务中退款；已使用余额的订单在取消或结算前禁止合并，避免资金归属丢失。独立余额支付表让已有 SQLite 数据库可以通过 `CREATE TABLE IF NOT EXISTS` 非破坏性升级，无需重建订单表。

`PATCH /me/orders/{id}/payment` 映射 PHP `user.php?act=act_edit_payment`，请求为 `{"payment_id":2}`。只允许订单所有者为尚未支付、尚未发货且仍为 `pending_payment` 的订单选择另一个启用的支付方式。事务锁定订单，从当前 `order_amount` 扣除旧手续费得到剩余商品应付款，再按 SQL 中新支付方式的定额 `pay_fee` 重算 `payment_fee` 与 `order_amount`，最后写入订单审计。支付方式未变化、订单状态并发变化返回 409；不存在或停用的支付方式返回 404。此前使用的账户余额已经体现在剩余应付款中，切换支付方式不会重复加回余额。

`PATCH /me/orders/{id}/address` 映射 PHP `user.php?act=save_order_address`，请求字段为 `consignee`、`email`、`address`、`zipcode`、`tel`、`mobile`、`sign_building` 和 `best_time`。`consignee`、合法格式的 `email` 与 `address` 必填，其余字段允许空字符串。只允许订单所有者修改仍待支付且未发货的订单；事务锁定订单，在 `order_info` 更新核心收货信息，在一对一 `order_delivery_address` 保存旧版完整字段，并写入订单审计。订单不存在或不属于当前用户返回 404，状态已变化返回 409。独立详情表允许现有 SQLite 数据库通过初始化 SQL 非破坏性补表。

`GET /me/group-buys` 与 `GET /me/group-buys/{id}` 实现旧 PHP 中仍为占位代码的 `group_buy` / `group_buy_detail`。接口只返回 Bearer token 当前用户通过 `order_promotion` 关联的 `act_type=1` 团购活动和订单金额快照，不会把购买了同一普通商品的订单误判为团购。详情路径中的 `id` 是团购活动 ID；无关联订单或其他用户的记录统一返回 404。活动结束后仍保留在用户历史中，因此不会套用公开活动接口的当前时间窗过滤。

`GET /me/affiliate?page=1&page_size=20` 映射 PHP `user.php?act=affiliate` 的“我的推荐”分支。响应的 `levels` 用递归 CTE 统计最多 10 层推荐用户，`orders` 合并下级用户订单和当前用户已有的 `affiliate_log` 分成记录；订单号按旧页面规则脱敏。`separated=false` 表示仍未生成当前用户的分成记录，已生成记录会返回佣金、积分及 `separate_type`（负值代表撤销）。推荐关系独立存放于 `user_referral`，不会为了兼容旧字段而破坏已有 `users` 表；`(order_id,user_id)` 唯一约束避免同一订单对同一推荐人重复分成。分页每页最多 100 条。

旧 `user.php?act=email_list` 的四个 `job` 拆为：`POST /newsletter-subscriptions` 请求订阅、`DELETE /newsletter-subscriptions` 请求退订、`POST /newsletter-subscriptions/confirm` 确认订阅、`POST /newsletter-unsubscriptions/confirm` 确认退订。请求体分别使用 `{"email":"alice@example.com"}` 和 `{"token":"64位十六进制随机令牌"}`。请求动作统一返回 202，避免通过响应枚举邮箱状态；服务端只保存 SHA-256 token 哈希，原始 token 进入事务性 `email_outbox`，有效期 24 小时。确认动作是带状态、动作、有效期条件的单条原子更新/删除，令牌只能使用一次。已订阅地址重复订阅、未订阅地址请求退订均幂等接受但不产生重复邮件。

## 商品评论

`GET /goods/{id}/comments` 返回已发布评论列表。`POST /goods/{id}/comments` 要求 Bearer token：

```json
{"content":"商品符合预期"}
```

内容长度为 1--2000 个字节；服务端从 session 获取评论用户名并使用绑定参数写库。当前开发配置自动发布评论（`status=1`）；生产审核模式可改为先写未发布状态。下架或不存在商品返回 404。

`DELETE /me/comments/{id}` 映射 PHP `user.php?act=del_cmt`，删除 SQL 同时匹配评论 ID 和 Bearer token 对应的用户 ID。成功返回 204，不存在或属于其他用户时返回 404。

`GET /me/comments` 映射 PHP `user.php?act=comment_list`，返回当前用户对商品或文章发表的顶层评论，并附带目标名称及第一条回复。

## 商品标签

`GET /tags` 返回公开标签云，按使用次数降序、标签文字升序排列，只统计仍上架且未删除的商品标签。

`GET /me/tags` 映射 PHP `user.php?act=tag_list`，要求 Bearer token，只按标签文字聚合当前用户的标签及使用次数。

`POST /goods/{id}/tags` 映射 PHP `user.php?act=add_tag`，要求 Bearer token：

```json
{"tag":"C++,商城"}
```

`tag` 是以英文逗号分隔的 1--10 个非空标签，每个标签最长 255 字节。服务端从 token 获取用户 ID，只允许对上架、未删除商品写入；同一用户对同一商品的相同标签会被忽略。成功返回 201 和该商品当前的标签统计：

`DELETE /me/tags` 映射 PHP `user.php?act=act_del_tag`，请求体为 `{"tag":"C++"}`。服务端只删除 Bearer token 当前用户的同名标签，不会删除其他用户或公共标签；即使标签不存在也幂等返回 204。

## 缺货登记

`GET /me/bookings` 映射 PHP `user.php?act=booking_list`，要求 Bearer token，按登记时间倒序返回当前用户的缺货登记。返回商品 ID、商品名、需求数量、登记时间和处理结果，不会暴露其他用户数据。

`POST /me/bookings` 映射 PHP `user.php?act=act_add_booking`，请求例子：

```json
{"goods_id":12,"goods_number":2,"description":"蓝色","linkman":"Alice","email":"alice@example.com","telephone":"13800000000"}
```

只允许登记上架且未删除的商品；同一用户对同一商品只能有一条登记，重复请求返回 409。去重依赖 SQLite/MySQL 唯一索引和原子插入，避免先查后写的 TOCTOU 竞态。

`DELETE /me/bookings/{id}` 映射 PHP `user.php?act=act_del_booking`，删除条件同时包含登记 ID 和 token 对应用户 ID。成功返回 204，不存在或属于其他用户时返回 404。

```json
{"items":[{"word":"C++","count":2},{"word":"商城","count":1}]}
```

缺失或无效 token 返回 401；格式错误或标签数量/长度越界返回 400。SQLite 与 MySQL 都使用单条带可售商品 `EXISTS` 条件的参数化 INSERT，并用已认证用户、商品和标签的数据库唯一约束去重，避免先读后写的 TOCTOU 竞争；匿名旧数据不受该去重约束影响。

## 支付回调

`POST /payments/{provider}/callback` 不使用用户 Bearer token。渠道适配器必须提交原始 JSON body，并在 `X-Payment-Signature` 携带该原始 body 的 HMAC-SHA256 十六进制签名。当前统一 payload：

```json
{"provider_trade_no":"gateway-transaction-1","order_sn":"EC...","amount":"107.80"}
```

服务器先验签，再检查金额与当前订单应付金额相等，最后在一个事务中将 `pending_payment` 订单变为 `paid`、插入 `pay_log` 和审计行。`(provider, provider_trade_no)` 是唯一键：同一交易、相同金额/订单号重放返回 200，冲突交易号返回 409。生产配置必须用环境变量提供 `payment.callback_hmac_secret`，不得使用开发 secret。

## 促销活动

`GET /promotions` 返回有效的 ECSHOP `goods_activity` 行；可选 `type=0..4` 对应夺宝、团购、拍卖、积分兑换、礼包。`GET /promotions/{id}` 返回一个活动。两个接口仅显示尚未结束、处于时间窗口内且关联商品仍可见的活动；`ext_info` 保持原始 JSON/扩展内容，供后续团购阶梯价、拍卖出价等专用接口解释。

`GET /activities` 映射 PHP `activity.php`，按 `sort_order`、结束时间返回 `favourable_activity` 优惠规则。响应保留 `user_rank`、`range`/`range_ext`、金额门槛、优惠类型/数值和 `gift` 原始内容，方便客户端展示并让后续结算逻辑复用同一份规则。金额仍以十进制字符串返回。

`GET /packages` 映射 PHP `package.php`，返回当前时间窗内、未结束的 `act_type=4` 礼包及其可售商品。每个礼包含 `package_price`、按商品现价和数量计算的 `subtotal`、非负 `saving` 与商品明细；三个金额均为两位小数字符串。新项目将 `goods_activity.ext_info` 保存为 JSON，并从 `package_price` 字段读取礼包价。

`GET /topics/{id}` 映射 PHP `topic.php?topic_id=`，仅返回当前时间窗内的专题。专题元数据包含标题、简介、样式和 SEO 字段；`groups` 按 `topic.data` 的 JSON 对象顺序组织商品，并再次通过可售商品仓储过滤下架/删除商品。C++ 新库用 JSON 表示专题分组，例如 `{"入门":[12],"进阶":[14]}`。

`GET /votes/current` 返回当前调查及选项计数。`POST /votes/{id}/responses` 请求 `{"option_ids":[1]}`；单选调查只接受一个选项，多选最多 20 个且不得重复。客户端 IP 直接取 socket 远端地址，不信任转发请求头；SQLite/MySQL 都在事务内验证活动和选项、写入唯一 `(vote_id,ip_address)` 日志，再增加主题及选项计数。同一地址重复提交返回 409。

`GET /exchange-goods` 映射 PHP `exchange.php?act=list`，支持 `category_id`（含子分类）、`integral_min`、`integral_max`、`page`、`page_size`、`sort=goods_id|exchange_integral|last_update` 和 `order=asc|desc`。仅返回启用兑换且公开可售的商品。`GET /exchange-goods/{id}` 映射详情页；未启用、下架、删除或不存在均返回 404。
