# 全部 URL 清单（src/http/routes/mod.rs 路由表）

清单与 `cargo build` 后的服务端注册一一对应。「证据」列是 `scripts/curl_api_test.sh`
在本地/CI 运行时写出的响应快照（`responses/` 目录，编号与脚本顺序一致）。

## 公开：健康检查与商品目录

| 方法 | URL | 作用 | 证据 |
|------|-----|------|------|
| GET | /healthz | 存活检查 | 00-healthz |
| GET | /api/v1/home | 首页聚合 | 01-home |
| GET | /api/v1/catalog | 商品分类树 | 02-catalog |
| GET | /api/v1/goods | 商品列表（分页/筛选） | 03-goods-list |
| GET | /api/v1/goods/:id | 商品详情 | 04-goods-detail |
| POST | /api/v1/goods/:id/price-quote | 单品/规格报价 | 05-goods-price-quote |
| GET | /api/v1/goods/:id/gallery | 商品相册 | 06-goods-gallery |
| GET | /api/v1/brands | 品牌列表 | 07-brands |
| GET | /api/v1/brands/:id/goods | 品牌商品 | 08-brand-goods |
| GET | /api/v1/categories/:id/goods | 分类商品 | 09-category-goods |
| GET | /api/v1/compare | 商品对比 | 10-goods-compare |
| GET | /api/v1/regions | 地区数据 | |
| GET | /api/v1/articles/:id | 文章详情 | |
| GET | /api/v1/article-categories/:id/articles | 分类文章 | |
| GET | /api/v1/shipping-options | 配送方式 | |
| GET | /api/v1/quotation | 报价单（会员区间价格） | |
| GET | /api/v1/activities | 营销活动列表 | |
| GET | /api/v1/packages | 配置套餐 | |
| GET | /api/v1/promotions | 促销列表 | |
| GET | /api/v1/promotions/:id | 促销详情 | |
| GET | /api/v1/topics/:id | 专题 | |
| GET | /api/v1/votes/current | 当前投票 | |
| POST | /api/v1/votes/:id/responses | 投票 | |
| GET | /api/v1/exchange-goods | 积分商品列表 | |
| GET | /api/v1/exchange-goods/:id | 积分商品详情 | |
| POST | /api/v1/exchange-goods/:id/redemptions | 积分兑换下单 | |
| GET/POST | /api/v1/promotions/:id/bids | 竞价出价 | |
| POST | /api/v1/promotions/:id/orders | 团购下单 | |

## 社区：留言/评论/标签

| 方法 | URL | 作用 |
|------|-----|------|
| GET/POST | /api/v1/messages | 站内留言（增/查） |
| GET | /api/v1/me/messages | 我的留言 |
| DELETE | /api/v1/me/messages/:id | 删除留言 |
| GET/POST | /api/v1/goods/:id/comments | 商品评论（查/增） |
| GET | /api/v1/me/comments | 我的评论 |
| DELETE | /api/v1/me/comments/:id | 删除评论 |
| GET | /api/v1/tags | 标签云 |
| GET/DELETE | /api/v1/me/tags | 我的标签（查/删） |
| POST | /api/v1/goods/:id/tags | 给商品打标签 |

## 挂件/SEO 输出

| 方法 | URL | 作用 |
|------|-----|------|
| GET | /api/v1/ads/:id | 广告位 |
| POST | /api/v1/ads/:id/click | 广告点击 |
| GET | /cycle-image.xml | 轮播图 XML |
| GET | /goods-widget.js | 外挂挂件脚本 |
| GET | /sitemap.xml | 站点地图 |
| GET | /feed.xml | RSS Feed |

## 认证与个人资料（账户全生命周期）

| 方法 | URL | 作用 |
|------|-----|------|
| POST | /api/v1/auth/availability/username | 用户名占用查询 |
| POST | /api/v1/auth/availability/email | 邮箱占用查询 |
| POST | /api/v1/auth/register | 注册（创建账户）|
| POST | /api/v1/auth/login | 登录 |
| POST | /api/v1/auth/logout | 登出（会话失效）|
| GET/PATCH | /api/v1/me | 资料（查/改）|
| PATCH | /api/v1/me/password | 修改密码 |
| GET/POST | /api/v1/me/favorites | 我的收藏（查/增）|
| PATCH/DELETE | /api/v1/me/favorites/:id | 收藏改/删 |

## 账户业务（UCenter 汇款/缺货/大额电汇等）

| 方法 | URL | 作用 |
|------|-----|------|
| GET/POST | /api/v1/me/addresses | 收货地址（查/增，ecs_user_address）|
| PATCH/DELETE | /api/v1/me/addresses/:id | 地址改/删 |
| GET/POST | /api/v1/me/bookings | 缺货登记（查/增）|
| DELETE | /api/v1/me/bookings/:id | 删除登记 |
| GET | /api/v1/me/bonuses | 我的红包 |
| POST | /api/v1/me/bonuses/claim | 认领红包 |
| POST | /api/v1/me/email-verifications | 发起邮箱验证 |
| POST | /api/v1/email-verifications/confirm | 验证确认 |
| POST | /api/v1/password-resets | 忘记密码申请 |
| POST | /api/v1/password-resets/confirm | 重置确认 |
| POST | /api/v1/newsletter-subscriptions | 订阅（查/订阅）|
| POST | /api/v1/newsletter-subscriptions/confirm | 订阅确认 |
| POST | /api/v1/newsletter-unsubscriptions/confirm | 退订确认 |
| GET/POST | /api/v1/me/account/requests | 账户资金申请（查/增）|
| POST | /api/v1/me/account/requests/:id/payment | 申请支付 |
| DELETE | /api/v1/me/account/requests/:id | 申请取消 |
| GET | /api/v1/me/account/transactions | 资金流水 |
| GET/POST | /api/v1/me/pms | 站内信（收件箱/发送）|
| DELETE | /api/v1/me/pms/:id | 删除站内信 |

## 购物车与结算

| 方法 | URL | 作用 |
|------|-----|------|
| GET/DELETE | /api/v1/me/cart | 购物车（查/清）|
| POST | /api/v1/me/cart | 加购（增；PATCH/DELETE 在 /:id）|
| PATCH/DELETE | /api/v1/me/cart/:id | 改数量/删除单项 |
| POST | /api/v1/me/cart/linked-buy | 关联购买 |
| POST | /api/v1/me/cart/packages/:id | 选择套餐 |
| POST | /api/v1/me/cart/favourables/:id | 选择优惠活动 |
| POST | /api/v1/me/cart/:id/favorite | 及时收藏 |
| GET | /api/v1/checkout/options | 结算选项 |
| POST | /api/v1/checkout/quote | 结算报价 |
| GET/PATCH | /api/v1/checkout/selection | 配件/红包/积分选择（查）|
| PATCH | /api/v1/checkout/selection/:field | 逐项选择 |
| GET | /api/v1/checkout/funds | 结算资金校验 |
| POST | /api/v1/checkout/bonuses/:id/validation | 红包校验 |

## 订单（查询/合并/改状态机）

| 方法 | URL | 作用 |
|------|-----|------|
| POST | /api/v1/orders | 下单（事务 + 幂等键）|
| GET | /api/v1/me/orders | 订单列表 |
| POST | /api/v1/me/orders/merge | 合并订单 |
| GET | /api/v1/me/orders/by-number/:order_sn/status | 按订单号查状态 |
| GET | /api/v1/me/orders/:id | 订单详情 |
| POST | /api/v1/me/orders/:id/cancel | 取消（库存/红包恢复）|
| POST | /api/v1/me/orders/:id/received | 确认收货 |
| POST | /api/v1/me/orders/:id/cart | 买错货退回购物车 |
| PATCH | /api/v1/me/orders/:id/surplus | 结算余额抵扣金额 |
| PATCH | /api/v1/me/orders/:id/payment | 修改支付方式 |
| PATCH | /api/v1/me/orders/:id/address | 修改收货人信息 |
| GET | /api/v1/me/group-buys | 我的团购 |
| GET | /api/v1/me/group-buys/:id | 团购详情 |

## mock 集成、支付意图、旧支付渠道（respond.php 等三个历史入口）

| 方法 | URL | 作用 |
|------|-----|------|
| POST | /api/v1/mock/orders/:id/pay | mock 支付（回调模拟）|
| POST | /api/v1/mock/orders/:id/ship | mock 发货 |
| POST | /api/v1/mock/mailbox | mock 信箱（邮件对话）|
| POST | /api/v1/mock/deposits/:id/pay | mock 资金支付 |
| POST | /api/v1/me/orders/:id/payment-intents | 支付意图（旧渠道绑定）|
| GET/POST | /respond.php | 旧网关分发器，code 白名单 |
| GET/POST | /receive.php | 确认收货（旧入口，Bearer 认证）|
| POST | /chinabank_receive.php | 网银异步对账（MD5）|
| POST | /api/v1/payments/:provider/callback | 现代 mock HMAC 回调 |

## 兼容层（UCenter/ECShop 历史接口映射）

| 方法 | URL | 作用 |
|------|-----|------|
| GET | /api/v1/me/affiliate | 推广 |
| GET | /api/v1/me/shipments | 物流查询 |
| GET/POST | /api/v1/me/points/conversion-options · /conversions | 积分兑换设置/执行 |
| POST | /api/v1/me/browsing-history | 清浏览记录 |
| GET/POST | /api/v1/me/pms | 个人消息（收件/发件）|
| GET | /api/v1/pick-out | 拣货清单 |
| GET | /api/v1/wholesale-goods | 批发商品 |
| POST | /api/v1/wholesale/quote | 批发报价 |
| GET/POST | /api/v1/captcha · /captcha/verify | 图形验证码 (HMAC)（发/验）|
| GET | /api.php | 旧搜索接口兼容 |
| GET | /api/v1/certi | 证书校验（UCenter）|

## 测试脚本

- `scripts/curl_api_test.sh` —— 本文件所有 URL 的业务旅程回归（Linux/Git-Bash 通用），产出 `responses/`。
- `scripts/http_acceptance.py` —— CI 的 python 验收（促销/旧支付矩阵 + 路由探测）。
