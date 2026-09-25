# PHP URL 覆盖矩阵

本文件是“实现所有 URL”目标的审计清单。参考源固定为
`/Users/ibqo/Develop/git/github/php/ecshop/upload`；完成状态必须同时满足：Crow 路由存在、
应用服务存在、SQLite/MySQL 仓储均存在、SQL 结构存在、契约与无代理 curl 示例存在。
仅在 `docs/02_url_api_list.md` 出现不算完成。

状态：`DONE` 已有两套数据库实现；`PARTIAL` 只覆盖旧入口的一部分语义；`TODO` 尚未实现。
页面渲染动作（例如登录页本身）由前端消费 JSON API，不单独复制 PHP 模板输出。

## 顶层 PHP 入口

| PHP 入口 | 状态 | C++ URL / 后续工作 |
| --- | --- | --- |
| `index.php` | DONE | `GET /api/v1/home` |
| `catalog.php` | DONE | `GET /api/v1/catalog` |
| `category.php` | DONE | `GET /api/v1/categories/{id}/goods` |
| `goods.php` | DONE | 商品详情、价格试算、预订 |
| `search.php` | DONE | `GET /api/v1/goods` |
| `brand.php` | DONE | 品牌列表与品牌商品 |
| `article.php` / `article_cat.php` | DONE | 文章详情与分类文章 |
| `region.php` | DONE | 地区查询 |
| `compare.php` | DONE | 商品对比 |
| `activity.php` | DONE | 优惠活动列表 |
| `package.php` | DONE | 礼包列表 |
| `topic.php` | DONE | 专题详情 |
| `vote.php` | DONE | 调查读取与提交 |
| `exchange.php` | DONE | 积分商品列表与详情；兑换写操作仍归 TODO |
| `message.php` | DONE | 留言列表与新增 |
| `comment.php` | DONE | 商品评论列表与新增 |
| `tag_cloud.php` | DONE | 标签云 |
| `gallery.php` | DONE | 商品图库 |
| `affiche.php` | DONE | 广告读取与点击统计 |
| `cycle_image.php` | DONE | 轮播 XML |
| `goods_script.php` | DONE | JavaScript 商品挂件 |
| `quotation.php` | DONE | 报价单 |
| `myship.php` | DONE | 地区和配送方式 |
| `sitemaps.php` | DONE | XML sitemap |
| `feed.php` | DONE | RSS feed |
| `respond.php` | DONE | 统一支付回调 |
| `receive.php` / `chinabank_receive.php` | PARTIAL | 统一回调已存在；需补旧网关字段适配 |
| `group_buy.php` / `auction.php` / `snatch.php` | PARTIAL | 活动读取已统一为 promotions；出价、抢购和团购写操作未完成 |
| `flow.php` | PARTIAL | 核心购物车、结算试算、下单已完成，细分步骤见下表 |
| `user.php` | PARTIAL | 核心账号、地址、订单等已完成，细分 action 见下表 |
| `affiliate.php` | TODO | 推荐关系与分成记录 |
| `api.php` | TODO | 旧商品搜索兼容入口，需定义受限 REST 兼容层 |
| `captcha.php` | TODO | 跨平台验证码 challenge/verify |
| `certi.php` | TODO | 商店证书/授权协议兼容层；不得复制不安全远程执行行为 |
| `pick_out.php` | TODO | 选购中心筛选接口 |
| `pm.php` | TODO | 站内信列表、读取、发送、删除 |
| `wholesale.php` | TODO | 批发商品、报价与下单 |

## `user.php` action

已覆盖：

- 注册/会话：`act_register`、`is_registered`、`check_email`、`act_login`、`signin`、`logout`。
- 资料：`profile`、`act_edit_profile`、`act_edit_password`。
- 地址：`address_list`、`act_edit_address`、`drop_consignee`。
- 收藏/标签：`collection_list`、`delete_collection`、`add_to_attention`、`del_attention`、
  `collect`、`add_tag`、`tag_list`、`act_del_tag`。
- 缺货登记：`booking_list`、`act_add_booking`、`act_del_booking`；`add_booking` 页面由前端表单替代。
- 留言/评论：`message_list`、`act_add_message`、`del_msg`、`comment_list`、`del_cmt`。
- 订单：`order_list`、`order_detail`、`cancel_order`、`affirm_received`、`merge_order`、
  `return_to_cart`、`order_query`、`act_edit_surplus`（整数分余额扣款、流水、退款与并发保护）、
  `act_edit_payment`（锁定待支付订单并重算定额手续费）、`save_order_address`（完整收货字段与审计）。
- 团购订单：`group_buy`、`group_buy_detail`（当前用户订单与活动显式关联）。
- 推荐中心：`affiliate`（推荐层级、关联订单、分成与撤销记录）。
- 邮件订阅：`email_list` 的订阅、退订及两种一次性 token 确认流程。
- 红包：`act_add_bonus`、`bonus`。
- 邮箱验证：`send_hash_mail`、`validate_email`（随机 token、outbox、事务消费与永久验证状态）。
- 密码找回申请：`get_password`、`send_pwd_email`；安全问题动作 `qpassword_name`、
  `get_passwd_question`、`check_answer` 安全地统一映射到相同邮件 token 流程。
- 密码重置确认：`reset_password`（事务修改密码、撤销会话并消费一次性 token）。
- 余额申请：`account_raply`、`account_deposit` 页面由客户端替代，`act_account` 统一映射为
  充值/提现申请；提现会原子冻结可用余额。`account_log` 返回余额和申请记录。
  `cancel` 取消未处理申请，并事务解冻提现金额。
  `account_detail` 返回整数分余额流水。`pay` 锁定充值申请并幂等准备支付意图。

待实现：

| action | 计划 REST API | 主要表 |
| --- | --- | --- |
| `track_packages` | `GET /api/v1/me/shipments` | order_info, shipping |
| `transform_points` | `GET /api/v1/me/points/conversion-options` | users, account_log |
| `act_transform_points` / `act_transform_ucenter_points` | `POST /api/v1/me/points/conversions` | users, account_log |
| `clear_history` | `DELETE /api/v1/me/browsing-history` | browsing_history |

`default`、`register`、`login` 是 HTML 页面动作，由客户端页面替代，不新增重复 JSON URL。

## `flow.php?step=`

已覆盖核心步骤：`cart`、`add_to_cart`、`checkout`、`done`、`update_cart`、`drop_goods`。
`login`、`consignee`、`drop_consignee` 分别复用认证与地址 API。

待覆盖步骤：

- 购物车动作：`link_buy`、`clear`、`drop_to_collect`、`add_package_to_cart`、`add_favourable`。
- 结算选择：`select_shipping`、`select_insure`、`select_payment`、`select_pack`、`select_card`。
- 结算抵扣：`change_surplus`、`change_integral`、`change_bonus`、`validate_bonus`。
- 发票/缺货策略：`change_needinv`、`change_oos`。
- 余额与积分校验：`check_surplus`、`check_integral`。

## 完成门槛

1. 每个 `TODO/PARTIAL` 项变成具体 Crow 路由，不能只写映射文档。
2. 每个持久化 URL 同时实现 SQLite 与 MySQL，并使用相同业务状态机。
3. 写操作必须有事务、归属过滤、参数化 SQL 和 TOCTOU 防护。
4. 每个 URL 独立 commit；编码阶段只执行 `git diff --check`。
5. 全部 URL 编码完成后统一执行 CMake/Ninja 编译、测试和 `curl --noproxy '*'` 验收。
