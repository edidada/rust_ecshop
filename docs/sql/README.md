# 数据库脚本与迁移说明

## 文件

- `001_core_schema.mysql8.sql`：M1--M4 所需的可执行 MySQL 8/InnoDB/utf8mb4 核心表，适合从零开始学习实现。
- `legacy_ecshop_structure.sql`：从 PHP 参考项目原样复制的完整结构快照（88 表、1794 行、SHA-256 见 [上级 README](../README.md)），用于逐表查阅和后续迁移。来源为 `/Users/ibqo/Develop/git/github/php/ecshop/upload/install/data/structure.sql`。

遗留快照使用 `TYPE=MyISAM`、`auto_increment`、无外键和 `ecs_` 前缀，不能不经审阅直接用于新项目生产库；新实现请使用 `001_core_schema.mysql8.sql` 加版本化 migration。

## 原始表分组

| 分组 | 原始表 |
| --- | --- |
| 商品目录 | category, goods, products, goods_attr, goods_gallery, goods_type, attribute, brand, goods_cat, goods_article, link_goods, volume_price |
| 用户与会话 | users, user_address, user_rank, sessions, sessions_data, reg_fields, reg_extend_info, collect_goods, tag, user_bonus |
| 交易 | cart, order_info, order_goods, order_action, pay_log, payment, shipping, shipping_area, area_region, delivery_order, delivery_goods, back_order, back_goods |
| 营销 | bonus_type, goods_activity, group_goods, auction_log, snatch_log, favourable_activity, package_goods, exchange_goods, virtual_card |
| 内容/运营 | article, article_cat, comment, feedback, friend_link, topic, vote, vote_option, vote_log, ad, ad_position, ad_custom |
| 管理/系统 | admin_user, role, admin_action, admin_log, shop_config, template, plugins, crons, error_log, stats, region, suppliers |

## 初始化（开发环境）

```bash
mysql --host=127.0.0.1 --port=3306 --user=root -p \
  -e 'CREATE DATABASE ecshop_cpp CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci;'
mysql --host=127.0.0.1 --port=3306 --user=root -p ecshop_cpp \
  < docs/sql/001_core_schema.mysql8.sql
```

生产禁用 root；另建最小权限应用账户。数据导入另建版本化 migration，例如 `002_seed_regions.sql`，不能把演示数据与 DDL 混在一起。

## 从 PHP 库迁移数据的顺序

1. 只读备份旧库，并记录行数、字符集、`ecs_` 前缀。
2. 导入地区、分类、品牌、属性等基础表。
3. 导入用户：强制密码重置，或在首次登录时验证旧哈希后立即改为 Argon2id。
4. 导入商品、图片与 SKU，再验证商品数、库存和价格汇总。
5. 导入订单和订单项，保留原 `order_sn`；历史订单不重新扣库存。
6. 最后导入内容和营销数据；支付令牌、会话、缓存、临时表不迁移。

每一步都在事务/可回滚批次中执行，输出源/目标行数、失败主键和校验和。对于真实生产迁移，请先制作一次脱敏演练库。
