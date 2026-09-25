-- SQLite development schema. Original ECSHOP table and column names are retained.
PRAGMA foreign_keys = ON;
BEGIN;
CREATE TABLE IF NOT EXISTS ecs_category (
 cat_id INTEGER PRIMARY KEY, cat_name VARCHAR(90) NOT NULL DEFAULT '', keywords VARCHAR(255) NOT NULL DEFAULT '', cat_desc VARCHAR(255) NOT NULL DEFAULT '', parent_id INTEGER NOT NULL DEFAULT 0, sort_order INTEGER NOT NULL DEFAULT 50, template_file VARCHAR(50) NOT NULL DEFAULT '', measure_unit VARCHAR(15) NOT NULL DEFAULT '', show_in_nav INTEGER NOT NULL DEFAULT 0, style VARCHAR(150) NOT NULL DEFAULT '', is_show INTEGER NOT NULL DEFAULT 1, grade INTEGER NOT NULL DEFAULT 0, filter_attr VARCHAR(255) NOT NULL DEFAULT '0');
CREATE TABLE IF NOT EXISTS ecs_brand (
 brand_id INTEGER PRIMARY KEY, brand_name VARCHAR(60) NOT NULL DEFAULT '', brand_logo VARCHAR(80) NOT NULL DEFAULT '', brand_desc TEXT NOT NULL DEFAULT '', site_url VARCHAR(255) NOT NULL DEFAULT '', sort_order INTEGER NOT NULL DEFAULT 50, is_show INTEGER NOT NULL DEFAULT 1);
CREATE TABLE IF NOT EXISTS ecs_goods (
 goods_id INTEGER PRIMARY KEY, cat_id INTEGER NOT NULL DEFAULT 0, goods_sn VARCHAR(60) NOT NULL DEFAULT '', goods_name VARCHAR(120) NOT NULL DEFAULT '', goods_name_style VARCHAR(60) NOT NULL DEFAULT '+', click_count INTEGER NOT NULL DEFAULT 0, brand_id INTEGER NOT NULL DEFAULT 0, provider_name VARCHAR(100) NOT NULL DEFAULT '', goods_number INTEGER NOT NULL DEFAULT 0, goods_weight DECIMAL(10,3) NOT NULL DEFAULT 0.000, market_price DECIMAL(10,2) NOT NULL DEFAULT 0.00, shop_price DECIMAL(10,2) NOT NULL DEFAULT 0.00, promote_price DECIMAL(10,2) NOT NULL DEFAULT 0.00, promote_start_date INTEGER NOT NULL DEFAULT 0, promote_end_date INTEGER NOT NULL DEFAULT 0, warn_number INTEGER NOT NULL DEFAULT 1, keywords VARCHAR(255) NOT NULL DEFAULT '', goods_brief VARCHAR(255) NOT NULL DEFAULT '', goods_desc TEXT NOT NULL DEFAULT '', goods_thumb VARCHAR(255) NOT NULL DEFAULT '', goods_img VARCHAR(255) NOT NULL DEFAULT '', original_img VARCHAR(255) NOT NULL DEFAULT '', is_real INTEGER NOT NULL DEFAULT 1, extension_code VARCHAR(30) NOT NULL DEFAULT '', is_on_sale INTEGER NOT NULL DEFAULT 1, is_alone_sale INTEGER NOT NULL DEFAULT 1, is_shipping INTEGER NOT NULL DEFAULT 0, integral INTEGER NOT NULL DEFAULT 0, add_time INTEGER NOT NULL DEFAULT 0, sort_order INTEGER NOT NULL DEFAULT 100, is_delete INTEGER NOT NULL DEFAULT 0, is_best INTEGER NOT NULL DEFAULT 0, is_new INTEGER NOT NULL DEFAULT 0, is_hot INTEGER NOT NULL DEFAULT 0, is_promote INTEGER NOT NULL DEFAULT 0, bonus_type_id INTEGER NOT NULL DEFAULT 0, last_update INTEGER NOT NULL DEFAULT 0, goods_type INTEGER NOT NULL DEFAULT 0, seller_note VARCHAR(255) NOT NULL DEFAULT '', give_integral INTEGER NOT NULL DEFAULT -1, rank_integral INTEGER NOT NULL DEFAULT -1, suppliers_id INTEGER, is_check INTEGER,
 FOREIGN KEY(cat_id) REFERENCES ecs_category(cat_id), FOREIGN KEY(brand_id) REFERENCES ecs_brand(brand_id));
CREATE INDEX IF NOT EXISTS idx_ecs_goods_visibility ON ecs_goods(goods_id, is_on_sale, is_delete);
CREATE TABLE IF NOT EXISTS ecs_products (
 product_id INTEGER PRIMARY KEY, goods_id INTEGER NOT NULL, goods_attr VARCHAR(255) NOT NULL DEFAULT '', product_sn VARCHAR(60) NOT NULL DEFAULT '', product_number INTEGER NOT NULL DEFAULT 0,
 FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_goods_attr (
 goods_attr_id INTEGER PRIMARY KEY, goods_id INTEGER NOT NULL, attr_id INTEGER NOT NULL DEFAULT 0, attr_value TEXT NOT NULL DEFAULT '', attr_price VARCHAR(255) NOT NULL DEFAULT '0.00',
 FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_goods_gallery (img_id INTEGER PRIMARY KEY,goods_id INTEGER NOT NULL,img_url TEXT NOT NULL DEFAULT '',img_desc TEXT NOT NULL DEFAULT '',thumb_url TEXT NOT NULL DEFAULT '',img_original TEXT NOT NULL DEFAULT '',FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_tag (tag_id INTEGER PRIMARY KEY AUTOINCREMENT,user_id INTEGER NOT NULL DEFAULT 0,goods_id INTEGER NOT NULL,tag_words TEXT NOT NULL DEFAULT '',FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE UNIQUE INDEX IF NOT EXISTS uk_ecs_tag_authenticated_user_goods_word ON ecs_tag(user_id,goods_id,tag_words) WHERE user_id<>0;
CREATE TABLE IF NOT EXISTS ecs_booking_goods (rec_id INTEGER PRIMARY KEY AUTOINCREMENT,user_id INTEGER NOT NULL DEFAULT 0,email VARCHAR(60) NOT NULL DEFAULT '',link_man VARCHAR(60) NOT NULL DEFAULT '',tel VARCHAR(60) NOT NULL DEFAULT '',goods_id INTEGER NOT NULL DEFAULT 0,goods_desc VARCHAR(255) NOT NULL DEFAULT '',goods_number INTEGER NOT NULL DEFAULT 0,booking_time INTEGER NOT NULL DEFAULT 0,is_dispose INTEGER NOT NULL DEFAULT 0,dispose_user VARCHAR(30) NOT NULL DEFAULT '',dispose_time INTEGER NOT NULL DEFAULT 0,dispose_note VARCHAR(255) NOT NULL DEFAULT '',FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE,FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE UNIQUE INDEX IF NOT EXISTS uk_ecs_booking_user_goods ON ecs_booking_goods(user_id,goods_id);
CREATE TABLE IF NOT EXISTS ecs_article_cat (
 cat_id INTEGER PRIMARY KEY, cat_name VARCHAR(90) NOT NULL DEFAULT '', cat_desc TEXT NOT NULL DEFAULT '', keywords VARCHAR(255) NOT NULL DEFAULT '', sort_order INTEGER NOT NULL DEFAULT 50, is_show INTEGER NOT NULL DEFAULT 1);
CREATE TABLE IF NOT EXISTS ecs_article (
 article_id INTEGER PRIMARY KEY, cat_id INTEGER NOT NULL, title VARCHAR(255) NOT NULL DEFAULT '', author VARCHAR(255) NOT NULL DEFAULT '', article_desc TEXT NOT NULL DEFAULT '', content TEXT NOT NULL DEFAULT '', keywords VARCHAR(255) NOT NULL DEFAULT '', is_open INTEGER NOT NULL DEFAULT 1,
 FOREIGN KEY(cat_id) REFERENCES ecs_article_cat(cat_id));
CREATE TABLE IF NOT EXISTS ecs_region (
 region_id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL DEFAULT 0, region_name VARCHAR(120) NOT NULL, region_type INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS ecs_users (
 user_id INTEGER PRIMARY KEY AUTOINCREMENT, user_name VARCHAR(60) NOT NULL UNIQUE, email VARCHAR(120) NOT NULL UNIQUE, password_hash VARCHAR(255) NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS ecs_sessions (
 token_hash CHAR(64) PRIMARY KEY, user_id INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch()),
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS ecs_user_referral (
 user_id INTEGER PRIMARY KEY,referrer_user_id INTEGER NOT NULL,created_at INTEGER NOT NULL DEFAULT (unixepoch()),CHECK(user_id<>referrer_user_id),
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE,FOREIGN KEY(referrer_user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE INDEX IF NOT EXISTS idx_ecs_user_referral_referrer ON ecs_user_referral(referrer_user_id,user_id);
CREATE TABLE IF NOT EXISTS ecs_account_balance (
 user_id INTEGER PRIMARY KEY,available_cents INTEGER NOT NULL DEFAULT 0 CHECK(available_cents>=0),frozen_cents INTEGER NOT NULL DEFAULT 0 CHECK(frozen_cents>=0),
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS ecs_user_account (
 rec_id INTEGER PRIMARY KEY AUTOINCREMENT,user_id INTEGER NOT NULL,amount_cents INTEGER NOT NULL CHECK(amount_cents>0),process_type VARCHAR(20) NOT NULL CHECK(process_type IN ('deposit','withdrawal')),payment_id INTEGER,user_note VARCHAR(255) NOT NULL DEFAULT '',admin_note VARCHAR(255) NOT NULL DEFAULT '',status VARCHAR(30) NOT NULL,created_at INTEGER NOT NULL DEFAULT (unixepoch()),paid_at INTEGER,
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE,FOREIGN KEY(payment_id) REFERENCES ecs_payment(pay_id));
CREATE INDEX IF NOT EXISTS idx_ecs_user_account_user ON ecs_user_account(user_id,rec_id);
CREATE TABLE IF NOT EXISTS ecs_account_payment_intent (
 intent_id INTEGER PRIMARY KEY AUTOINCREMENT,request_id INTEGER NOT NULL UNIQUE,user_id INTEGER NOT NULL,payment_id INTEGER NOT NULL,amount_cents INTEGER NOT NULL CHECK(amount_cents>0),fee_cents INTEGER NOT NULL DEFAULT 0 CHECK(fee_cents>=0),status VARCHAR(30) NOT NULL DEFAULT 'pending',created_at INTEGER NOT NULL DEFAULT (unixepoch()),updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
 FOREIGN KEY(request_id) REFERENCES ecs_user_account(rec_id) ON DELETE CASCADE,FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE,FOREIGN KEY(payment_id) REFERENCES ecs_payment(pay_id));
CREATE INDEX IF NOT EXISTS idx_ecs_account_payment_intent_user ON ecs_account_payment_intent(user_id,intent_id);
CREATE TABLE IF NOT EXISTS ecs_account_log (
 log_id INTEGER PRIMARY KEY AUTOINCREMENT,user_id INTEGER NOT NULL,available_delta_cents INTEGER NOT NULL DEFAULT 0,frozen_delta_cents INTEGER NOT NULL DEFAULT 0,reason VARCHAR(255) NOT NULL DEFAULT '',reference_type VARCHAR(60) NOT NULL DEFAULT '',reference_id INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL DEFAULT (unixepoch()),
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE INDEX IF NOT EXISTS idx_ecs_account_log_user ON ecs_account_log(user_id,log_id);
CREATE TABLE IF NOT EXISTS ecs_email_verification_tokens (
 user_id INTEGER PRIMARY KEY,token_hash CHAR(64) NOT NULL UNIQUE,expires_at INTEGER NOT NULL,created_at INTEGER NOT NULL DEFAULT (unixepoch()),consumed_at INTEGER,
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS ecs_email_verified_users (
 user_id INTEGER PRIMARY KEY,verified_at INTEGER NOT NULL,
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS ecs_password_reset_tokens (
 user_id INTEGER PRIMARY KEY,token_hash CHAR(64) NOT NULL UNIQUE,expires_at INTEGER NOT NULL,created_at INTEGER NOT NULL DEFAULT (unixepoch()),consumed_at INTEGER,
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS ecs_email_outbox (
 message_id INTEGER PRIMARY KEY AUTOINCREMENT,user_id INTEGER,recipient VARCHAR(120) NOT NULL,template_name VARCHAR(60) NOT NULL,payload TEXT NOT NULL,status VARCHAR(20) NOT NULL DEFAULT 'pending',created_at INTEGER NOT NULL DEFAULT (unixepoch()),sent_at INTEGER,
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE SET NULL);
CREATE INDEX IF NOT EXISTS idx_ecs_email_outbox_pending ON ecs_email_outbox(status,message_id);
CREATE TABLE IF NOT EXISTS ecs_email_list (
 email VARCHAR(120) PRIMARY KEY,status INTEGER NOT NULL DEFAULT 0 CHECK(status IN (0,1)),token_hash CHAR(64) UNIQUE,pending_action VARCHAR(20) NOT NULL DEFAULT '' CHECK(pending_action IN ('','subscribe','unsubscribe')),token_expires_at INTEGER,updated_at INTEGER NOT NULL DEFAULT (unixepoch()));
CREATE TABLE IF NOT EXISTS ecs_bonus_type (
 type_id INTEGER PRIMARY KEY AUTOINCREMENT,type_name VARCHAR(60) NOT NULL DEFAULT '',type_money DECIMAL(10,2) NOT NULL DEFAULT 0.00,send_type INTEGER NOT NULL DEFAULT 0,min_amount DECIMAL(10,2) NOT NULL DEFAULT 0.00,max_amount DECIMAL(10,2) NOT NULL DEFAULT 0.00,send_start_date INTEGER NOT NULL DEFAULT 0,send_end_date INTEGER NOT NULL DEFAULT 0,use_start_date INTEGER NOT NULL DEFAULT 0,use_end_date INTEGER NOT NULL DEFAULT 0,min_goods_amount DECIMAL(10,2) NOT NULL DEFAULT 0.00);
CREATE TABLE IF NOT EXISTS ecs_user_bonus (
 bonus_id INTEGER PRIMARY KEY AUTOINCREMENT,bonus_type_id INTEGER NOT NULL DEFAULT 0,bonus_sn INTEGER NOT NULL UNIQUE,user_id INTEGER NOT NULL DEFAULT 0,used_time INTEGER NOT NULL DEFAULT 0,order_id INTEGER NOT NULL DEFAULT 0,emailed INTEGER NOT NULL DEFAULT 0,
 FOREIGN KEY(bonus_type_id) REFERENCES ecs_bonus_type(type_id));
CREATE TABLE IF NOT EXISTS ecs_user_address (
 address_id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER NOT NULL, consignee VARCHAR(60) NOT NULL, country_id INTEGER NOT NULL DEFAULT 0, province_id INTEGER NOT NULL DEFAULT 0, city_id INTEGER NOT NULL DEFAULT 0, district_id INTEGER NOT NULL DEFAULT 0, address VARCHAR(255) NOT NULL, zipcode VARCHAR(20) NOT NULL DEFAULT '', mobile VARCHAR(32) NOT NULL DEFAULT '', is_default INTEGER NOT NULL DEFAULT 0,
 FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS ecs_cart (
 rec_id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER NOT NULL, goods_id INTEGER NOT NULL, goods_number INTEGER NOT NULL CHECK(goods_number>0), version INTEGER NOT NULL DEFAULT 1, UNIQUE(user_id,goods_id), FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE, FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_shipping (
 shipping_id INTEGER PRIMARY KEY, shipping_name VARCHAR(120) NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, shipping_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00);
CREATE TABLE IF NOT EXISTS ecs_payment (
 pay_id INTEGER PRIMARY KEY, pay_name VARCHAR(120) NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, pay_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00);
CREATE TABLE IF NOT EXISTS ecs_order_info (
 order_id INTEGER PRIMARY KEY AUTOINCREMENT, order_sn VARCHAR(40) NOT NULL UNIQUE, user_id INTEGER NOT NULL, order_status VARCHAR(32) NOT NULL DEFAULT 'pending_payment', consignee VARCHAR(60) NOT NULL, address VARCHAR(255) NOT NULL, mobile VARCHAR(32) NOT NULL DEFAULT '', shipping_id INTEGER NOT NULL, pay_id INTEGER NOT NULL, goods_amount DECIMAL(12,2) NOT NULL, shipping_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00, payment_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00, order_amount DECIMAL(12,2) NOT NULL, idempotency_key VARCHAR(100) NOT NULL, request_fingerprint TEXT NOT NULL, remark VARCHAR(255) NOT NULL DEFAULT '', created_at INTEGER NOT NULL DEFAULT (unixepoch()), UNIQUE(user_id,idempotency_key), FOREIGN KEY(user_id) REFERENCES ecs_users(user_id), FOREIGN KEY(shipping_id) REFERENCES ecs_shipping(shipping_id), FOREIGN KEY(pay_id) REFERENCES ecs_payment(pay_id));
CREATE TABLE IF NOT EXISTS ecs_order_delivery_address (
 order_id INTEGER PRIMARY KEY,user_id INTEGER NOT NULL,email VARCHAR(60) NOT NULL,zipcode VARCHAR(60) NOT NULL DEFAULT '',telephone VARCHAR(60) NOT NULL DEFAULT '',sign_building VARCHAR(120) NOT NULL DEFAULT '',best_time VARCHAR(120) NOT NULL DEFAULT '',
 FOREIGN KEY(order_id) REFERENCES ecs_order_info(order_id) ON DELETE CASCADE,FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS ecs_order_balance_payment (
 order_id INTEGER PRIMARY KEY,user_id INTEGER NOT NULL,paid_cents INTEGER NOT NULL DEFAULT 0 CHECK(paid_cents>=0),updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
 FOREIGN KEY(order_id) REFERENCES ecs_order_info(order_id) ON DELETE CASCADE,FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE INDEX IF NOT EXISTS idx_ecs_order_balance_payment_user ON ecs_order_balance_payment(user_id,order_id);
CREATE TABLE IF NOT EXISTS ecs_order_goods (
 rec_id INTEGER PRIMARY KEY AUTOINCREMENT, order_id INTEGER NOT NULL, goods_id INTEGER NOT NULL, goods_name VARCHAR(255) NOT NULL, goods_number INTEGER NOT NULL CHECK(goods_number>0), goods_price DECIMAL(12,2) NOT NULL, FOREIGN KEY(order_id) REFERENCES ecs_order_info(order_id), FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_order_action (
 action_id INTEGER PRIMARY KEY AUTOINCREMENT, order_id INTEGER NOT NULL, actor_user_id INTEGER NOT NULL, action VARCHAR(32) NOT NULL, note VARCHAR(255) NOT NULL DEFAULT '', created_at INTEGER NOT NULL DEFAULT (unixepoch()), FOREIGN KEY(order_id) REFERENCES ecs_order_info(order_id), FOREIGN KEY(actor_user_id) REFERENCES ecs_users(user_id));
CREATE TABLE IF NOT EXISTS ecs_affiliate_log (
 log_id INTEGER PRIMARY KEY AUTOINCREMENT,order_id INTEGER NOT NULL,user_id INTEGER NOT NULL,user_name VARCHAR(60) NOT NULL DEFAULT '',money DECIMAL(12,2) NOT NULL DEFAULT 0.00,point INTEGER NOT NULL DEFAULT 0,separate_type INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL DEFAULT (unixepoch()),UNIQUE(order_id,user_id),
 FOREIGN KEY(order_id) REFERENCES ecs_order_info(order_id) ON DELETE CASCADE,FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE);
CREATE INDEX IF NOT EXISTS idx_ecs_affiliate_log_user ON ecs_affiliate_log(user_id,log_id);
CREATE TABLE IF NOT EXISTS ecs_comment (
 comment_id INTEGER PRIMARY KEY AUTOINCREMENT, comment_type INTEGER NOT NULL DEFAULT 0, id_value INTEGER NOT NULL, user_id INTEGER NOT NULL, user_name VARCHAR(60) NOT NULL, content TEXT NOT NULL, status INTEGER NOT NULL DEFAULT 1, add_time INTEGER NOT NULL DEFAULT (unixepoch()), FOREIGN KEY(id_value) REFERENCES ecs_goods(goods_id), FOREIGN KEY(user_id) REFERENCES ecs_users(user_id));
CREATE INDEX IF NOT EXISTS idx_ecs_comment_goods_status ON ecs_comment(id_value,status,comment_id DESC);
CREATE TABLE IF NOT EXISTS ecs_pay_log (
 log_id INTEGER PRIMARY KEY AUTOINCREMENT, order_id INTEGER NOT NULL, provider VARCHAR(60) NOT NULL, provider_trade_no VARCHAR(120) NOT NULL, amount DECIMAL(12,2) NOT NULL, status VARCHAR(32) NOT NULL, raw_payload TEXT NOT NULL, received_at INTEGER NOT NULL DEFAULT (unixepoch()), UNIQUE(provider,provider_trade_no), FOREIGN KEY(order_id) REFERENCES ecs_order_info(order_id));
CREATE TABLE IF NOT EXISTS ecs_goods_activity (
 act_id INTEGER PRIMARY KEY, act_name VARCHAR(255) NOT NULL, act_desc TEXT NOT NULL DEFAULT '', act_type INTEGER NOT NULL, goods_id INTEGER NOT NULL, product_id INTEGER NOT NULL DEFAULT 0, goods_name VARCHAR(255) NOT NULL DEFAULT '', start_time INTEGER NOT NULL, end_time INTEGER NOT NULL, is_finished INTEGER NOT NULL DEFAULT 0, ext_info TEXT NOT NULL DEFAULT '', FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_order_promotion (
 order_id INTEGER PRIMARY KEY,promotion_id INTEGER NOT NULL,promotion_type VARCHAR(32) NOT NULL,
 FOREIGN KEY(order_id) REFERENCES ecs_order_info(order_id) ON DELETE CASCADE,FOREIGN KEY(promotion_id) REFERENCES ecs_goods_activity(act_id));
CREATE INDEX IF NOT EXISTS idx_ecs_order_promotion_activity ON ecs_order_promotion(promotion_type,promotion_id,order_id);
CREATE TABLE IF NOT EXISTS ecs_package_goods (package_id INTEGER NOT NULL,goods_id INTEGER NOT NULL,product_id INTEGER NOT NULL DEFAULT 0,goods_number INTEGER NOT NULL DEFAULT 1,admin_id INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(package_id,goods_id,admin_id,product_id),FOREIGN KEY(package_id) REFERENCES ecs_goods_activity(act_id),FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_favourable_activity (act_id INTEGER PRIMARY KEY AUTOINCREMENT,act_name VARCHAR(255) NOT NULL,start_time INTEGER NOT NULL,end_time INTEGER NOT NULL,user_rank VARCHAR(255) NOT NULL DEFAULT '0',act_range INTEGER NOT NULL DEFAULT 0,act_range_ext VARCHAR(255) NOT NULL DEFAULT '',min_amount DECIMAL(10,2) NOT NULL DEFAULT 0,max_amount DECIMAL(10,2) NOT NULL DEFAULT 0,act_type INTEGER NOT NULL DEFAULT 0,act_type_ext DECIMAL(10,2) NOT NULL DEFAULT 0,gift TEXT NOT NULL DEFAULT '[]',sort_order INTEGER NOT NULL DEFAULT 50);
CREATE TABLE IF NOT EXISTS ecs_topic (topic_id INTEGER PRIMARY KEY AUTOINCREMENT,title VARCHAR(255) NOT NULL,intro TEXT NOT NULL DEFAULT '',start_time INTEGER NOT NULL DEFAULT 0,end_time INTEGER NOT NULL DEFAULT 0,data TEXT NOT NULL DEFAULT '{}',template VARCHAR(255) NOT NULL DEFAULT '',css TEXT NOT NULL DEFAULT '',topic_img VARCHAR(255),title_pic VARCHAR(255),base_style VARCHAR(6),htmls TEXT,keywords VARCHAR(255),description VARCHAR(255));
CREATE TABLE IF NOT EXISTS ecs_vote (vote_id INTEGER PRIMARY KEY AUTOINCREMENT,vote_name VARCHAR(250) NOT NULL DEFAULT '',start_time INTEGER NOT NULL DEFAULT 0,end_time INTEGER NOT NULL DEFAULT 0,can_multi INTEGER NOT NULL DEFAULT 0,vote_count INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS ecs_vote_option (option_id INTEGER PRIMARY KEY AUTOINCREMENT,vote_id INTEGER NOT NULL,option_name VARCHAR(250) NOT NULL DEFAULT '',option_count INTEGER NOT NULL DEFAULT 0,option_order INTEGER NOT NULL DEFAULT 100,FOREIGN KEY(vote_id) REFERENCES ecs_vote(vote_id));
CREATE TABLE IF NOT EXISTS ecs_vote_log (log_id INTEGER PRIMARY KEY AUTOINCREMENT,vote_id INTEGER NOT NULL,ip_address VARCHAR(45) NOT NULL,vote_time INTEGER NOT NULL DEFAULT (unixepoch()),UNIQUE(vote_id,ip_address),FOREIGN KEY(vote_id) REFERENCES ecs_vote(vote_id));
CREATE TABLE IF NOT EXISTS ecs_exchange_goods (goods_id INTEGER PRIMARY KEY,exchange_integral INTEGER NOT NULL DEFAULT 0,is_exchange INTEGER NOT NULL DEFAULT 0,is_hot INTEGER NOT NULL DEFAULT 0,FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_collect_goods (rec_id INTEGER PRIMARY KEY AUTOINCREMENT,user_id INTEGER NOT NULL,goods_id INTEGER NOT NULL,add_time INTEGER NOT NULL DEFAULT (unixepoch()),is_attention INTEGER NOT NULL DEFAULT 0,UNIQUE(user_id,goods_id),FOREIGN KEY(user_id) REFERENCES ecs_users(user_id),FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id));
CREATE TABLE IF NOT EXISTS ecs_feedback (msg_id INTEGER PRIMARY KEY AUTOINCREMENT,parent_id INTEGER NOT NULL DEFAULT 0,user_id INTEGER NOT NULL DEFAULT 0,user_name TEXT NOT NULL DEFAULT '',user_email TEXT NOT NULL DEFAULT '',msg_title TEXT NOT NULL DEFAULT '',msg_type INTEGER NOT NULL DEFAULT 0,msg_status INTEGER NOT NULL DEFAULT 1,msg_content TEXT NOT NULL,msg_time INTEGER NOT NULL DEFAULT (unixepoch()),message_img TEXT NOT NULL DEFAULT '0',order_id INTEGER NOT NULL DEFAULT 0,msg_area INTEGER NOT NULL DEFAULT 1);
CREATE TABLE IF NOT EXISTS ecs_ad (ad_id INTEGER PRIMARY KEY,position_id INTEGER NOT NULL DEFAULT 0,media_type INTEGER NOT NULL DEFAULT 0,ad_name TEXT NOT NULL DEFAULT '',ad_link TEXT NOT NULL DEFAULT '',ad_code TEXT NOT NULL,start_time INTEGER NOT NULL DEFAULT 0,end_time INTEGER NOT NULL DEFAULT 0,click_count INTEGER NOT NULL DEFAULT 0,enabled INTEGER NOT NULL DEFAULT 1);
CREATE TABLE IF NOT EXISTS ecs_adsense (from_ad INTEGER NOT NULL,referer TEXT NOT NULL DEFAULT '',clicks INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(from_ad,referer));
CREATE INDEX IF NOT EXISTS idx_ecs_activity_active ON ecs_goods_activity(act_type,start_time,end_time,is_finished);
INSERT OR IGNORE INTO ecs_category(cat_id, cat_name) VALUES(1, '示例分类');
INSERT OR IGNORE INTO ecs_brand(brand_id, brand_name) VALUES(1, '示例品牌');
INSERT OR IGNORE INTO ecs_article_cat(cat_id, cat_name, cat_desc) VALUES(1, '商城公告', '示例文章分类');
INSERT OR IGNORE INTO ecs_article(article_id, cat_id, title, author, article_desc, content, is_open) VALUES(1, 1, 'C++ ECSHOP 项目说明', 'cpp_ecshop', '用于验证文章 URL 的示例文章', '这是来自 SQLite ecs_article 表的示例正文。', 1);
INSERT OR IGNORE INTO ecs_region(region_id, parent_id, region_name, region_type) VALUES(1, 0, '中国', 1);
INSERT OR IGNORE INTO ecs_region(region_id, parent_id, region_name, region_type) VALUES(2, 1, '北京市', 2);
INSERT OR IGNORE INTO ecs_goods(goods_id,cat_id,goods_sn,goods_name,brand_id,goods_number,market_price,shop_price,goods_brief,goods_desc,is_on_sale,is_delete) VALUES(12,1,'CPP-EC-001','C++ 入门商品',1,18,69.90,49.90,'用于验证第一个 HTTP URL 的示例商品','来自 SQLite ecs_goods 表',1,0);
INSERT OR IGNORE INTO ecs_goods(goods_id,cat_id,goods_sn,goods_name,brand_id,goods_number,market_price,shop_price,goods_brief,goods_desc,is_on_sale,is_delete) VALUES(14,1,'CPP-EC-003','C++ 进阶商品',1,9,99.90,79.90,'用于验证商品对比的第二个公开商品','来自 SQLite ecs_goods 表',1,0);
INSERT OR IGNORE INTO ecs_goods(goods_id,cat_id,goods_sn,goods_name,brand_id,goods_number,market_price,shop_price,goods_brief,goods_desc,is_on_sale,is_delete) VALUES(13,1,'CPP-EC-002','已下架商品',1,5,129.90,99.90,'不可公开访问','用于验证可见性过滤',0,0);
INSERT OR IGNORE INTO ecs_goods_attr(goods_attr_id,goods_id,attr_id,attr_value,attr_price) VALUES(1001,12,1,'扩展版','5.00');
INSERT OR IGNORE INTO ecs_goods_attr(goods_attr_id,goods_id,attr_id,attr_value,attr_price) VALUES(1005,12,2,'标准包装','0.00');
INSERT OR IGNORE INTO ecs_goods_gallery(img_id,goods_id,img_url,img_desc,thumb_url,img_original) VALUES(1,12,'https://example.test/images/cpp-ecshop-1.jpg','C++ ECSHOP 示例商品图','https://example.test/images/cpp-ecshop-1-thumb.jpg','https://example.test/images/cpp-ecshop-1-original.jpg');
INSERT OR IGNORE INTO ecs_tag(tag_id,user_id,goods_id,tag_words) VALUES(1,0,12,'C++');
INSERT OR IGNORE INTO ecs_tag(tag_id,user_id,goods_id,tag_words) VALUES(2,0,12,'商城');
INSERT OR IGNORE INTO ecs_tag(tag_id,user_id,goods_id,tag_words) VALUES(3,0,12,'C++');
INSERT OR IGNORE INTO ecs_products(product_id,goods_id,goods_attr,product_sn,product_number) VALUES(81,12,'1001|1005','CPP-EC-001-EXT',10);
INSERT OR IGNORE INTO ecs_shipping(shipping_id,shipping_name,enabled,shipping_fee) VALUES(1,'标准快递',1,'8.00');
INSERT OR IGNORE INTO ecs_payment(pay_id,pay_name,enabled,pay_fee) VALUES(1,'在线支付',1,'0.00');
INSERT OR IGNORE INTO ecs_payment(pay_id,pay_name,enabled,pay_fee) VALUES(2,'线下转账',1,'2.00');
INSERT OR IGNORE INTO ecs_goods_activity(act_id,act_name,act_desc,act_type,goods_id,goods_name,start_time,end_time,is_finished,ext_info) VALUES(1,'C++ 示例团购','用于学习 goods_activity 的公开读取',1,12,'C++ 入门商品',0,4102444800,0,'{"cur_price":"39.90"}');
INSERT OR IGNORE INTO ecs_goods_activity(act_id,act_name,act_desc,act_type,goods_id,goods_name,start_time,end_time,is_finished,ext_info) VALUES(2,'C++ 学习礼包','组合购买两个示例商品',4,12,'C++ 学习礼包',0,4102444800,0,'{"package_price":"119.00"}');
INSERT OR IGNORE INTO ecs_package_goods(package_id,goods_id,goods_number) VALUES(2,12,1);
INSERT OR IGNORE INTO ecs_package_goods(package_id,goods_id,goods_number) VALUES(2,14,1);
INSERT OR IGNORE INTO ecs_favourable_activity(act_id,act_name,start_time,end_time,user_rank,act_range,act_range_ext,min_amount,max_amount,act_type,act_type_ext,gift,sort_order) VALUES(1,'满百减十',0,4102444800,'0',0,'','100.00','0.00',1,'10.00','[]',10);
INSERT OR IGNORE INTO ecs_topic(topic_id,title,intro,start_time,end_time,data,css,topic_img,title_pic,base_style,keywords,description) VALUES(1,'C++ 学习专题','按学习阶段组织示例商品',0,4102444800,'{"入门":[12],"进阶":[14]}','.topic-title{color:#336699;}','topic/cpp.jpg','topic/cpp-title.jpg','336699','C++,商城','C++ ECSHOP 学习专题');
INSERT OR IGNORE INTO ecs_vote(vote_id,vote_name,start_time,end_time,can_multi,vote_count) VALUES(1,'你最想继续学习什么？',0,4102444800,0,0);
INSERT OR IGNORE INTO ecs_vote_option(option_id,vote_id,option_name,option_order) VALUES(1,1,'C++ HTTP 服务',10);
INSERT OR IGNORE INTO ecs_vote_option(option_id,vote_id,option_name,option_order) VALUES(2,1,'数据库事务',20);
INSERT OR IGNORE INTO ecs_exchange_goods(goods_id,exchange_integral,is_exchange,is_hot) VALUES(12,500,1,1);
INSERT OR IGNORE INTO ecs_exchange_goods(goods_id,exchange_integral,is_exchange,is_hot) VALUES(14,800,1,0);
INSERT OR IGNORE INTO ecs_ad(ad_id,media_type,ad_name,ad_link,ad_code,start_time,end_time,enabled) VALUES(1,3,'C++ ECSHOP 示例广告','https://example.test/ecshop','C++ ECSHOP 示例广告',0,4102444800,1);
INSERT OR IGNORE INTO ecs_ad(ad_id,position_id,media_type,ad_name,ad_link,ad_code,start_time,end_time,enabled) VALUES(2,1,0,'C++ ECSHOP 示例轮播','https://example.test/ecshop','https://example.test/images/cpp-ecshop-banner.jpg',0,4102444800,1);
INSERT OR IGNORE INTO ecs_bonus_type(type_id,type_name,type_money,use_start_date,use_end_date,min_goods_amount) VALUES(1,'C++ 学习红包',10.00,0,4102444800,100.00);
INSERT OR IGNORE INTO ecs_user_bonus(bonus_id,bonus_type_id,bonus_sn) VALUES(1,1,202609230001);
COMMIT;
