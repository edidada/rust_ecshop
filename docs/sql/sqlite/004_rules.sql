CREATE TABLE IF NOT EXISTS ecs_user_rank(rank_id INTEGER PRIMARY KEY,rank_name VARCHAR(60) NOT NULL,discount_percent INTEGER NOT NULL DEFAULT 100 CHECK(discount_percent BETWEEN 0 AND 100));
CREATE TABLE IF NOT EXISTS ecs_member_price(goods_id INTEGER NOT NULL,rank_id INTEGER NOT NULL,user_price DECIMAL(12,2) NOT NULL,PRIMARY KEY(goods_id,rank_id));
CREATE TABLE IF NOT EXISTS ecs_volume_price(goods_id INTEGER NOT NULL,volume_number INTEGER NOT NULL,volume_price DECIMAL(12,2) NOT NULL,PRIMARY KEY(goods_id,volume_number));
CREATE TABLE IF NOT EXISTS ecs_user_membership(user_id INTEGER PRIMARY KEY,rank_id INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS ecs_cart_gift(user_id INTEGER NOT NULL,goods_id INTEGER NOT NULL,quantity INTEGER NOT NULL CHECK(quantity>0),PRIMARY KEY(user_id,goods_id));
CREATE TABLE IF NOT EXISTS ecs_user_write_lock(user_id INTEGER PRIMARY KEY,version INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS ecs_payment_intent(intent_sn VARCHAR(64) PRIMARY KEY,order_id INTEGER NOT NULL,user_id INTEGER NOT NULL,pay_id INTEGER NOT NULL,provider VARCHAR(30) NOT NULL,amount_cents INTEGER NOT NULL CHECK(amount_cents>0),status VARCHAR(20) NOT NULL DEFAULT 'pending',created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_payment_intent_order ON ecs_payment_intent(order_id,status);
INSERT OR IGNORE INTO ecs_user_rank(rank_id,rank_name,discount_percent) VALUES(0,'普通会员',100);
