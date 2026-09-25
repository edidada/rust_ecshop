-- cpp_ecshop core schema; MySQL 8.0+, InnoDB, utf8mb4.
-- All monetary values are DECIMAL(12,2). Timestamps use UTC.
SET NAMES utf8mb4;
SET time_zone = '+00:00';

CREATE TABLE region (
  region_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  parent_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  region_name VARCHAR(120) NOT NULL,
  region_type TINYINT UNSIGNED NOT NULL,
  KEY idx_region_parent (parent_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE users (
  user_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  user_name VARCHAR(60) NOT NULL,
  email VARCHAR(120) NOT NULL,
  password_hash VARCHAR(255) NOT NULL,
  mobile_phone VARCHAR(32) NOT NULL DEFAULT '',
  user_money DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  pay_points INT NOT NULL DEFAULT 0,
  rank_points INT NOT NULL DEFAULT 0,
  is_validated TINYINT(1) NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uk_users_name (user_name),
  UNIQUE KEY uk_users_email (email)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE sessions (
  token_hash CHAR(64) PRIMARY KEY,
  user_id BIGINT UNSIGNED NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_sessions_user (user_id),
  CONSTRAINT fk_sessions_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE user_referral (
  user_id BIGINT UNSIGNED PRIMARY KEY,
  referrer_user_id BIGINT UNSIGNED NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_user_referral_referrer (referrer_user_id, user_id),
  CONSTRAINT fk_user_referral_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  CONSTRAINT fk_user_referral_referrer FOREIGN KEY (referrer_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  CONSTRAINT ck_user_referral_not_self CHECK (user_id <> referrer_user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE account_balance (
  user_id BIGINT UNSIGNED PRIMARY KEY,
  available_cents BIGINT UNSIGNED NOT NULL DEFAULT 0,
  frozen_cents BIGINT UNSIGNED NOT NULL DEFAULT 0,
  CONSTRAINT fk_account_balance_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE user_account (
  rec_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NOT NULL,
  amount_cents BIGINT UNSIGNED NOT NULL,
  process_type VARCHAR(20) NOT NULL,
  payment_id BIGINT UNSIGNED NULL,
  user_note VARCHAR(255) NOT NULL DEFAULT '',
  admin_note VARCHAR(255) NOT NULL DEFAULT '',
  status VARCHAR(30) NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  paid_at DATETIME NULL,
  KEY idx_user_account_user (user_id, rec_id),
  CONSTRAINT fk_user_account_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  CONSTRAINT ck_user_account_amount CHECK (amount_cents > 0),
  CONSTRAINT ck_user_account_type CHECK (process_type IN ('deposit','withdrawal'))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE account_payment_intent (
  intent_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  request_id BIGINT UNSIGNED NOT NULL,
  user_id BIGINT UNSIGNED NOT NULL,
  payment_id BIGINT UNSIGNED NOT NULL,
  amount_cents BIGINT UNSIGNED NOT NULL,
  fee_cents BIGINT UNSIGNED NOT NULL DEFAULT 0,
  status VARCHAR(30) NOT NULL DEFAULT 'pending',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uk_account_payment_intent_request (request_id),
  KEY idx_account_payment_intent_user (user_id, intent_id),
  CONSTRAINT fk_account_payment_intent_request FOREIGN KEY (request_id) REFERENCES user_account(rec_id) ON DELETE CASCADE,
  CONSTRAINT fk_account_payment_intent_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  CONSTRAINT ck_account_payment_intent_amount CHECK (amount_cents > 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE account_log (
  log_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NOT NULL,
  available_delta_cents BIGINT NOT NULL DEFAULT 0,
  frozen_delta_cents BIGINT NOT NULL DEFAULT 0,
  reason VARCHAR(255) NOT NULL DEFAULT '',
  reference_type VARCHAR(60) NOT NULL DEFAULT '',
  reference_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_account_log_user (user_id, log_id),
  CONSTRAINT fk_account_log_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE email_verification_tokens (
  user_id BIGINT UNSIGNED PRIMARY KEY,
  token_hash CHAR(64) NOT NULL,
  expires_at DATETIME NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  consumed_at DATETIME NULL,
  UNIQUE KEY uk_email_verification_hash (token_hash),
  CONSTRAINT fk_email_verification_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE email_verified_users (
  user_id BIGINT UNSIGNED PRIMARY KEY,
  verified_at DATETIME NOT NULL,
  CONSTRAINT fk_email_verified_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE password_reset_tokens (
  user_id BIGINT UNSIGNED PRIMARY KEY,
  token_hash CHAR(64) NOT NULL,
  expires_at DATETIME NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  consumed_at DATETIME NULL,
  UNIQUE KEY uk_password_reset_hash (token_hash),
  CONSTRAINT fk_password_reset_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE email_outbox (
  message_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NULL,
  recipient VARCHAR(120) NOT NULL,
  template_name VARCHAR(60) NOT NULL,
  payload JSON NOT NULL,
  status VARCHAR(20) NOT NULL DEFAULT 'pending',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  sent_at DATETIME NULL,
  KEY idx_email_outbox_pending (status, message_id),
  CONSTRAINT fk_email_outbox_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE email_list (
  email VARCHAR(120) PRIMARY KEY,
  status TINYINT(1) NOT NULL DEFAULT 0,
  token_hash CHAR(64) NULL,
  pending_action VARCHAR(20) NOT NULL DEFAULT '',
  token_expires_at DATETIME NULL,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uk_email_list_token (token_hash),
  CONSTRAINT ck_email_list_status CHECK (status IN (0,1)),
  CONSTRAINT ck_email_list_action CHECK (pending_action IN ('','subscribe','unsubscribe'))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE bonus_type (
  type_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  type_name VARCHAR(60) NOT NULL DEFAULT '',
  type_money DECIMAL(10,2) NOT NULL DEFAULT 0.00,
  send_type TINYINT UNSIGNED NOT NULL DEFAULT 0,
  min_amount DECIMAL(10,2) UNSIGNED NOT NULL DEFAULT 0.00,
  max_amount DECIMAL(10,2) UNSIGNED NOT NULL DEFAULT 0.00,
  send_start_date BIGINT UNSIGNED NOT NULL DEFAULT 0,
  send_end_date BIGINT UNSIGNED NOT NULL DEFAULT 0,
  use_start_date BIGINT UNSIGNED NOT NULL DEFAULT 0,
  use_end_date BIGINT UNSIGNED NOT NULL DEFAULT 0,
  min_goods_amount DECIMAL(10,2) UNSIGNED NOT NULL DEFAULT 0.00
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE user_bonus (
  bonus_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  bonus_type_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  bonus_sn BIGINT UNSIGNED NOT NULL,
  user_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  used_time BIGINT UNSIGNED NOT NULL DEFAULT 0,
  order_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  emailed TINYINT UNSIGNED NOT NULL DEFAULT 0,
  UNIQUE KEY uk_user_bonus_sn (bonus_sn),
  KEY idx_user_bonus_user (user_id),
  CONSTRAINT fk_user_bonus_type FOREIGN KEY (bonus_type_id) REFERENCES bonus_type(type_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE user_address (
  address_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NOT NULL,
  consignee VARCHAR(60) NOT NULL,
  country_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  province_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  city_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  district_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  address VARCHAR(255) NOT NULL,
  zipcode VARCHAR(20) NOT NULL DEFAULT '',
  mobile VARCHAR(32) NOT NULL DEFAULT '',
  is_default TINYINT(1) NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_address_user (user_id),
  CONSTRAINT fk_address_user FOREIGN KEY (user_id) REFERENCES users(user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE category (
  cat_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  parent_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  cat_name VARCHAR(120) NOT NULL,
  sort_order INT NOT NULL DEFAULT 50,
  is_show TINYINT(1) NOT NULL DEFAULT 1,
  keywords VARCHAR(255) NOT NULL DEFAULT '',
  cat_desc VARCHAR(255) NOT NULL DEFAULT '',
  KEY idx_category_parent_sort (parent_id, sort_order)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE brand (
  brand_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  brand_name VARCHAR(120) NOT NULL,
  brand_logo VARCHAR(255) NOT NULL DEFAULT '',
  site_url VARCHAR(255) NOT NULL DEFAULT '',
  is_show TINYINT(1) NOT NULL DEFAULT 1,
  sort_order INT NOT NULL DEFAULT 50,
  UNIQUE KEY uk_brand_name (brand_name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE article_cat (
  cat_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  cat_name VARCHAR(120) NOT NULL,
  cat_desc TEXT NOT NULL,
  keywords VARCHAR(255) NOT NULL DEFAULT '',
  sort_order INT NOT NULL DEFAULT 50,
  is_show TINYINT(1) NOT NULL DEFAULT 1
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE article (
  article_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  cat_id BIGINT UNSIGNED NOT NULL,
  title VARCHAR(255) NOT NULL,
  author VARCHAR(255) NOT NULL DEFAULT '',
  article_desc TEXT NOT NULL,
  content LONGTEXT NOT NULL,
  keywords VARCHAR(255) NOT NULL DEFAULT '',
  is_open TINYINT(1) NOT NULL DEFAULT 1,
  KEY idx_article_category_open (cat_id, is_open, article_id),
  CONSTRAINT fk_article_category FOREIGN KEY (cat_id) REFERENCES article_cat(cat_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE goods (
  goods_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  cat_id BIGINT UNSIGNED NOT NULL,
  brand_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  goods_sn VARCHAR(80) NOT NULL DEFAULT '',
  goods_name VARCHAR(255) NOT NULL,
  goods_brief VARCHAR(255) NOT NULL DEFAULT '',
  keywords VARCHAR(255) NOT NULL DEFAULT '',
  goods_desc LONGTEXT NOT NULL,
  shop_price DECIMAL(12,2) NOT NULL,
  market_price DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  goods_number INT NOT NULL DEFAULT 0,
  is_on_sale TINYINT(1) NOT NULL DEFAULT 1,
  is_alone_sale TINYINT(1) NOT NULL DEFAULT 1,
  is_delete TINYINT(1) NOT NULL DEFAULT 0,
  is_best TINYINT(1) NOT NULL DEFAULT 0,
  is_new TINYINT(1) NOT NULL DEFAULT 0,
  is_hot TINYINT(1) NOT NULL DEFAULT 0,
  is_promote TINYINT(1) NOT NULL DEFAULT 0,
  add_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  last_update DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uk_goods_sn (goods_sn),
  KEY idx_goods_listing (cat_id, is_on_sale, is_delete),
  KEY idx_goods_brand (brand_id),
  CONSTRAINT fk_goods_category FOREIGN KEY (cat_id) REFERENCES category(cat_id),
  CONSTRAINT fk_goods_brand FOREIGN KEY (brand_id) REFERENCES brand(brand_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE products (
  product_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  goods_id BIGINT UNSIGNED NOT NULL,
  product_sn VARCHAR(80) NOT NULL DEFAULT '',
  product_number INT NOT NULL DEFAULT 0,
  product_attr TEXT NOT NULL,
  UNIQUE KEY uk_product_sn (product_sn),
  KEY idx_product_goods (goods_id),
  CONSTRAINT fk_product_goods FOREIGN KEY (goods_id) REFERENCES goods(goods_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE goods_attr (
  goods_attr_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  goods_id BIGINT UNSIGNED NOT NULL,
  attr_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  attr_value TEXT NOT NULL,
  attr_price DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  KEY idx_goods_attr_goods (goods_id),
  CONSTRAINT fk_goods_attr_goods FOREIGN KEY (goods_id) REFERENCES goods(goods_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE goods_gallery (
  img_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  goods_id BIGINT UNSIGNED NOT NULL,
  img_url VARCHAR(255) NOT NULL,
  img_desc VARCHAR(255) NOT NULL DEFAULT '',
  sort_order INT NOT NULL DEFAULT 0,
  KEY idx_gallery_goods (goods_id),
  CONSTRAINT fk_gallery_goods FOREIGN KEY (goods_id) REFERENCES goods(goods_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE tag (tag_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,user_id BIGINT UNSIGNED NOT NULL DEFAULT 0,goods_id BIGINT UNSIGNED NOT NULL,tag_words VARCHAR(255) NOT NULL DEFAULT '',authenticated_user_id BIGINT UNSIGNED GENERATED ALWAYS AS (NULLIF(user_id,0)) STORED,KEY idx_tag_words(tag_words),KEY idx_tag_goods(goods_id),UNIQUE KEY uk_tag_authenticated_user_goods_word(authenticated_user_id,goods_id,tag_words)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE booking_goods (rec_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,user_id BIGINT UNSIGNED NOT NULL DEFAULT 0,email VARCHAR(60) NOT NULL DEFAULT '',link_man VARCHAR(60) NOT NULL DEFAULT '',tel VARCHAR(60) NOT NULL DEFAULT '',goods_id BIGINT UNSIGNED NOT NULL DEFAULT 0,goods_desc VARCHAR(255) NOT NULL DEFAULT '',goods_number SMALLINT UNSIGNED NOT NULL DEFAULT 0,booking_time BIGINT UNSIGNED NOT NULL DEFAULT 0,is_dispose TINYINT(1) UNSIGNED NOT NULL DEFAULT 0,dispose_user VARCHAR(30) NOT NULL DEFAULT '',dispose_time BIGINT UNSIGNED NOT NULL DEFAULT 0,dispose_note VARCHAR(255) NOT NULL DEFAULT '',KEY idx_booking_user(user_id),UNIQUE KEY uk_booking_user_goods(user_id,goods_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE cart (
  rec_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NULL,
  session_key CHAR(64) NULL,
  goods_id BIGINT UNSIGNED NOT NULL,
  product_id BIGINT UNSIGNED NULL,
  attr_signature CHAR(64) NOT NULL DEFAULT '',
  goods_attr TEXT NOT NULL,
  goods_number INT NOT NULL,
  version INT NOT NULL DEFAULT 1,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  KEY idx_cart_user (user_id), KEY idx_cart_session (session_key),
  -- The HTTP cart currently represents the no-product/no-attribute variant.
  -- This key makes its increment operation atomic; future product variants
  -- should use a non-null product identity in their own cart key.
  UNIQUE KEY uk_cart_user_goods_base (user_id, goods_id, attr_signature),
  CONSTRAINT fk_cart_user FOREIGN KEY (user_id) REFERENCES users(user_id),
  CONSTRAINT fk_cart_goods FOREIGN KEY (goods_id) REFERENCES goods(goods_id),
  CONSTRAINT fk_cart_product FOREIGN KEY (product_id) REFERENCES products(product_id),
  CONSTRAINT ck_cart_owner CHECK (user_id IS NOT NULL OR session_key IS NOT NULL),
  CONSTRAINT ck_cart_quantity CHECK (goods_number > 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE payment (
  pay_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  pay_name VARCHAR(120) NOT NULL,
  pay_code VARCHAR(60) NOT NULL,
  enabled TINYINT(1) NOT NULL DEFAULT 1,
  pay_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  UNIQUE KEY uk_payment_code (pay_code)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE shipping (
  shipping_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  shipping_name VARCHAR(120) NOT NULL,
  shipping_code VARCHAR(60) NOT NULL,
  enabled TINYINT(1) NOT NULL DEFAULT 1,
  shipping_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  UNIQUE KEY uk_shipping_code (shipping_code)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE order_info (
  order_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  order_sn VARCHAR(40) NOT NULL,
  user_id BIGINT UNSIGNED NOT NULL,
  order_status VARCHAR(32) NOT NULL DEFAULT 'pending_payment',
  shipping_status VARCHAR(32) NOT NULL DEFAULT 'unshipped',
  pay_status VARCHAR(32) NOT NULL DEFAULT 'unpaid',
  consignee VARCHAR(60) NOT NULL, address VARCHAR(255) NOT NULL,
  mobile VARCHAR(32) NOT NULL DEFAULT '',
  shipping_id BIGINT UNSIGNED NULL, pay_id BIGINT UNSIGNED NULL,
  goods_amount DECIMAL(12,2) NOT NULL, shipping_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00, payment_fee DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  discount_amount DECIMAL(12,2) NOT NULL DEFAULT 0.00, order_amount DECIMAL(12,2) NOT NULL,
  idempotency_key VARCHAR(100) NOT NULL,
  request_fingerprint VARCHAR(512) NOT NULL,
  remark VARCHAR(255) NOT NULL DEFAULT '',
  add_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  pay_time DATETIME NULL, shipping_time DATETIME NULL,
  UNIQUE KEY uk_order_sn (order_sn), UNIQUE KEY uk_order_idempotency (user_id, idempotency_key),
  KEY idx_order_user_time (user_id, add_time),
  CONSTRAINT fk_order_user FOREIGN KEY (user_id) REFERENCES users(user_id),
  CONSTRAINT fk_order_shipping FOREIGN KEY (shipping_id) REFERENCES shipping(shipping_id),
  CONSTRAINT fk_order_payment FOREIGN KEY (pay_id) REFERENCES payment(pay_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE order_delivery_address (
  order_id BIGINT UNSIGNED PRIMARY KEY,
  user_id BIGINT UNSIGNED NOT NULL,
  email VARCHAR(60) NOT NULL,
  zipcode VARCHAR(60) NOT NULL DEFAULT '',
  telephone VARCHAR(60) NOT NULL DEFAULT '',
  sign_building VARCHAR(120) NOT NULL DEFAULT '',
  best_time VARCHAR(120) NOT NULL DEFAULT '',
  KEY idx_order_delivery_address_user (user_id, order_id),
  CONSTRAINT fk_order_delivery_address_order FOREIGN KEY (order_id) REFERENCES order_info(order_id) ON DELETE CASCADE,
  CONSTRAINT fk_order_delivery_address_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE order_balance_payment (
  order_id BIGINT UNSIGNED PRIMARY KEY,
  user_id BIGINT UNSIGNED NOT NULL,
  paid_cents BIGINT UNSIGNED NOT NULL DEFAULT 0,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  KEY idx_order_balance_payment_user (user_id, order_id),
  CONSTRAINT fk_order_balance_payment_order FOREIGN KEY (order_id) REFERENCES order_info(order_id) ON DELETE CASCADE,
  CONSTRAINT fk_order_balance_payment_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE order_goods (
  rec_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  order_id BIGINT UNSIGNED NOT NULL, goods_id BIGINT UNSIGNED NOT NULL,
  product_id BIGINT UNSIGNED NULL, goods_name VARCHAR(255) NOT NULL,
  goods_sn VARCHAR(80) NOT NULL DEFAULT '', goods_number INT NOT NULL,
  market_price DECIMAL(12,2) NOT NULL, goods_price DECIMAL(12,2) NOT NULL,
  goods_attr TEXT NOT NULL, KEY idx_order_goods_order (order_id),
  CONSTRAINT fk_order_goods_order FOREIGN KEY (order_id) REFERENCES order_info(order_id),
  CONSTRAINT ck_order_goods_quantity CHECK (goods_number > 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE order_action (
  action_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  order_id BIGINT UNSIGNED NOT NULL, actor_type VARCHAR(20) NOT NULL,
  actor_id BIGINT UNSIGNED NULL, action_note VARCHAR(255) NOT NULL DEFAULT '',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_order_action_order (order_id),
  CONSTRAINT fk_action_order FOREIGN KEY (order_id) REFERENCES order_info(order_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE affiliate_log (
  log_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  order_id BIGINT UNSIGNED NOT NULL,
  user_id BIGINT UNSIGNED NOT NULL,
  user_name VARCHAR(60) NOT NULL DEFAULT '',
  money DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  point BIGINT NOT NULL DEFAULT 0,
  separate_type TINYINT NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uk_affiliate_log_order_user (order_id, user_id),
  KEY idx_affiliate_log_user (user_id, log_id),
  CONSTRAINT fk_affiliate_log_order FOREIGN KEY (order_id) REFERENCES order_info(order_id) ON DELETE CASCADE,
  CONSTRAINT fk_affiliate_log_user FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE pay_log (
  log_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  order_id BIGINT UNSIGNED NOT NULL, provider VARCHAR(60) NOT NULL,
  provider_trade_no VARCHAR(120) NOT NULL, amount DECIMAL(12,2) NOT NULL,
  status VARCHAR(32) NOT NULL, raw_payload JSON NULL,
  received_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uk_provider_trade (provider, provider_trade_no),
  CONSTRAINT fk_paylog_order FOREIGN KEY (order_id) REFERENCES order_info(order_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE comment (
  comment_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,
  comment_type TINYINT UNSIGNED NOT NULL DEFAULT 0,
  id_value BIGINT UNSIGNED NOT NULL,
  user_id BIGINT UNSIGNED NOT NULL,
  user_name VARCHAR(60) NOT NULL,
  content TEXT NOT NULL,
  status TINYINT(1) NOT NULL DEFAULT 1,
  add_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_comment_goods_status (id_value, status, comment_id),
  CONSTRAINT fk_comment_goods FOREIGN KEY (id_value) REFERENCES goods(goods_id),
  CONSTRAINT fk_comment_user FOREIGN KEY (user_id) REFERENCES users(user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE goods_activity (
  act_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT, act_name VARCHAR(255) NOT NULL, act_desc TEXT NOT NULL,
  act_type TINYINT UNSIGNED NOT NULL, goods_id BIGINT UNSIGNED NOT NULL, product_id BIGINT UNSIGNED NOT NULL DEFAULT 0,
  goods_name VARCHAR(255) NOT NULL DEFAULT '', start_time BIGINT UNSIGNED NOT NULL, end_time BIGINT UNSIGNED NOT NULL,
  is_finished TINYINT(1) NOT NULL DEFAULT 0, ext_info TEXT NOT NULL,
  KEY idx_activity_active (act_type,start_time,end_time,is_finished), CONSTRAINT fk_activity_goods FOREIGN KEY(goods_id) REFERENCES goods(goods_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE order_promotion (
  order_id BIGINT UNSIGNED PRIMARY KEY,promotion_id BIGINT UNSIGNED NOT NULL,promotion_type VARCHAR(32) NOT NULL,
  KEY idx_order_promotion_activity (promotion_type,promotion_id,order_id),
  CONSTRAINT fk_order_promotion_order FOREIGN KEY(order_id) REFERENCES order_info(order_id) ON DELETE CASCADE,
  CONSTRAINT fk_order_promotion_activity FOREIGN KEY(promotion_id) REFERENCES goods_activity(act_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE package_goods (package_id BIGINT UNSIGNED NOT NULL,goods_id BIGINT UNSIGNED NOT NULL,product_id BIGINT UNSIGNED NOT NULL DEFAULT 0,goods_number SMALLINT UNSIGNED NOT NULL DEFAULT 1,admin_id BIGINT UNSIGNED NOT NULL DEFAULT 0,PRIMARY KEY(package_id,goods_id,admin_id,product_id),CONSTRAINT fk_package_activity FOREIGN KEY(package_id) REFERENCES goods_activity(act_id),CONSTRAINT fk_package_goods FOREIGN KEY(goods_id) REFERENCES goods(goods_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE favourable_activity (act_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,act_name VARCHAR(255) NOT NULL,start_time BIGINT UNSIGNED NOT NULL,end_time BIGINT UNSIGNED NOT NULL,user_rank VARCHAR(255) NOT NULL DEFAULT '0',act_range TINYINT UNSIGNED NOT NULL DEFAULT 0,act_range_ext VARCHAR(255) NOT NULL DEFAULT '',min_amount DECIMAL(10,2) UNSIGNED NOT NULL DEFAULT 0,max_amount DECIMAL(10,2) UNSIGNED NOT NULL DEFAULT 0,act_type TINYINT UNSIGNED NOT NULL DEFAULT 0,act_type_ext DECIMAL(10,2) UNSIGNED NOT NULL DEFAULT 0,gift TEXT NOT NULL,sort_order TINYINT UNSIGNED NOT NULL DEFAULT 50,KEY ix_favourable_activity_order(sort_order,end_time)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE topic (topic_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,title VARCHAR(255) NOT NULL,intro TEXT NOT NULL,start_time BIGINT UNSIGNED NOT NULL DEFAULT 0,end_time BIGINT UNSIGNED NOT NULL DEFAULT 0,data MEDIUMTEXT NOT NULL,template VARCHAR(255) NOT NULL DEFAULT '',css TEXT NOT NULL,topic_img VARCHAR(255),title_pic VARCHAR(255),base_style CHAR(6),htmls MEDIUMTEXT,keywords VARCHAR(255),description VARCHAR(255),KEY ix_topic_active(start_time,end_time)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE vote (vote_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,vote_name VARCHAR(250) NOT NULL DEFAULT '',start_time BIGINT UNSIGNED NOT NULL DEFAULT 0,end_time BIGINT UNSIGNED NOT NULL DEFAULT 0,can_multi TINYINT UNSIGNED NOT NULL DEFAULT 0,vote_count BIGINT UNSIGNED NOT NULL DEFAULT 0,KEY ix_vote_active(start_time,end_time)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE vote_option (option_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,vote_id BIGINT UNSIGNED NOT NULL,option_name VARCHAR(250) NOT NULL DEFAULT '',option_count BIGINT UNSIGNED NOT NULL DEFAULT 0,option_order TINYINT UNSIGNED NOT NULL DEFAULT 100,KEY ix_vote_option(vote_id,option_order),CONSTRAINT fk_vote_option_vote FOREIGN KEY(vote_id) REFERENCES vote(vote_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE vote_log (log_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,vote_id BIGINT UNSIGNED NOT NULL,ip_address VARCHAR(45) NOT NULL,vote_time BIGINT UNSIGNED NOT NULL,UNIQUE KEY uk_vote_client(vote_id,ip_address),CONSTRAINT fk_vote_log_vote FOREIGN KEY(vote_id) REFERENCES vote(vote_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE exchange_goods (goods_id BIGINT UNSIGNED PRIMARY KEY,exchange_integral BIGINT UNSIGNED NOT NULL DEFAULT 0,is_exchange TINYINT UNSIGNED NOT NULL DEFAULT 0,is_hot TINYINT UNSIGNED NOT NULL DEFAULT 0,KEY ix_exchange_list(is_exchange,exchange_integral),CONSTRAINT fk_exchange_goods FOREIGN KEY(goods_id) REFERENCES goods(goods_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE collect_goods (rec_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,user_id BIGINT UNSIGNED NOT NULL,goods_id BIGINT UNSIGNED NOT NULL,add_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,is_attention TINYINT(1) NOT NULL DEFAULT 0,UNIQUE KEY uk_collect_user_goods(user_id,goods_id),CONSTRAINT fk_collect_user FOREIGN KEY(user_id) REFERENCES users(user_id),CONSTRAINT fk_collect_goods FOREIGN KEY(goods_id) REFERENCES goods(goods_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE feedback (msg_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,parent_id BIGINT UNSIGNED NOT NULL DEFAULT 0,user_id BIGINT UNSIGNED NOT NULL DEFAULT 0,user_name VARCHAR(60) NOT NULL DEFAULT '',user_email VARCHAR(60) NOT NULL DEFAULT '',msg_title VARCHAR(200) NOT NULL DEFAULT '',msg_type TINYINT UNSIGNED NOT NULL DEFAULT 0,msg_status TINYINT UNSIGNED NOT NULL DEFAULT 1,msg_content TEXT NOT NULL,msg_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,message_img VARCHAR(255) NOT NULL DEFAULT '0',order_id BIGINT UNSIGNED NOT NULL DEFAULT 0,msg_area TINYINT UNSIGNED NOT NULL DEFAULT 1,KEY ix_feedback_board(msg_area,msg_status,msg_time),KEY ix_feedback_user(user_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE ad (ad_id BIGINT UNSIGNED PRIMARY KEY AUTO_INCREMENT,position_id BIGINT UNSIGNED NOT NULL DEFAULT 0,media_type TINYINT UNSIGNED NOT NULL DEFAULT 0,ad_name VARCHAR(60) NOT NULL DEFAULT '',ad_link VARCHAR(255) NOT NULL DEFAULT '',ad_code TEXT NOT NULL,start_time DATETIME NOT NULL DEFAULT '1970-01-01 00:00:00',end_time DATETIME NOT NULL DEFAULT '2100-01-01 00:00:00',click_count BIGINT UNSIGNED NOT NULL DEFAULT 0,enabled TINYINT UNSIGNED NOT NULL DEFAULT 1,KEY ix_ad_active(enabled,start_time,end_time)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE adsense (from_ad BIGINT NOT NULL,referer VARCHAR(255) NOT NULL DEFAULT '',clicks BIGINT UNSIGNED NOT NULL DEFAULT 0,PRIMARY KEY(from_ad,referer)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
